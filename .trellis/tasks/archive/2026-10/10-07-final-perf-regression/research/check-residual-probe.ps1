# B7 check-side residual probe reproduction (reviewer's own contexts + real binary).
$ErrorActionPreference = 'Stop'
$base = 'C:\AgentSessions\.trellis\.runtime\b7-bare-probe-check'
$repo = Join-Path $base 'work'
$bare = Join-Path $base 'asg.git'
$elsewhere = Join-Path $base 'elsewhere'
$db = 'C:\AgentSessions\.trellis\.runtime\hotpath-experiment-check-b7\fixture.db'
$exe = 'C:\AgentSessions\.trellis\.runtime\target-b7\release\agent-session-grep.exe'
New-Item -ItemType Directory -Force -Path $elsewhere | Out-Null
if (-not (Test-Path (Join-Path $repo '.git'))) { git -C $base init -q work; git -C $repo remote add origin https://example.com/owner/name.git; New-Item -ItemType Directory -Force -Path (Join-Path $repo 'sub\deep') | Out-Null }
if (-not (Test-Path $bare)) { git -C $base init -q --bare asg.git; git -C $bare remote add origin https://example.com/owner/name.git | Out-Null }
if (-not (Test-Path $db)) { throw "missing fixture db: $db" }

$log = New-Object System.Collections.Generic.List[string]
$log.Add('# B7 check-side residual probe reproduction (trellis-check, independent contexts)')
$log.Add("base=$base")
$log.Add("git version: $(git --version)")
$log.Add('')

function Probe-RawGit([string]$label, [string]$dir, [string[]]$GitArgs) {
    Push-Location $dir
    $out = & git @GitArgs 2>&1 | Out-String
    $code = $LASTEXITCODE
    Pop-Location
    $log.Add("## raw git: $label")
    $log.Add("cwd=$dir | git $($GitArgs -join ' ')")
    $log.Add("exit=$code")
    $log.Add("output=$($out.Trim())")
    $log.Add('')
}

Probe-RawGit 'worktree nested subdir: step1 (current first gate)' (Join-Path $repo 'sub\deep') @('rev-parse','--show-toplevel')
Probe-RawGit 'worktree nested subdir: step2 / candidate single call' (Join-Path $repo 'sub\deep') @('remote','get-url','origin')
Probe-RawGit 'bare repo with origin: step1' $bare @('rev-parse','--show-toplevel')
Probe-RawGit 'bare repo with origin: candidate single call' $bare @('remote','get-url','origin')
Probe-RawGit 'cwd inside .git/hooks: step1' (Join-Path $repo '.git\hooks') @('rev-parse','--show-toplevel')
Probe-RawGit 'cwd inside .git/hooks: candidate single call' (Join-Path $repo '.git\hooks') @('remote','get-url','origin')

function Probe-Binary([string]$label, [string]$dir, [string]$expectTrace, [hashtable]$ExtraEnv = @{}) {
    $trace = Join-Path $base ("trace-{0}.log" -f ($label -replace '[^A-Za-z0-9]','_'))
    if (Test-Path $trace) { Remove-Item -LiteralPath $trace -Force }
    $envDesc = if ($ExtraEnv.Count -gt 0) { ($ExtraEnv.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ', ' } else { '(none)' }
    $saved = @{}
    [Environment]::SetEnvironmentVariable('GIT_TRACE', $trace, 'Process')
    [Environment]::SetEnvironmentVariable('ASG_CLOCK_MS', '1787616000000', 'Process')
    foreach ($k in $ExtraEnv.Keys) { $saved[$k] = [Environment]::GetEnvironmentVariable($k, 'Process'); [Environment]::SetEnvironmentVariable($k, $ExtraEnv[$k], 'Process') }
    Remove-Item Env:ASG_CURRENT_REPO -ErrorAction SilentlyContinue
    Push-Location $dir
    $out = & $exe '--db' $db '--robot' '--request-id' 'check-residual' 'search' 'needle' 2>$null | Out-String
    $code = $LASTEXITCODE
    Pop-Location
    Remove-Item Env:GIT_TRACE -ErrorAction SilentlyContinue
    foreach ($k in $ExtraEnv.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k], 'Process') }
    $count = 0
    $traceLines = @()
    if (Test-Path $trace) {
        $traceLines = @(Get-Content -LiteralPath $trace | Where-Object { $_ -match 'built-in: git ' })
        $count = $traceLines.Count
    }
    $log.Add("## binary search: $label")
    $log.Add("cwd=$dir | env=$envDesc | exit=$code | spawned git built-ins=$count (expect $expectTrace)")
    foreach ($tl in $traceLines) { $log.Add("  trace: $tl") }
    $ok = ($code -eq 0) -and ($count.ToString() -eq $expectTrace)
    $log.Add("verdict=$(if ($ok) {'MATCH'} else {'MISMATCH'})")
    $log.Add('')
}

Probe-Binary 'worktree subdir (expect 2: rev-parse+remote)' (Join-Path $repo 'sub\deep') '2'
Probe-Binary 'bare repo (expect 1: rev-parse fails -> None, no remote call)' $bare '1'
Probe-Binary 'cwd inside .git/hooks (expect 1: rev-parse fails -> None)' (Join-Path $repo '.git\hooks') '1'
Probe-Binary 'nested dir with GIT_CEILING_DIRECTORIES at probe base (expect 1: walk-up blocked)' $elsewhere '1' @{ GIT_CEILING_DIRECTORIES = $base }

$out = Join-Path 'C:\AgentSessions\.trellis\tasks\10-07-final-perf-regression\research' 'check-residual-repo-probe-evidence.log'
[System.IO.File]::WriteAllText($out, ($log -join "`n") + "`n")
Write-Output "wrote $out"
