# B1 热路径配对实验（get/show/search × 20，双 variant）+ GIT_TRACE 探测计数。
# 用法: pwsh -NoProfile -File measure.ps1 -Binary <abs exe> -Phase before|after [-Runs 20] [-ProbeRuns 3]
param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][ValidateSet('before', 'after')][string]$Phase,
    [int]$Runs = 20,
    [int]$ProbeRuns = 3
)
$ErrorActionPreference = 'Stop'
$repoRoot = 'C:\AgentSessions'
$expDir = Join-Path $repoRoot '.trellis/.runtime/hotpath-experiment-check-b7'
$outDir = Join-Path $repoRoot '.trellis/tasks/10-07-final-perf-regression/research'
New-Item -ItemType Directory -Force -Path $expDir, $outDir | Out-Null
$db = Join-Path $expDir 'fixture.db'
$wireIdFile = Join-Path $expDir 'wire-id.txt'
$clockMs = '1787616000000'   # e2e 同款固定时钟（2026-08-25T00:00:00Z）
$requestId = 'hotpath-diff'

function Invoke-Cli([string[]]$CliArgs, [string]$TracePath = '') {
    if ($TracePath -ne '') {
        [System.Environment]::SetEnvironmentVariable('GIT_TRACE', $TracePath, 'Process')
        if (Test-Path $TracePath) { Remove-Item -LiteralPath $TracePath -Force }
    } else {
        Remove-Item Env:GIT_TRACE -ErrorAction SilentlyContinue
    }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $stdout = & $Binary @CliArgs 2>$null | Out-String
    $sw.Stop()
    $code = $LASTEXITCODE
    return [pscustomobject]@{ Ms = $sw.Elapsed.TotalMilliseconds; Code = $code; Stdout = $stdout.TrimEnd() }
}

function Count-GitProbes([string]$TracePath) {
    if (-not (Test-Path $TracePath)) { return 0 }
    return @((Get-Content -LiteralPath $TracePath) | Where-Object { $_ -match 'built-in: git ' }).Count
}

function Get-Median([double[]]$Values) {
    $sorted = @($Values | Sort-Object)
    $n = $sorted.Count
    if ($n -eq 0) { return $null }
    if ($n % 2 -eq 1) { return $sorted[[int][math]::Floor($n / 2)] }
    return ($sorted[$n / 2 - 1] + $sorted[$n / 2]) / 2.0
}

Remove-Item Env:GIT_TRACE -ErrorAction SilentlyContinue

# 1) 合成夹具（一次）：index 写入一条可检索消息，wire id 复用给 get/show。
if (-not (Test-Path $db)) {
    $seed = Invoke-Cli @('--db', $db, '--robot', '--request-id', 'hotpath-seed', 'index', 'hotpath-fact-1', 'needle in the haystack')
    if ($seed.Code -ne 0) { throw "seed index failed: $($seed.Stdout)" }
    $wire = ($seed.Stdout | ConvertFrom-Json).data.indexed
    if (-not $wire) { throw "seed output missing data.indexed: $($seed.Stdout)" }
    [System.IO.File]::WriteAllText($wireIdFile, $wire)
}
$wireId = (Get-Content -Raw -LiteralPath $wireIdFile).Trim()

$commands = [ordered]@{
    get    = @('get', $wireId)
    show   = @('show', $wireId)
    search = @('search', 'needle')
}
$variants = [ordered]@{
    git_detect                 = 'probe'
    repo_disabled_by_test_seam = 'seam'
}

$sha = (Get-FileHash -LiteralPath $Binary -Algorithm SHA256).Hash.ToLowerInvariant()
$result = [ordered]@{
    phase                  = $Phase
    binary                 = $Binary
    binary_sha256          = $sha
    commit                 = (& git -C $repoRoot rev-parse HEAD)
    cwd                    = $repoRoot
    db                     = $db
    wire_id                = $wireId
    clock_ms               = $clockMs
    runs_per_variant       = $Runs
    probe_runs_per_variant = $ProbeRuns
    method                 = 'Paired process launches, one synthetic catalog entry, fixed ASG_CLOCK_MS, --robot --request-id pinned; variant git_detect = default CWD repo discovery (repo with origin => 2 git subprocesses per resolution round), variant repo_disabled_by_test_seam = ASG_CURRENT_REPO empty (existing test seam, zero probes). Probe counts observed via GIT_TRACE (one built-in line per dispatched git process). Not a universal benchmark.'
    variants               = [ordered]@{}
}

foreach ($variant in $variants.Keys) {
    if ($variant -eq 'git_detect') {
        Remove-Item Env:ASG_CURRENT_REPO -ErrorAction SilentlyContinue
    } else {
        [System.Environment]::SetEnvironmentVariable('ASG_CURRENT_REPO', '', 'Process')
    }
    [System.Environment]::SetEnvironmentVariable('ASG_CLOCK_MS', $clockMs, 'Process')
    $variantData = [ordered]@{}
    foreach ($cmd in $commands.Keys) {
        $cliArgs = @('--db', $db, '--robot', '--request-id', $requestId) + $commands[$cmd]
        $times = @()
        $codes = @()
        for ($i = 0; $i -lt $Runs; $i++) {
            $r = Invoke-Cli $cliArgs
            $times += $r.Ms
            $codes += $r.Code
        }
        $probes = @()
        for ($i = 0; $i -lt $ProbeRuns; $i++) {
            $trace = Join-Path $expDir ("trace-{0}-{1}-{2}.log" -f $Phase, $variant, $cmd)
            $null = Invoke-Cli $cliArgs $trace
            $probes += Count-GitProbes $trace
        }
        $canonical = Invoke-Cli $cliArgs
        $outFile = Join-Path $outDir ("check-{0}-output-{1}-{2}.json" -f $Phase, $variant, $cmd)
        [System.IO.File]::WriteAllText($outFile, $canonical.Stdout + "`n")
        $variantData[$cmd] = [ordered]@{
            raw_ms       = @($times | ForEach-Object { [math]::Round($_, 6) })
            median_ms    = [math]::Round((Get-Median $times), 6)
            min_ms       = [math]::Round(($times | Measure-Object -Minimum).Minimum, 6)
            max_ms       = [math]::Round(($times | Measure-Object -Maximum).Maximum, 6)
            exit_codes   = @($codes | Select-Object -Unique)
            probe_counts = @($probes)
            probe_median = (Get-Median @($probes | ForEach-Object { [double]$_ }))
            output_file  = $outFile
        }
    }
    $result.variants[$variant] = $variantData
}
Remove-Item Env:ASG_CURRENT_REPO -ErrorAction SilentlyContinue
Remove-Item Env:GIT_TRACE -ErrorAction SilentlyContinue

$jsonPath = Join-Path $outDir ("check-{0}-measurements.json" -f $Phase)
[System.IO.File]::WriteAllText($jsonPath, ($result | ConvertTo-Json -Depth 8))
Write-Output "wrote $jsonPath"
