#!/usr/bin/env pwsh
# B6 / release-closure: real Windows install -> first run -> upgrade -> uninstall smoke.
#
# Design notes (why this is the shape it is):
#   * It runs against a detached git worktree of the commit under test, so a
#     concurrent edit in the shared checkout cannot change the artifact between
#     steps. -RepoRoot defaults to that worktree.
#   * The upgrade step needs two different versions, and the repository has
#     exactly one committed version (0.1.0). It therefore builds a synthetic
#     successor in a second scratch worktree with [workspace.package] version
#     bumped to 0.1.1. That bump is never committed and the log says so.
#   * The installer is always invoked through the documented path
#     (scripts/install/install.ps1 -Prefix <dir>), never with -SkipBuild, so the
#     smoke exercises the command the docs tell users to run.
#   * First-run commands get a sandboxed APPDATA/LOCALAPPDATA so the evidence
#     shows the paths a clean machine would see and no real profile is used.
#
# Every step writes variables/logs under -EvidenceRoot; summary.json records
# each step's exit code and every assertion.

[CmdletBinding()]
param(
    [string]$RepoRoot = 'C:/AgentSessions-worktrees/b6-release-closure',
    [string]$SuccessorRoot = 'C:/AgentSessions-worktrees/b6-release-successor',
    [string]$EvidenceRoot = 'C:/AgentSessions/.trellis/tasks/10-06-release-closure/research/windows-smoke',
    [string]$Prefix = 'C:/AgentSessions/.trellis/.runtime/b6-smoke/prefix',
    [string]$Sandbox = 'C:/AgentSessions/.trellis/.runtime/b6-smoke/user-profile',
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Assertions own the interpretation of native exit codes.
$PSNativeCommandUseErrorActionPreference = $false

$LogsDir = Join-Path $EvidenceRoot 'logs'
New-Item -ItemType Directory -Force -Path $LogsDir | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Prefix) | Out-Null
New-Item -ItemType Directory -Force -Path $Sandbox | Out-Null

$script:Steps = @()
$script:Assertions = @()
$script:LastExitCode = 0
$script:LastOutput = @()

function Add-Assertion {
    param([string]$Name, [bool]$Passed, [string]$Detail = '')
    $script:Assertions += [ordered]@{ assertion = $Name; passed = $Passed; detail = $Detail }
    if ($Passed) {
        Write-Host "smoke: pass  $Name"
    } else {
        Write-Host "smoke: FAIL  $Name  $Detail" -ForegroundColor Red
    }
}

function Invoke-Captured {
    param(
        [Parameter(Mandatory)][string]$Step,
        [Parameter(Mandatory)][string]$LogName,
        [Parameter(Mandatory)][string[]]$Argv,
        [switch]$AllowFailure
    )
    $logPath = Join-Path $LogsDir $LogName
    $display = ($Argv | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }) -join ' '
    @(
        '',
        "# ==== $Step",
        "# command: $display",
        "# cwd: $(Get-Location)",
        "# started: $((Get-Date).ToString('yyyy-MM-dd HH:mm:ssK'))"
    ) | Add-Content -Encoding utf8NoBOM -Path $logPath
    $exe = $Argv[0]
    $rest = @()
    if ($Argv.Count -gt 1) { $rest = $Argv[1..($Argv.Count - 1)] }
    $output = & $exe @rest 2>&1
    $code = $LASTEXITCODE
    $script:LastExitCode = $code
    $script:LastOutput = @($output | ForEach-Object { "$_" })
    if ($output) { $output | Tee-Object -FilePath $logPath -Append | Out-Host }
    "# exit=$code" | Add-Content -Encoding utf8NoBOM -Path $logPath
    $script:Steps += [ordered]@{ step = $Step; log = $LogName; exit_code = $code }
    if (-not $AllowFailure -and $code -ne 0) {
        throw "step failed: $Step (exit $code) - see $logPath"
    }
}

