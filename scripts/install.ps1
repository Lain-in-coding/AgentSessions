# agent-session-grep install script (Windows PowerShell)
# Usage: irm https://raw.githubusercontent.com/qin-devs/AgentSessions/main/scripts/install.ps1 | iex
$ErrorActionPreference = "Stop"

$Repo = "qin-devs/AgentSessions"
$BinaryName = "agent-session-grep"
$AliasName = "asg"
$InstallDir = if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { "$env:USERPROFILE\.local\bin" }

Write-Host "agent-session-grep installer (Windows)" -ForegroundColor Cyan
Write-Host "--------------------------------------"

# Detect platform
$Arch = if ([System.Environment]::Is64BitOperatingSystem) { "x64" } else { "x86" }
Write-Host "Platform: windows-$Arch"
Write-Host "Install dir: $InstallDir"

# Create install directory
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Check cargo
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Error: cargo not found. Install Rust: https://rustup.rs" -ForegroundColor Red
    exit 1
}

Write-Host "Building from source..."
$TmpDir = New-Item -ItemType Directory -Path ([System.IO.Path]::GetTempPath() + "asg-install-" + [guid]::NewGuid().ToString()) -Force
$TmpPath = $TmpDir.FullName

try {
    git clone --depth 1 "https://github.com/$Repo.git" "$TmpPath\repo"
    Push-Location "$TmpPath\repo"
    cargo build --release --workspace

    # Install binary + alias
    Copy-Item "target\release\$BinaryName.exe" "$InstallDir\$BinaryName.exe" -Force
    Copy-Item "target\release\$BinaryName.exe" "$InstallDir\$AliasName.exe" -Force
    Pop-Location

    Write-Host ""
    Write-Host "Installed $BinaryName.exe and $AliasName.exe to $InstallDir" -ForegroundColor Green
    Write-Host ""

    # PATH check
    $pathEntries = $env:Path -split ";"
    if ($pathEntries -notcontains $InstallDir) {
        Write-Host "Warning: $InstallDir is not in your PATH." -ForegroundColor Yellow
        Write-Host "Add it manually or run:"
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', $InstallDir + ';' + $env:Path, 'User')"
    }

    Write-Host ""
    Write-Host "Verify: $InstallDir\$BinaryName.exe --version"
    Write-Host "Quickstart: $AliasName.exe sync --discover; $AliasName.exe search hello"
}
finally {
    Remove-Item -Path $TmpPath -Recurse -Force -ErrorAction SilentlyContinue
}
