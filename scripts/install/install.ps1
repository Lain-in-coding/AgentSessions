#!/usr/bin/env pwsh
# Build agent-session-grep from source and copy the binary into a user-level bin
# directory. The artifact is unsigned and unnotarized, so this script is the
# only supported install path; it deliberately does NOT touch PATH, the
# registry, or shell profiles — environment changes are the user's decision and
# would make uninstall irreversible.

[CmdletBinding()]
param(
    [string]$Prefix,
    [switch]$SkipBuild,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# We assert on native exit codes ourselves; PowerShell must not turn a non-zero
# exit into a terminating error before we can report a usable diagnostic.
$PSNativeCommandUseErrorActionPreference = $false

function Fail {
    param([string]$Message)
    Write-Host "install: error: $Message" -ForegroundColor Red
    exit 1
}

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not (Test-Path -LiteralPath (Join-Path $repoRoot 'Cargo.toml'))) {
    Fail "cannot find Cargo.toml at $repoRoot; run this script from a checkout of the repository"
}

$exeSuffix = if ($IsWindows) { '.exe' } else { '' }
$binaryName = "agent-session-grep$exeSuffix"

if ([string]::IsNullOrWhiteSpace($Prefix)) {
    if ($IsWindows) {
        if (-not $env:LOCALAPPDATA) {
            Fail 'LOCALAPPDATA is not set; pass -Prefix <dir> to choose an install directory'
        }
        $Prefix = Join-Path $env:LOCALAPPDATA 'agent-session-grep\bin'
    } else {
        $base = if ($env:XDG_BIN_HOME) { $env:XDG_BIN_HOME } elseif ($env:HOME) { Join-Path $env:HOME '.local/bin' } else { '' }
        if (-not $base) {
            Fail 'neither XDG_BIN_HOME nor HOME is set; pass -Prefix <dir> to choose an install directory'
        }
        $Prefix = $base
    }
}

$artifact = Join-Path $repoRoot (Join-Path 'target/release' $binaryName)

if (-not $SkipBuild) {
    # cargo is only required on the build path; -SkipBuild exists precisely for
    # hosts that already have the artifact.
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargo) {
        Fail 'cargo not found on PATH; install a Rust toolchain from https://rustup.rs and re-run'
    }
    & cargo --version
    if ($LASTEXITCODE -ne 0) { Fail "cargo --version failed with exit code $LASTEXITCODE" }

    if ($DryRun) {
        Write-Host "install: dry run: would run cargo build --locked --release -p agent-session-grep-cli in $repoRoot"
    } else {
        $buildCode = 1
        Push-Location $repoRoot
        try {
            # --locked keeps the build reproducible: an install must never
            # silently resolve different dependency versions than CI did.
            & cargo build --locked --release -p agent-session-grep-cli
            $buildCode = $LASTEXITCODE
        } finally {
            Pop-Location
        }
        if ($buildCode -ne 0) { Fail "cargo build failed with exit code $buildCode" }
    }
}

if ($DryRun -and -not (Test-Path -LiteralPath $artifact)) {
    Write-Host "install: dry run: no file written"
    Write-Host "install: dry run: would install $artifact -> $(Join-Path $Prefix $binaryName)"
    exit 0
}

if (-not (Test-Path -LiteralPath $artifact)) {
    if ($SkipBuild) {
        Fail "$artifact does not exist; -SkipBuild requires a prior cargo build --locked --release -p agent-session-grep-cli"
    }
    Fail "$artifact does not exist after a successful build"
}

$sha256 = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
$target = Join-Path $Prefix $binaryName

if ($DryRun) {
    Write-Host "install: dry run: no file written"
    Write-Host "install: dry run: would create directory $Prefix"
    Write-Host "install: dry run: would copy $artifact -> $target"
    Write-Host "install: dry run: artifact sha256 $sha256"
    exit 0
}

if (-not (Test-Path -LiteralPath $Prefix)) {
    New-Item -ItemType Directory -Path $Prefix -Force | Out-Null
}

# 原子替换：先把新二进制落到同目录临时文件，再经 Move-Item 换名覆盖目标。
# 直接 Copy-Item -Force 覆盖运行中的可执行文件在 Windows 上会因 EBUSY 失败，
# 或留下目标被截断的窗口；先写后换名让读取方永远看到完整文件（install design）。
$tempTarget = Join-Path $Prefix ".$binaryName.tmp-$(Get-Random)"
try {
    Copy-Item -LiteralPath $artifact -Destination $tempTarget -Force
    Move-Item -LiteralPath $tempTarget -Destination $target -Force
} finally {
    if (Test-Path -LiteralPath $tempTarget) {
        Remove-Item -LiteralPath $tempTarget -Force -ErrorAction SilentlyContinue
    }
}

$versionLine = & $target --version
$versionCode = $LASTEXITCODE
if ($versionCode -ne 0) {
    Fail "installed binary failed its self check: $target --version exited $versionCode"
}

$pathHint = if ($IsWindows) {
    "`$env:PATH = `"$Prefix;`$env:PATH`""
} else {
    "export PATH=`"${Prefix}:`$PATH`""
}

Write-Host "install: installed $($versionLine -join ' ')"
Write-Host "install: path     $target"
Write-Host "install: sha256   $sha256"
Write-Host "install: this script does not modify PATH. To use the binary by name in"
Write-Host "install: the current shell session, run:"
Write-Host "install:   $pathHint"
Write-Host "install: to make it permanent, add $Prefix to PATH yourself."
Write-Host "install: the artifact is unsigned and unnotarized."
exit 0
