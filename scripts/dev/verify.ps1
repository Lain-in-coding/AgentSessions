<#
.SYNOPSIS
Runs a targeted Rust check or test, or the serial full integration gate.

.DESCRIPTION
During editing, pass -Package to run only
`cargo check -p <package> --all-targets --locked`. Add -Test to run that
package's tests instead.

With no -Package, the script runs the repository integration gate in order:
format check, workspace Clippy with warnings denied, then workspace tests.
Test modes use a valid cargo-nextest 0.9.149 or newer with eight test threads
and four build jobs, then run doctests separately with Cargo. Automatic
discovery includes the stable user-local install root
%LOCALAPPDATA%\ASG\tools\nextest. If no candidate exists, the script falls
back to native Cargo tests. Use -NativeTests to bypass nextest explicitly.

Cargo build parallelism is capped at four jobs by default and can be lowered
with -Jobs. The script restores the caller's working directory and
CARGO_BUILD_JOBS value when it exits. This wrapper never installs
cargo-nextest, changes PATH, or modifies global Cargo configuration.

.PARAMETER Package
Optional Cargo package name. Supplying it without -Test selects targeted check
mode; supplying it with -Test selects package-test mode.

.PARAMETER Test
Runs package tests for the package named by -Package.

.PARAMETER NativeTests
For test modes, bypasses cargo-nextest and runs native Cargo tests.

.PARAMETER NextestPath
For test modes, uses this cargo-nextest executable. The path must exist and
report cargo-nextest 0.9.149 or newer.

.PARAMETER Jobs
Maximum Cargo build jobs, from 1 through 4. Defaults to 4.

.EXAMPLE
./scripts/dev/verify.ps1 -Package agent-session-grep-domain

Runs the fast targeted check used while editing one package.

.EXAMPLE
./scripts/dev/verify.ps1 -Package agent-session-grep-domain -Test

Runs that package's tests with the selected test runner.

.EXAMPLE
./scripts/dev/verify.ps1 -NativeTests

Runs the serial full integration gate with native Cargo tests.
#>

