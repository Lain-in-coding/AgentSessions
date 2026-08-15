#!/usr/bin/env pwsh
# Local install gate: install -> smoke -> uninstall -> reinstall -> smoke again,
# against a throwaway prefix. Best-effort wrapper around the existing
# install.ps1 / smoke.ps1 / uninstall.ps1 scripts; emits a JSON result fragment
# into the gate evidence output directory. Never touches PATH or shell profiles.
#
# The reinstall leg proves idempotency: a second install over the same prefix
# must succeed and the reinstalled binary must pass smoke again.

[CmdletBinding()]
param(
    [string]$Prefix,
    [string]$Binary,
    [string]$OutputDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $repoRoot 'scripts/evidence/out'
}
if (-not (Test-Path -LiteralPath $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
}

# install.ps1 takes -Prefix/-SkipBuild; uninstall.ps1 takes only -Prefix.
# install's --skip-build copies target/release/agent-session-grep, so a
# caller-supplied binary is staged into target/release first.
$installArgs = @()
$uninstallArgs = @()
if (-not [string]::IsNullOrWhiteSpace($Prefix)) {
    $installArgs += @('--prefix', $Prefix)
    $uninstallArgs += @('--prefix', $Prefix)
}
if (-not [string]::IsNullOrWhiteSpace($Binary)) {
    if (-not (Test-Path -LiteralPath $Binary)) {
        Write-Host "gate: error: binary not found: $Binary" -ForegroundColor Red
        exit 1
    }
    $repoArtifactDir = Join-Path $repoRoot 'target/release'
    if (-not (Test-Path -LiteralPath $repoArtifactDir)) {
        New-Item -ItemType Directory -Path $repoArtifactDir -Force | Out-Null
    }
    $exeSuffix = if ($IsWindows) { '.exe' } else { '' }
    $repoArtifact = Join-Path $repoArtifactDir "agent-session-grep$exeSuffix"
    Copy-Item -LiteralPath $Binary -Destination $repoArtifact -Force
    $installArgs += @('--skip-build')
}

$script:Failures = 0
$script:Steps = @()

function Record-Step {
    param([string]$Name, [bool]$Ok, [string]$Detail)
    $script:Steps += @{ name = $Name; ok = $Ok; detail = $Detail }
    if (-not $Ok) { $script:Failures++ }
    $status = if ($Ok) { 'pass' } else { 'FAIL' }
    Write-Host "gate: $status  $Name" -ForegroundColor $(if ($Ok) { 'Green' } else { 'Red' })
    if ($Detail) { Write-Host "gate:       $Detail" }
}

$install = Join-Path $PSScriptRoot 'install.ps1'
$smoke = Join-Path $PSScriptRoot 'smoke.ps1'
$uninstall = Join-Path $PSScriptRoot 'uninstall.ps1'
foreach ($script in @($install, $smoke, $uninstall)) {
    if (-not (Test-Path -LiteralPath $script)) {
        Write-Host "gate: error: missing script $script" -ForegroundColor Red
        exit 1
    }
}

# 1. install (build, or skip-build with an explicit binary).
& $install @installArgs
Record-Step 'install' ($LASTEXITCODE -eq 0) "exit=$LASTEXITCODE"

# 2. smoke against the installed binary. The prefix is resolved by the
#    install script itself; resolve it the same way here for the smoke leg.
$exeSuffix = if ($IsWindows) { '.exe' } else { '' }
if ([string]::IsNullOrWhiteSpace($Prefix)) {
    if ($IsWindows) {
        $Prefix = Join-Path $env:LOCALAPPDATA 'agent-session-grep\bin'
    } else {
        $base = if ($env:XDG_BIN_HOME) { $env:XDG_BIN_HOME } elseif ($env:HOME) { Join-Path $env:HOME '.local/bin' } else { '' }
        $Prefix = $base
    }
}
$installed = Join-Path $Prefix "agent-session-grep$exeSuffix"
if (Test-Path -LiteralPath $installed) {
    & $smoke -Binary $installed
    Record-Step 'smoke-installed' ($LASTEXITCODE -eq 0) "exit=$LASTEXITCODE"
} else {
    Record-Step 'smoke-installed' $false "installed binary not found at $installed"
}

# 3. uninstall: exactly the one file, idempotent.
& $uninstall @uninstallArgs
Record-Step 'uninstall' ($LASTEXITCODE -eq 0) "exit=$LASTEXITCODE"

# 4. uninstall again: second run must report "not installed" and exit 0.
& $uninstall @uninstallArgs
Record-Step 'uninstall-idempotent' ($LASTEXITCODE -eq 0) "exit=$LASTEXITCODE"

# 5. reinstall over the same prefix: idempotent install.
& $install @installArgs
Record-Step 'reinstall' ($LASTEXITCODE -eq 0) "exit=$LASTEXITCODE"

# 6. smoke against the reinstalled binary.
if (Test-Path -LiteralPath $installed) {
    & $smoke -Binary $installed
    Record-Step 'smoke-reinstalled' ($LASTEXITCODE -eq 0) "exit=$LASTEXITCODE"
} else {
    Record-Step 'smoke-reinstalled' $false "installed binary not found at $installed"
}

# 7. final uninstall leaves the prefix clean.
& $uninstall @uninstallArgs
Record-Step 'uninstall-final' ($LASTEXITCODE -eq 0) "exit=$LASTEXITCODE"

$os = if ($IsWindows) { 'windows' } elseif ($IsMacOS) { 'macos' } else { 'linux' }
$result = [ordered]@{
    schema_version = 'agent-session-grep.install-gate/v1'
    os             = $os
    pass           = ($script:Failures -eq 0)
    failures       = $script:Failures
    steps          = @($script:Steps)
    generated_at_utc = (Get-Date).ToUniversalTime().ToString('o')
}
$outFile = Join-Path $OutputDir "install-gate-$os.json"
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $outFile -Encoding utf8NoBOM
Write-Host "gate: manifest $outFile"

if ($script:Failures -gt 0) {
    Write-Host "gate: $script:Failures step(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host 'gate: all install-gate steps passed'
exit 0
