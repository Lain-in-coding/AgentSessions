# B8 round-2 E2E: real release binary + git shim (exact spawn counts) + cursor
# query_digest (faithful observable of current_repo; see search_query_digest).
$ErrorActionPreference = 'Stop'
$b = 'C:\AgentSessions\.trellis\.runtime\target-b8\release\agent-session-grep.exe'
$db = 'C:\AgentSessions\.trellis\.runtime\b8-check2\fixture2.db'
$shimDir = 'C:\AgentSessions\.trellis\.runtime\b8-check2\gitshim'
$shimLog = 'C:\AgentSessions\.trellis\.runtime\b8-check2\e2e-shim.log'
$realGit = 'C:\Program Files\Git\cmd\git.exe'
$base = Join-Path $env:TEMP 'b8-check2e'
$clock = '1787616000000'

function Invoke-Cli {
    param([string]$Cwd, [string[]]$CliArgs, [hashtable]$Env = @{}, [bool]$UseShim = $false)
    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $b
    $psi.WorkingDirectory = $Cwd
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    foreach ($a in $CliArgs) { $psi.ArgumentList.Add($a) }
    $psi.Environment['ASG_CLOCK_MS'] = $clock
    $null = $psi.Environment.Remove('ASG_CURRENT_REPO')
    if ($UseShim) {
        $psi.Environment['ASG_SHIM_LOG'] = $shimLog
        $psi.Environment['ASG_REAL_GIT'] = $realGit
        $psi.Environment['PATH'] = $shimDir + ';' + $psi.Environment['PATH']
    }
    foreach ($k in $Env.Keys) { $psi.Environment[$k] = [string]$Env[$k] }
    $p = [System.Diagnostics.Process]::Start($psi)
    $stdout = $p.StandardOutput.ReadToEnd()
    $stderr = $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    return [pscustomobject]@{ Code = $p.ExitCode; Stdout = $stdout.Trim(); Stderr = $stderr.Trim() }
}
function Get-QueryDigest([string]$cursor) {
    $seg = $cursor.Split('.')[0]
    $s = $seg.Replace('-', '+').Replace('_', '/')
    while ($s.Length % 4 -ne 0) { $s += '=' }
    $json = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($s))
    return ($json | ConvertFrom-Json).query_digest
}
function Search-Cursor([string]$Cwd, [hashtable]$Env = @{}, [bool]$UseShim = $false, [string]$ReqId = 'check2') {
    if (Test-Path $shimLog) { Remove-Item -LiteralPath $shimLog -Force }
    $r = Invoke-Cli -Cwd $Cwd -CliArgs @('--db', $db, '--robot', '--request-id', $ReqId, 'search', 'needle', '--max-items', '1') -Env $Env -UseShim $UseShim
    if ($r.Code -ne 0) { throw "cli failed ($Cwd): $($r.Stderr)" }
    $j = $r.Stdout | ConvertFrom-Json
    $spawns = @()
    if ($UseShim -and (Test-Path $shimLog)) { $spawns = @(Get-Content -LiteralPath $shimLog) }
    return [pscustomobject]@{ Digest = (Get-QueryDigest $j.page.next_cursor); Spawns = $spawns }
}

$repo = 'C:\AgentSessions'
$dNone = (Search-Cursor $repo @{ ASG_CURRENT_REPO = '' }).Digest
$slugs = @(
    'github.com/synthetic-owner/plain',
    'github.com/synthetic-owner/wt-host',
    'github.com/synthetic-owner/submodule',
    'github.com/synthetic-owner/envrepo',
    'github.com/synthetic-owner/h1-worktree-repo',
    'github.com/synthetic-owner/h1-gitdir',
    'github.com/synthetic-owner/h2-gitdir',
    'github.com/synthetic-owner/r3-detached'
)
$ref = @{ $dNone = 'None' }
foreach ($s in $slugs) { $d = (Search-Cursor $repo @{ ASG_CURRENT_REPO = $s }).Digest; $ref[$d] = $s }
"digest table size = $($ref.Count)"