function Get-Sha256 {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-RobotJson {
    param([string[]]$Lines)
    foreach ($line in $Lines) {
        $trimmed = $line.Trim()
        if ($trimmed.StartsWith('{')) {
            return ($trimmed | ConvertFrom-Json)
        }
    }
    throw 'no JSON envelope found in the captured output'
}

function Set-WorkspaceVersion {
    param([Parameter(Mandatory)][string]$TomlPath, [Parameter(Mandatory)][string]$NewVersion)
    $lines = [System.Collections.Generic.List[string]]::new()
    foreach ($line in (Get-Content -LiteralPath $TomlPath)) { $lines.Add($line) }
    $sectionIndex = -1
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i].Trim() -eq '[workspace.package]') { $sectionIndex = $i; break }
    }
    if ($sectionIndex -lt 0) { throw "no [workspace.package] section in $TomlPath" }
    for ($i = $sectionIndex + 1; $i -lt $lines.Count; $i++) {
        if ($lines[$i].Trim().StartsWith('[')) { break }
        if ($lines[$i] -match '^version\s*=\s*"([^"]+)"\s*$') {
            $oldVersion = $Matches[1]
            $matchedLine = $Matches[0]
            $lines[$i] = $lines[$i] -replace [regex]::Escape($matchedLine), "version = `"$NewVersion`""
            Set-Content -Encoding utf8NoBOM -LiteralPath $TomlPath -Value $lines
            return $oldVersion
        }
    }
    throw "no version key in [workspace.package] of $TomlPath"
}

$repoResolved = (Resolve-Path -LiteralPath $RepoRoot).Path
$head = (& git -C $repoResolved rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw "cannot resolve HEAD in $repoResolved" }
Write-Host "smoke: repo $repoResolved at $head"

# A previous run must not leave managed files behind: remove exactly the two
# command files the installer owns, never the directory and never anything else.
foreach ($name in @('agent-session-grep.exe', 'asg.exe')) {
    $candidate = Join-Path $Prefix $name
    if (Test-Path -LiteralPath $candidate) { Remove-Item -LiteralPath $candidate -Force }
}
$prefixExistedBefore = Test-Path -LiteralPath $Prefix
$sandboxBefore = [ordered]@{
    roaming = Test-Path -LiteralPath (Join-Path $Sandbox 'Roaming')
    local = Test-Path -LiteralPath (Join-Path $Sandbox 'Local')
}

# ---------------------------------------------------------------- step 01
if ($SkipBuild) {
    Write-Host 'smoke: skip  release build (-SkipBuild; see 01-release-build.log)'
} else {
    Push-Location $repoResolved
    try {
        Invoke-Captured -Step 'release build (documented command)' -LogName '01-release-build.log' `
            -Argv @('cargo', 'build', '--release', '--locked', '-p', 'agent-session-grep-cli', '--bin', 'agent-session-grep')
    } finally { Pop-Location }
}
$buildArtifact = Join-Path $repoResolved 'target/release/agent-session-grep.exe'
if (-not (Test-Path -LiteralPath $buildArtifact)) { throw "missing build artifact $buildArtifact" }
$buildHash = Get-Sha256 -Path $buildArtifact
$buildVersion = (& $buildArtifact --version | Out-String).Trim()

# ---------------------------------------------------------------- step 02
Invoke-Captured -Step 'clean install from source (install.ps1 -Prefix)' -LogName '02-install-first.log' `
    -Argv @('pwsh', '-NoProfile', '-File', (Join-Path $repoResolved 'scripts/install/install.ps1'), '-Prefix', $Prefix)
$canonical = Join-Path $Prefix 'agent-session-grep.exe'
$alias = Join-Path $Prefix 'asg.exe'
$installedCanonicalHash = Get-Sha256 -Path $canonical
$installedAliasHash = Get-Sha256 -Path $alias
Add-Assertion 'install: both command names exist' ((Test-Path $canonical) -and (Test-Path $alias))
Add-Assertion 'install: canonical matches the built artifact' ($installedCanonicalHash -eq $buildHash) "built=$buildHash installed=$installedCanonicalHash"
Add-Assertion 'install: alias is byte-identical to canonical' ($installedAliasHash -eq $installedCanonicalHash)

# ------------------------------------------------- step 03/04/05: first run
$env:APPDATA = Join-Path $Sandbox 'Roaming'
$env:LOCALAPPDATA = Join-Path $Sandbox 'Local'

Invoke-Captured -Step 'first run: --version (both command names)' -LogName '03-first-run-version.log' `
    -Argv @($canonical, '--version')
$canonicalVersion = ($script:LastOutput | Out-String).Trim()
Invoke-Captured -Step 'first run: alias --version' -LogName '03-first-run-version.log' `
    -Argv @($alias, '--version')
$aliasVersion = ($script:LastOutput | Out-String).Trim()
Add-Assertion 'first run: both names report the same version' ($canonicalVersion -eq $aliasVersion) "canonical=$canonicalVersion alias=$aliasVersion"
Add-Assertion 'first run: version matches the built artifact' ($canonicalVersion -eq $buildVersion) "build=$buildVersion installed=$canonicalVersion"

Invoke-Captured -Step 'first run: --robot config paths' -LogName '04-first-run-config-paths.log' `
    -Argv @($canonical, '--robot', 'config', 'paths')
