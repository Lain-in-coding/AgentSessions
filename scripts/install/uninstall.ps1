#!/usr/bin/env pwsh
# Remove the binary installed by install.ps1. This deletes exactly one file and
# never recurses into a directory: the prefix may be a shared bin directory
# holding other people's tools, and a data root must survive an uninstall.
# Missing binary is success, not failure, so repeated runs and CI are safe.

[CmdletBinding()]
param(
    [string]$Prefix
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

function Fail {
    param([string]$Message)
    Write-Host "uninstall: error: $Message" -ForegroundColor Red
    exit 1
}

$exeSuffix = if ($IsWindows) { '.exe' } else { '' }
$binaryName = "agent-session-grep$exeSuffix"

# Prefix resolution mirrors install.ps1 exactly; a divergence here would leave
# an installed binary that uninstall cannot see.
if ([string]::IsNullOrWhiteSpace($Prefix)) {
    if ($IsWindows) {
        if (-not $env:LOCALAPPDATA) {
            Fail 'LOCALAPPDATA is not set; pass -Prefix <dir> to choose the install directory'
        }
        $Prefix = Join-Path $env:LOCALAPPDATA 'agent-session-grep\bin'
    } else {
        $base = if ($env:XDG_BIN_HOME) { $env:XDG_BIN_HOME } elseif ($env:HOME) { Join-Path $env:HOME '.local/bin' } else { '' }
        if (-not $base) {
            Fail 'neither XDG_BIN_HOME nor HOME is set; pass -Prefix <dir> to choose the install directory'
        }
        $Prefix = $base
    }
}

$target = Join-Path $Prefix $binaryName

if (-not (Test-Path -LiteralPath $target)) {
    Write-Host "uninstall: not installed: $target"
    Write-Host 'uninstall: nothing to do'
    exit 0
}

Remove-Item -LiteralPath $target -Force
if (Test-Path -LiteralPath $target) {
    Fail "could not remove $target"
}

Write-Host "uninstall: removed $target"
Write-Host "uninstall: the directory $Prefix was left in place."
Write-Host 'uninstall: config, data, cache, and logs were not touched. To remove those,'
Write-Host 'uninstall: delete the paths reported by: agent-session-grep --robot config paths'
exit 0