if (Test-Path -LiteralPath $base) {
    $resolved = (Resolve-Path -LiteralPath $base).Path
    $expectedPrefix = Join-Path $env:TEMP 'b8-check2e'
    if ($resolved -ne $expectedPrefix) { throw "refusing to delete unexpected path: $resolved" }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $base | Out-Null
function New-Repo([string]$Path, [string]$Origin = '') {
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
    $null = & git -C $Path init -q -b main 2>$null
    if ($LASTEXITCODE -ne 0) { throw "init failed $Path" }
    if ($Origin -ne '') { $null = & git -C $Path remote add origin $Origin 2>$null }
    return $Path
}
function Commit-Empty([string]$Path) { $null = & git -C $Path -c user.name=c -c user.email=c@e.invalid commit -q --allow-empty -m i 2>$null }
function New-Detached([string]$Gd, [string]$W, [string]$Origin) {
    New-Item -ItemType Directory -Force -Path $Gd, $W | Out-Null
    $null = & git -C $Gd init -q --bare 2>$null
    $null = & git --git-dir=$Gd config core.bare false 2>$null
    $null = & git --git-dir=$Gd config core.worktree ($W -replace '\\','/') 2>$null
    $null = & git --git-dir=$Gd remote add origin $Origin 2>$null
}
$outside = Join-Path $base 'outside'; New-Item -ItemType Directory -Force -Path $outside | Out-Null
$plain = New-Repo (Join-Path $base 'plain') 'git@github.com:synthetic-owner/plain.git'
$plainSub = Join-Path $plain 'packages/app'; New-Item -ItemType Directory -Force -Path $plainSub | Out-Null
$bare = Join-Path $base 'bare.git'; New-Item -ItemType Directory -Force -Path $bare | Out-Null
$null = & git -C $bare init -q --bare 2>$null
$null = & git -C $bare remote add origin 'git@github.com:synthetic-owner/bare.git' 2>$null
$noOrigin = New-Repo (Join-Path $base 'no-origin')
$nongit = Join-Path $base 'nongit'; New-Item -ItemType Directory -Force -Path $nongit | Out-Null
$wtHost = New-Repo (Join-Path $base 'wt-host') 'https://github.com/synthetic-owner/wt-host.git'; Commit-Empty $wtHost
$wt = Join-Path $base 'wt-linked'
$null = & git -C $wtHost worktree add -q -b wtb $wt 2>$null
$subsrc = New-Repo (Join-Path $base 'subsrc') 'git@github.com:synthetic-owner/sub-src.git'; Commit-Empty $subsrc
$super = New-Repo (Join-Path $base 'super') 'https://github.com/synthetic-owner/super.git'; Commit-Empty $super
$null = (& git -C $super -c protocol.file.allow=always submodule add -q $subsrc submod 2>&1 | Out-String)
$subMod = Join-Path $super 'submod'
$null = & git -C $subMod remote set-url origin 'git@github.com:synthetic-owner/submodule.git' 2>$null
$envRepo = New-Repo (Join-Path $base 'envrepo') 'git@github.com:synthetic-owner/envrepo.git'
$envSub = Join-Path $envRepo 'x/y'; New-Item -ItemType Directory -Force -Path $envSub | Out-Null
$envNested = New-Repo $envSub 'git@github.com:synthetic-owner/nested-under-envrepo.git'
$envPlain = Join-Path $envRepo 'plainno'; New-Item -ItemType Directory -Force -Path $envPlain | Out-Null
$r3g = Join-Path $base 'r3/gd'; $r3w = Join-Path $base 'r3/w'
New-Detached $r3g $r3w 'git@github.com:synthetic-owner/r3-detached.git'
$h1g = Join-Path $base 'h1/gd'; $h1w = Join-Path $base 'h1/w'
New-Detached $h1g $h1w 'git@github.com:synthetic-owner/h1-gitdir.git'
$null = New-Repo $h1w 'git@github.com:synthetic-owner/h1-worktree-repo.git'
$h2g = Join-Path $base 'h2/gd'; $h2w = Join-Path $base 'h2/w'
New-Detached $h2g $h2w 'git@github.com:synthetic-owner/h2-gitdir.git'
New-Item -ItemType Directory -Force -Path (Join-Path $h2w '.git') | Out-Null

$cases = @(
    [pscustomobject]@{ Name='plain root';               Cwd=$plain;    Env=@{};   Expect='github.com/synthetic-owner/plain' }
    [pscustomobject]@{ Name='plain subdir';             Cwd=$plainSub; Env=@{};   Expect='github.com/synthetic-owner/plain' }
    [pscustomobject]@{ Name='bare root';                Cwd=$bare;     Env=@{};   Expect='None' }
    [pscustomobject]@{ Name='git-dir cwd';              Cwd=(Join-Path $plain '.git'); Env=@{}; Expect='None' }
    [pscustomobject]@{ Name='non-git dir';              Cwd=$nongit;   Env=@{};   Expect='None' }
    [pscustomobject]@{ Name='no-origin repo';           Cwd=$noOrigin; Env=@{};   Expect='None' }
    [pscustomobject]@{ Name='linked worktree root';     Cwd=$wt;       Env=@{};   Expect='github.com/synthetic-owner/wt-host' }
    [pscustomobject]@{ Name='submodule root';           Cwd=$subMod;   Env=@{};   Expect='github.com/synthetic-owner/submodule' }
    [pscustomobject]@{ Name='R1 GIT_WORK_TREE only';    Cwd=$envSub;   Env=@{GIT_WORK_TREE=$outside}; Expect='None' }
    [pscustomobject]@{ Name='R2 rel GIT_DIR + WT plain'; Cwd=$envPlain; Env=@{GIT_DIR='.git'; GIT_WORK_TREE=$envRepo}; Expect='None' }
    [pscustomobject]@{ Name='R2b rel GIT_DIR + WT nested'; Cwd=$envNested; Env=@{GIT_DIR='.git'; GIT_WORK_TREE=$envRepo}; Expect='github.com/synthetic-owner/envrepo' }
    [pscustomobject]@{ Name='R3 core.worktree outside'; Cwd=$r3g;      Env=@{};   Expect='None' }
    [pscustomobject]@{ Name='H1 toplevel .git other repo'; Cwd=$h1g;  Env=@{};   Expect='github.com/synthetic-owner/h1-worktree-repo' }
    [pscustomobject]@{ Name='H2 toplevel .git invalid'; Cwd=$h2g;      Env=@{};   Expect='None' }
)
$rows = @()
foreach ($c in $cases) {
    $r = Search-Cursor $c.Cwd $c.Env $true ('check3-' + ($c.Name -replace '[^a-zA-Z0-9]+','-').ToLower())
    $observed = if ($ref.ContainsKey($r.Digest)) { $ref[$r.Digest] } else { "UNKNOWN($($r.Digest))" }
    $verdict = if ($observed -eq $c.Expect) { 'MATCH' } else { 'DRIFT' }
    $rows += [pscustomobject]@{
        Context = $c.Name; Expect = $c.Expect; Observed = $observed; Verdict = $verdict
        Spawns = $r.Spawns.Count; Argv = ($r.Spawns -join ' || ')
    }
}
$fmt = '{0,-32} {1,-40} {2,-42} {3,-7} {4}'
$fmt -f 'context','expected (pre-change)','observed (real binary)','verdict','spawns'
$rows | ForEach-Object { $fmt -f $_.Context, $_.Expect, $_.Observed, $_.Verdict, $_.Spawns }
"`n--- spawn argv per context ---"
$rows | ForEach-Object { "  [$($_.Context)] spawns=$($_.Spawns): $($_.Argv)" }
$drift = @($rows | Where-Object { $_.Verdict -ne 'MATCH' })
"`nVERDICT: matches=$(@($rows | Where-Object { $_.Verdict -eq 'MATCH' }).Count)/$($rows.Count); drifts=$($drift.Count)"
$drift | ForEach-Object { "  DRIFT $($_.Context): expected [$($_.Expect)] observed [$($_.Observed)]" }
$json = 'C:\AgentSessions\.trellis\tasks\10-07-repo-probe-overlap\research\check3-e2e.json'
[System.IO.File]::WriteAllText($json, ([pscustomobject]@{ digest_refs = $ref; rows = $rows } | ConvertTo-Json -Depth 6))
"wrote $json"