$envelope = Get-RobotJson -Lines $script:LastOutput
if (-not $envelope.ok) { throw "config paths returned ok=$($envelope.ok)" }
$paths = $envelope.data
$sandboxRoaming = (Join-Path $Sandbox 'Roaming').Replace('\', '/')
$sandboxLocal = (Join-Path $Sandbox 'Local').Replace('\', '/')
Add-Assertion 'first run: config path is inside the sandboxed profile' ($paths.config.Replace('\', '/').StartsWith($sandboxRoaming)) "config=$($paths.config)"
Add-Assertion 'first run: data/cache/logs paths are inside the sandboxed profile' (
    $paths.data.Replace('\', '/').StartsWith($sandboxLocal) -and
    $paths.cache.Replace('\', '/').StartsWith($sandboxLocal) -and
    $paths.logs.Replace('\', '/').StartsWith($sandboxLocal)
) "data=$($paths.data)"

Invoke-Captured -Step 'first run: doctor (environment only)' -LogName '05-first-run-doctor.log' `
    -Argv @($canonical, 'doctor')
$doctorOutput = ($script:LastOutput | Out-String)
Add-Assertion 'first run: doctor reports the tool and skips the db check' ($doctorOutput -match 'db: not-checked') $doctorOutput.Trim()

$sandboxAfter = [ordered]@{
    roaming = Test-Path -LiteralPath (Join-Path $Sandbox 'Roaming')
    local = Test-Path -LiteralPath (Join-Path $Sandbox 'Local')
}

Invoke-Captured -Step 'surface smoke against the installed pair (robot envelopes, exit codes, MCP handshake)' `
    -LogName '06-surface-smoke.log' `
    -Argv @('pwsh', '-NoProfile', '-File', (Join-Path $repoResolved 'scripts/install/smoke.ps1'), '-Binary', $canonical, '-AliasBinary', $alias)
Add-Assertion 'surface smoke: exit 0' ($script:LastExitCode -eq 0)

# ---------------------------------------------------------------- step 07
# Synthetic successor: a real second version built from a scratch worktree.
if (Test-Path -LiteralPath $SuccessorRoot) {
    $resolved = (Resolve-Path -LiteralPath $SuccessorRoot).Path
    if (-not $resolved.EndsWith('b6-release-successor')) {
        throw "refusing to remove unexpected successor path $resolved"
    }
    & git -C $repoResolved worktree remove --force $resolved
    if ($LASTEXITCODE -ne 0) { throw "could not remove stale successor worktree $resolved" }
}
Invoke-Captured -Step 'upgrade: create synthetic successor worktree' -LogName '07-upgrade-build.log' `
    -Argv @('git', '-C', $repoResolved, 'worktree', 'add', '--detach', $SuccessorRoot, $head)
$successorToml = Join-Path $SuccessorRoot 'Cargo.toml'
$oldVersion = Set-WorkspaceVersion -TomlPath $successorToml -NewVersion '0.1.1'
Add-Content -Encoding utf8NoBOM -Path (Join-Path $LogsDir '07-upgrade-build.log') -Value @(
    "# synthetic version bump in the scratch worktree only: $oldVersion -> 0.1.1",
    "# (Cargo.lock is rewritten by the build below; --locked is deliberately not",
    "#  used here because the committed lockfile does not know this version)"
)
Push-Location $SuccessorRoot
try {
    Invoke-Captured -Step 'upgrade: build the successor artifact (synthetic 0.1.1)' -LogName '07-upgrade-build.log' `
        -Argv @('cargo', 'build', '--release', '-p', 'agent-session-grep-cli', '--bin', 'agent-session-grep')
} finally { Pop-Location }
$successorArtifact = Join-Path $SuccessorRoot 'target/release/agent-session-grep.exe'
$successorHash = Get-Sha256 -Path $successorArtifact
$successorVersion = (& $successorArtifact --version | Out-String).Trim()
Add-Content -Encoding utf8NoBOM -Path (Join-Path $LogsDir '07-upgrade-build.log') -Value @(
    "# successor artifact: $successorArtifact",
    "# successor sha256: $successorHash",
    "# successor --version: $successorVersion"
)
Add-Assertion 'upgrade: successor version differs from the installed version' ($successorVersion -ne $canonicalVersion) "successor=$successorVersion installed=$canonicalVersion"
Add-Assertion 'upgrade: successor artifact hash differs' ($successorHash -ne $buildHash)

# ---------------------------------------------------------------- step 08
Invoke-Captured -Step 'upgrade: re-run the documented installer over the existing install' `
    -LogName '08-upgrade-install.log' `
    -Argv @('pwsh', '-NoProfile', '-File', (Join-Path $SuccessorRoot 'scripts/install/install.ps1'), '-Prefix', $Prefix)
$upgradedCanonicalHash = Get-Sha256 -Path $canonical
$upgradedAliasHash = Get-Sha256 -Path $alias
$upgradedVersion = (& $canonical --version | Out-String).Trim()
$upgradedAliasVersion = (& $alias --version | Out-String).Trim()
$leftovers = @(Get-ChildItem -LiteralPath $Prefix -Filter '*.tmp-*' -Force -ErrorAction SilentlyContinue)
Add-Assertion 'upgrade: installed canonical is the successor artifact' ($upgradedCanonicalHash -eq $successorHash) "expect=$successorHash actual=$upgradedCanonicalHash"
Add-Assertion 'upgrade: alias follows the successor artifact' ($upgradedAliasHash -eq $successorHash)
Add-Assertion 'upgrade: both names report the successor version' (($upgradedVersion -eq $successorVersion) -and ($upgradedAliasVersion -eq $successorVersion)) "canonical=$upgradedVersion alias=$upgradedAliasVersion"
Add-Assertion 'upgrade: no temporary files left in the prefix' ($leftovers.Count -eq 0) (($leftovers | ForEach-Object { $_.Name }) -join ', ')

# ---------------------------------------------------------------- step 09
Invoke-Captured -Step 'uninstall (removes exactly the two managed files)' -LogName '09-uninstall.log' `
    -Argv @('pwsh', '-NoProfile', '-File', (Join-Path $repoResolved 'scripts/install/uninstall.ps1'), '-Prefix', $Prefix)
Add-Assertion 'uninstall: canonical removed' (-not (Test-Path -LiteralPath $canonical))
Add-Assertion 'uninstall: alias removed' (-not (Test-Path -LiteralPath $alias))
Add-Assertion 'uninstall: prefix directory left in place' (Test-Path -LiteralPath $Prefix)
Add-Assertion 'uninstall: sandboxed data root untouched by uninstall' (
    (Test-Path -LiteralPath (Join-Path $Sandbox 'Roaming')) -eq $sandboxAfter.roaming -and
    (Test-Path -LiteralPath (Join-Path $Sandbox 'Local')) -eq $sandboxAfter.local
)

# ---------------------------------------------------------------- step 10
Invoke-Captured -Step 'uninstall again (must be a no-op and exit 0)' -LogName '10-uninstall-again.log' `
    -Argv @('pwsh', '-NoProfile', '-File', (Join-Path $repoResolved 'scripts/install/uninstall.ps1'), '-Prefix', $Prefix)
$secondUninstall = ($script:LastOutput | Out-String)
Add-Assertion 'uninstall twice: reports not installed and exits 0' (($script:LastExitCode -eq 0) -and ($secondUninstall -match 'not installed'))

# ---------------------------------------------------------------- summary
$failed = @($script:Assertions | Where-Object { -not $_.passed })
$summary = [ordered]@{
    schema = 'agent-session-grep.b6-windows-smoke/v1'
    generated_at = (Get-Date).ToString('yyyy-MM-dd HH:mm:ssK')
    host = [ordered]@{
        os = [System.Environment]::OSVersion.VersionString
        architecture = $env:PROCESSOR_ARCHITECTURE
        powershell = $PSVersionTable.PSVersion.ToString()
        cargo = (cargo --version)
        rustc = (rustc --version)
    }
    repo = [ordered]@{ worktree = $repoResolved; commit = $head }
    prefix = $Prefix
    sandbox = [ordered]@{ root = $Sandbox; grew_during_first_run = $sandboxAfter }
    artifact = [ordered]@{ path = $buildArtifact; sha256 = $buildHash; version = $buildVersion }
    successor = [ordered]@{ worktree = $SuccessorRoot; sha256 = $successorHash; version = $successorVersion; note = 'synthetic 0.1.1 bump in a scratch worktree; never committed' }
    steps = $script:Steps
    assertions = $script:Assertions
    failures = $failed.Count
}
$summaryPath = Join-Path $EvidenceRoot 'summary.json'
$summary | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8NoBOM -LiteralPath $summaryPath
Write-Host "smoke: summary written to $summaryPath"
if ($failed.Count -gt 0) {
    Write-Host "smoke: FAILED ($($failed.Count) assertion(s))" -ForegroundColor Red
    exit 1
}
Write-Host 'smoke: OK (all assertions passed)'
exit 0
