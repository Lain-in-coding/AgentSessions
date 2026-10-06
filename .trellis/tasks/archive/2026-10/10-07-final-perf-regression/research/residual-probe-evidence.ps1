# B7 residual hot-path probe evidence: two-step (rev-parse + remote get-url) vs
# single-step candidate (remote get-url only). Writes a raw log next to this file.
$ErrorActionPreference = 'Stop'
$base = 'C:\AgentSessions\.trellis\.runtime\b7-bare-probe'
$repo = Join-Path $base 'work'
$bare = Join-Path $base 'asg.git'
$elsewhere = Join-Path $base 'elsewhere'
New-Item -ItemType Directory -Force -Path $elsewhere | Out-Null
if (-not (Test-Path (Join-Path $repo '.git'))) { git -C $base init -q work; git -C $repo remote add origin https://example.com/owner/name.git; New-Item -ItemType Directory -Force -Path (Join-Path $repo 'sub\deep') | Out-Null }
if (-not (Test-Path $bare)) { git -C $base init -q --bare asg.git; git -C $bare remote add origin https://example.com/owner/name.git | Out-Null }
$logLines = New-Object System.Collections.Generic.List[string]
$logLines.Add('B7 residual hot-path probe evidence: current two-step (rev-parse + remote get-url)')
$logLines.Add('vs single-step candidate (remote get-url only). Repro: run this script.')
$logLines.Add('================================================================================')
function Invoke-GitProbe {
    param([string]$Label, [string]$Dir, [string[]]$GitArgs, [hashtable]$ExtraEnv = @{})
    $logLines.Add("### $Label")
    $envDesc = if ($ExtraEnv.Count -gt 0) { ($ExtraEnv.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ', ' } else { '(none)' }
    $logLines.Add("cwd=$Dir | git $($GitArgs -join ' ') | env=$envDesc")
    $saved = @{}
    foreach ($k in $ExtraEnv.Keys) { $saved[$k] = [Environment]::GetEnvironmentVariable($k, 'Process'); [Environment]::SetEnvironmentVariable($k, $ExtraEnv[$k], 'Process') }
    Push-Location $Dir
    $res = & git @GitArgs 2>&1 | Out-String
    $code = $LASTEXITCODE
    Pop-Location
    foreach ($k in $ExtraEnv.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k], 'Process') }
    $logLines.Add("exit=$code")
    $logLines.Add("output=$($res.Trim())")
    $logLines.Add('')
}
Invoke-GitProbe -Label 'worktree repo, nested subdir: step1 toplevel gate (current)' -Dir (Join-Path $repo 'sub\deep') -GitArgs @('rev-parse','--show-toplevel')
Invoke-GitProbe -Label 'worktree repo, nested subdir: step2 remote get-url (current) == candidate single call' -Dir (Join-Path $repo 'sub\deep') -GitArgs @('remote','get-url','origin')
Invoke-GitProbe -Label 'bare repo w/ origin: step1 fails (no work tree) -> current = None' -Dir $bare -GitArgs @('rev-parse','--show-toplevel')
Invoke-GitProbe -Label 'bare repo w/ origin: candidate returns URL -> single-step would widen None -> Some(slug)' -Dir $bare -GitArgs @('remote','get-url','origin')
Invoke-GitProbe -Label 'cwd inside .git/hooks: step1 fails -> current = None' -Dir (Join-Path $repo '.git\hooks') -GitArgs @('rev-parse','--show-toplevel')
Invoke-GitProbe -Label 'cwd inside .git/hooks: candidate returns URL -> widen None -> Some(slug)' -Dir (Join-Path $repo '.git\hooks') -GitArgs @('remote','get-url','origin')
Invoke-GitProbe -Label 'GIT_DIR env set, cwd elsewhere: step1 (current)' -Dir $elsewhere -GitArgs @('rev-parse','--show-toplevel') -ExtraEnv @{ GIT_DIR = (Join-Path $repo '.git') }
Invoke-GitProbe -Label 'GIT_DIR env set, cwd elsewhere: candidate' -Dir $elsewhere -GitArgs @('remote','get-url','origin') -ExtraEnv @{ GIT_DIR = (Join-Path $repo '.git') }
$log = Join-Path $PSScriptRoot 'residual-repo-probe-evidence.log'
[System.IO.File]::WriteAllText($log, ($logLines -join "`n") + "`n")
Write-Output "wrote $log"