[CmdletBinding()]
param(
    [string]$Package,
    [switch]$Test,
    [switch]$NativeTests,
    [string]$NextestPath,
    [ValidateRange(1, 4)]
    [int]$Jobs = 4
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

$packageSupplied = $PSBoundParameters.ContainsKey('Package')
$nextestPathSupplied = $PSBoundParameters.ContainsKey('NextestPath')
$packageCheckMode = $packageSupplied -and -not $Test
$testMode = (-not $packageSupplied) -or $Test
$minimumNextestVersion = [version]'0.9.149'

if ($packageSupplied -and [string]::IsNullOrWhiteSpace($Package)) {
    throw '-Package must name a Cargo package.'
}
if ($Test -and -not $packageSupplied) {
    throw '-Test requires -Package.'
}
if ($packageCheckMode -and ($NativeTests -or $nextestPathSupplied)) {
    throw '-NativeTests and -NextestPath apply only to test modes; add -Test to test a package.'
}
if ($NativeTests -and $nextestPathSupplied) {
    throw '-NativeTests and -NextestPath are mutually exclusive.'
}
if ($nextestPathSupplied -and [string]::IsNullOrWhiteSpace($NextestPath)) {
    throw '-NextestPath must name a cargo-nextest executable.'
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

function Invoke-Nextest {
    param(
        [string]$NextestCommand,
        [string[]]$NextestArguments
    )

    Write-Host ("verify: cargo-nextest {0}" -f ($NextestArguments -join ' '))
    & $NextestCommand @NextestArguments
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw ("cargo-nextest {0} failed with exit code {1}" -f ($NextestArguments -join ' '), $exitCode)
    }
}

function Resolve-CandidatePath {
    param([string]$CandidatePath)

    if ([string]::IsNullOrWhiteSpace($CandidatePath)) {
        return $CandidatePath
    }
    if ([System.IO.Path]::IsPathRooted($CandidatePath)) {
        return [System.IO.Path]::GetFullPath($CandidatePath)
    }
    return [System.IO.Path]::GetFullPath((Join-Path (Get-Location).Path $CandidatePath))
}

function ConvertTo-NextestVersion {
    param([string]$VersionText)

    $match = [regex]::Match(
        $VersionText,
        '(?im)^\s*cargo-nextest\s+v?(?<version>\d+\.\d+\.\d+)\b'
    )
    if (-not $match.Success) {
        $match = [regex]::Match(
            $VersionText,
            '(?im)^\s*release:\s*v?(?<version>\d+\.\d+\.\d+)\b'
        )
    }
    if (-not $match.Success) {
        return $null
    }

    [version]$parsedVersion = $null
    if (-not [version]::TryParse($match.Groups['version'].Value, [ref]$parsedVersion)) {
        return $null
    }
    return $parsedVersion
}

function Get-NextestIdentity {
    param(
        [string]$CandidatePath,
        [string]$Origin,
        [bool]$Explicit
    )

    $displayPath = $CandidatePath
    if ([string]::IsNullOrWhiteSpace($displayPath)) {
        $displayPath = '<empty>'
    }

    $identity = [pscustomobject]@{
        Path = $CandidatePath
        Origin = $Origin
        Version = $null
        Priority = $null
        Valid = $false
        Detail = ''
    }

    if ([string]::IsNullOrWhiteSpace($CandidatePath)) {
        $identity.Detail = 'path is empty'
    } else {
        $resolvedPath = Resolve-CandidatePath -CandidatePath $CandidatePath
        $identity.Path = $resolvedPath
        $displayPath = $resolvedPath

        try {
            $exists = Test-Path -LiteralPath $resolvedPath -PathType Leaf
        } catch {
            $exists = $false
            $identity.Detail = $_.Exception.Message
        }

        if (-not $exists) {
            if ([string]::IsNullOrWhiteSpace($identity.Detail)) {
                $identity.Detail = 'file does not exist'
            }
        } else {
            $versionOutput = @()
            $versionExitCode = 1
            try {
                $versionOutput = @(& $resolvedPath --version 2>&1)
                $exitVariable = Get-Variable -Name LASTEXITCODE -ErrorAction SilentlyContinue
                if ($null -ne $exitVariable) {
                    $versionExitCode = [int]$exitVariable.Value
                }
            } catch {
                $identity.Detail = $_.Exception.Message
            }

            if ($versionExitCode -ne 0) {
                if ([string]::IsNullOrWhiteSpace($identity.Detail)) {
                    $identity.Detail = "version command exited with code $versionExitCode"
                }
            } else {
                $versionText = ($versionOutput | ForEach-Object { [string]$_ }) -join "`n"
                $version = ConvertTo-NextestVersion -VersionText $versionText
                if ($null -eq $version) {
                    $identity.Detail = 'version output is not recognized'
                } elseif ($version -lt $script:minimumNextestVersion) {
                    $identity.Version = $version
                    $identity.Detail = "version $version is older than required $($script:minimumNextestVersion)"
                } else {
                    $identity.Version = $version
                    $identity.Valid = $true
                    $identity.Detail = "version $version"
                }
            }
        }
    }

    if ($Explicit -and -not $identity.Valid) {
        throw ("Explicit cargo-nextest at '{0}' is unusable: {1}. Version {2} or newer is required." -f $displayPath, $identity.Detail, $script:minimumNextestVersion)
    }
    return $identity
}

function Add-NextestCandidate {
    param(
        [System.Collections.Generic.List[object]]$Candidates,
        [hashtable]$SeenPaths,
        [string]$CandidatePath,
        [string]$Origin,
        [int]$Priority
    )

    if ([string]::IsNullOrWhiteSpace($CandidatePath)) {
        return
    }
    $resolvedPath = [System.IO.Path]::GetFullPath($CandidatePath)
    if (-not $SeenPaths.ContainsKey($resolvedPath)) {
        $SeenPaths[$resolvedPath] = $true
        $Candidates.Add([pscustomobject]@{
            Path = $resolvedPath
            Origin = $Origin
            Priority = $Priority
        })
    }
}

function Resolve-Nextest {
    param(
        [bool]$ExplicitPathWasSet,
        [string]$ExplicitPath,
        [string]$RepositoryRoot
    )

    if ($ExplicitPathWasSet) {
        return Get-NextestIdentity -CandidatePath $ExplicitPath -Origin '-NextestPath' -Explicit $true
    }

    $environmentPath = [Environment]::GetEnvironmentVariable('ASG_NEXTEST', 'Process')
    if ($null -ne $environmentPath) {
        return Get-NextestIdentity -CandidatePath $environmentPath -Origin 'ASG_NEXTEST' -Explicit $true
    }

    $candidatePaths = New-Object 'System.Collections.Generic.List[object]'
    $seenPaths = @{}

    $localAppData = [Environment]::GetEnvironmentVariable('LOCALAPPDATA', 'Process')
    if (-not [string]::IsNullOrWhiteSpace($localAppData)) {
        $userInstallRoot = Join-Path $localAppData 'ASG/tools/nextest'
        if (Test-Path -LiteralPath $userInstallRoot -PathType Container) {
            $userInstallDirectories = @(Get-ChildItem -LiteralPath $userInstallRoot -Directory -Filter 'nextest-*' -ErrorAction SilentlyContinue)
            foreach ($userInstallDirectory in $userInstallDirectories) {
                Add-NextestCandidate -Candidates $candidatePaths -SeenPaths $seenPaths -CandidatePath (Join-Path $userInstallDirectory.FullName 'bin/cargo-nextest.exe') -Origin 'LOCALAPPDATA/ASG/tools/nextest' -Priority 0
            }
        }
    }

    $pathCommands = @(Get-Command -Name cargo-nextest -CommandType Application -All -ErrorAction SilentlyContinue)
    foreach ($pathCommand in $pathCommands) {
        $candidatePath = [string]$pathCommand.Source
        if ([string]::IsNullOrWhiteSpace($candidatePath)) {
            $candidatePath = [string]$pathCommand.Path
        }
        Add-NextestCandidate -Candidates $candidatePaths -SeenPaths $seenPaths -CandidatePath $candidatePath -Origin 'PATH' -Priority 1
    }

    $runtimeRoot = Join-Path $RepositoryRoot '.trellis/.runtime'
    if (Test-Path -LiteralPath $runtimeRoot -PathType Container) {
        $runtimeDirectories = @()
        $runtimeDirectories += @(Get-ChildItem -LiteralPath $runtimeRoot -Directory -Filter 'nextest-*' -ErrorAction SilentlyContinue)

        $taskDirectories = @(Get-ChildItem -LiteralPath $runtimeRoot -Directory -ErrorAction SilentlyContinue)
        foreach ($taskDirectory in $taskDirectories) {
            $runtimeDirectories += @(Get-ChildItem -LiteralPath $taskDirectory.FullName -Directory -Filter 'nextest-*' -ErrorAction SilentlyContinue)
        }

        $runtimeDirectories = @($runtimeDirectories | Sort-Object -Property FullName -Unique)
        foreach ($runtimeDirectory in $runtimeDirectories) {
            Add-NextestCandidate -Candidates $candidatePaths -SeenPaths $seenPaths -CandidatePath (Join-Path $runtimeDirectory.FullName 'bin/cargo-nextest.exe') -Origin '.trellis/.runtime' -Priority 2
        }
    }

    if ($candidatePaths.Count -eq 0) {
        return $null
    }

    $validCandidates = @()
    $invalidCandidates = @()
    foreach ($candidate in $candidatePaths) {
        $identity = Get-NextestIdentity -CandidatePath $candidate.Path -Origin $candidate.Origin -Explicit $false
        $identity.Priority = $candidate.Priority
        if ($identity.Valid) {
            $validCandidates += $identity
        } else {
            $invalidCandidates += $identity
        }
    }

    if ($validCandidates.Count -eq 0) {
        $details = ($invalidCandidates | ForEach-Object {
            "{0} ({1}): {2}" -f $_.Path, $_.Origin, $_.Detail
        }) -join '; '
        throw ("cargo-nextest candidates were found, but none is usable. Version {0} or newer is required. {1}" -f $script:minimumNextestVersion, $details)
    }

    return ($validCandidates |
        Sort-Object -Property @{ Expression = { $_.Version }; Descending = $true }, @{ Expression = { $_.Priority }; Descending = $false } |
        Select-Object -First 1)
}

function Invoke-NativeTests {
    param([string[]]$SelectionArguments)

    $cargoArguments = @('test') + $SelectionArguments + @('--locked', '-j', [string]$script:Jobs, '--no-fail-fast')
    Invoke-Cargo -CargoArguments $cargoArguments
}

function Invoke-NextestTests {
    param(
        [string]$NextestCommand,
        [string[]]$SelectionArguments
    )

    $nextestArguments = @('nextest', 'run') + $SelectionArguments + @(
        '--locked',
        '--test-threads', '8',
        '--build-jobs', [string]$script:Jobs,
        '--no-fail-fast'
    )
    Invoke-Nextest -NextestCommand $NextestCommand -NextestArguments $nextestArguments
}

function Invoke-Doctests {
    param([string[]]$SelectionArguments)

    $cargoArguments = @('test') + $SelectionArguments + @('--locked', '--doc', '-j', [string]$script:Jobs, '--no-fail-fast')
    Invoke-Cargo -CargoArguments $cargoArguments
}

$originalLocation = Get-Location
$originalCargoBuildJobs = [Environment]::GetEnvironmentVariable('CARGO_BUILD_JOBS', 'Process')

try {
    Set-Location -LiteralPath $repoRoot
    $env:CARGO_BUILD_JOBS = [string]$Jobs

    $selectedNextest = $null
    if ($testMode -and -not $NativeTests) {
        $selectedNextest = Resolve-Nextest -ExplicitPathWasSet $nextestPathSupplied -ExplicitPath $NextestPath -RepositoryRoot $repoRoot
        if ($null -eq $selectedNextest) {
            Write-Host 'verify: cargo-nextest not found; falling back to native cargo tests'
        } else {
            Write-Host ("verify: using cargo-nextest {0} from {1}: {2}" -f $selectedNextest.Version, $selectedNextest.Origin, $selectedNextest.Path)
        }
    }

    if ($packageSupplied -and -not $Test) {
        Write-Host "verify: targeted check for $Package in $repoRoot (jobs=$Jobs)"
        Invoke-Cargo -CargoArguments @('check', '-p', $Package, '--all-targets', '--locked')
    } elseif ($Test) {
        $selectionArguments = @('-p', $Package)
        Write-Host "verify: targeted tests for $Package in $repoRoot (jobs=$Jobs)"
        if ($null -eq $selectedNextest) {
            Invoke-NativeTests -SelectionArguments $selectionArguments
        } else {
            Invoke-NextestTests -NextestCommand $selectedNextest.Path -SelectionArguments $selectionArguments
            Invoke-Doctests -SelectionArguments $selectionArguments
        }
        Write-Host "verify: package tests passed for $Package"
    } else {
        Write-Host "verify: serial full gate in $repoRoot (jobs=$Jobs)"
        Invoke-Cargo -CargoArguments @('fmt', '--all', '--check')
        Invoke-Cargo -CargoArguments @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')

        $selectionArguments = @('--workspace')
        if ($null -eq $selectedNextest) {
            Invoke-NativeTests -SelectionArguments $selectionArguments
        } else {
            Invoke-NextestTests -NextestCommand $selectedNextest.Path -SelectionArguments $selectionArguments
            Invoke-Doctests -SelectionArguments $selectionArguments
        }
        Write-Host 'verify: full gate passed'
    }
} finally {
    [Environment]::SetEnvironmentVariable('CARGO_BUILD_JOBS', $originalCargoBuildJobs, 'Process')
    Set-Location -LiteralPath $originalLocation.Path
}
