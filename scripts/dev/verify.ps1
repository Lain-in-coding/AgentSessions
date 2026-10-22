<#
.SYNOPSIS
Runs a targeted Rust check or the serial full integration gate.

.DESCRIPTION
During editing, pass -Package to run only
`cargo check -p <package> --all-targets --locked`.

With no -Package, the script runs the repository integration gate in order:
format check, workspace Clippy with warnings denied, then workspace tests.
Cargo build parallelism is capped at four jobs by default and can be lowered
with -Jobs. The script restores the caller's working directory and
CARGO_BUILD_JOBS value when it exits.

.PARAMETER Package
Optional Cargo package name. Supplying it selects targeted check mode.

.PARAMETER Jobs
Maximum Cargo build jobs, from 1 through 4. Defaults to 4.

.EXAMPLE
./scripts/dev/verify.ps1 -Package agent-session-grep-domain

Runs the fast targeted check used while editing one package.

.EXAMPLE
./scripts/dev/verify.ps1

Runs the serial full integration gate with four Cargo jobs.
#>

[CmdletBinding()]
param(
    [string]$Package,
    [ValidateRange(1, 4)]
    [int]$Jobs = 4
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

if ($PSBoundParameters.ContainsKey('Package') -and [string]::IsNullOrWhiteSpace($Package)) {
    throw '-Package must name a Cargo package.'
}

$repoRoot = [System.IO.Path]::GetFullPath($PSScriptRoot)
while (-not (Test-Path -LiteralPath (Join-Path $repoRoot 'Cargo.toml') -PathType Leaf)) {
    $parent = Split-Path -Parent $repoRoot
    if ([string]::IsNullOrWhiteSpace($parent) -or $parent -eq $repoRoot) {
        throw "Cannot locate the repository root from $PSScriptRoot."
    }
    $repoRoot = $parent
}

$cargoCommand = Get-Command -Name cargo -CommandType Application -ErrorAction Stop

function Invoke-Cargo {
    param([string[]]$CargoArguments)

    Write-Host ("verify: cargo {0}" -f ($CargoArguments -join ' '))
    & $script:cargoCommand.Source @CargoArguments
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw ("cargo {0} failed with exit code {1}" -f ($CargoArguments -join ' '), $exitCode)
    }
}

$originalLocation = Get-Location
$originalCargoBuildJobs = [Environment]::GetEnvironmentVariable('CARGO_BUILD_JOBS', 'Process')

try {
    Set-Location -LiteralPath $repoRoot
    $env:CARGO_BUILD_JOBS = [string]$Jobs

    if (-not [string]::IsNullOrWhiteSpace($Package)) {
        Write-Host "verify: targeted check for $Package in $repoRoot (jobs=$Jobs)"
        Invoke-Cargo -CargoArguments @('check', '-p', $Package, '--all-targets', '--locked')
    } else {
        Write-Host "verify: serial full gate in $repoRoot (jobs=$Jobs)"
        Invoke-Cargo -CargoArguments @('fmt', '--all', '--check')
        Invoke-Cargo -CargoArguments @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
        Invoke-Cargo -CargoArguments @('test', '--workspace', '--locked')
        Write-Host 'verify: full gate passed'
    }
} finally {
    [Environment]::SetEnvironmentVariable('CARGO_BUILD_JOBS', $originalCargoBuildJobs, 'Process')
    Set-Location -LiteralPath $originalLocation.Path
}
