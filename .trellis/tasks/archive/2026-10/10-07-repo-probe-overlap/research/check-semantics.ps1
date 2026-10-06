# B8 check v2: independent semantic falsification harness (fixes v1 env-restore bug).
# For each context, compares pre-change "old serial" git protocol
#   (gate = rev-parse -C cwd; url = remote get-url -C <toplevel>)
# with post-change "new overlap" git protocol
#   (gate = rev-parse -C cwd; url = remote get-url -C cwd)
# The gate command is identical on both sides, so the only possible divergence is
# the URL probe target. Exit codes and trimmed stdout are recorded verbatim.
param(
    [string]$Root = 'C:\AgentSessions\.trellis\.runtime\b8-check-v2',
    [string]$OutsideBase = "$env:TEMP\b8-check-v2-outside"
)
$ErrorActionPreference = 'Stop'

function Invoke-Git {
    param([string[]]$GitArgs)
    $out = (& git @GitArgs 2>$null | Out-String)
    return [pscustomobject]@{ Code = $LASTEXITCODE; Out = $out.Trim() }
}
function Set-EnvExact {
    param([string]$Name, $Value)
    if ($null -eq $Value) { Remove-Item -LiteralPath "Env:$Name" -ErrorAction SilentlyContinue }
    else { [System.Environment]::SetEnvironmentVariable($Name, $Value, 'Process') }
}

$script:rows = @()
function Compare-Context {
    param([string]$Name, [string]$Cwd, [hashtable]$Env = @{})
    $saved = @{}
    foreach ($k in $Env.Keys) {
        $saved[$k] = [System.Environment]::GetEnvironmentVariable($k, 'Process')
        [System.Environment]::SetEnvironmentVariable($k, [string]$Env[$k], 'Process')
    }
    try {
        $gateOld = Invoke-Git @('-C', $Cwd, 'rev-parse', '--show-toplevel')
        $gateNew = Invoke-Git @('-C', $Cwd, 'rev-parse', '--show-toplevel')
        $oldProbe = $null; $newProbe = $null
        if ($gateOld.Code -eq 0) { $oldProbe = Invoke-Git @('-C', $gateOld.Out, 'remote', 'get-url', 'origin') }
        if ($gateNew.Code -eq 0) { $newProbe = Invoke-Git @('-C', $Cwd,     'remote', 'get-url', 'origin') }
        $oldResult = if ($gateOld.Code -eq 0 -and $oldProbe -and $oldProbe.Code -eq 0) { $oldProbe.Out } else { $null }
        $newResult = if ($gateNew.Code -eq 0 -and $newProbe -and $newProbe.Code -eq 0) { $newProbe.Out } else { $null }
        $status = if ($gateOld.Code -ne $gateNew.Code) { 'GATE-DIFF' }
                  elseif ($null -eq $oldResult -and $null -eq $newResult) { 'EQUAL(None)' }
                  elseif ($null -eq $oldResult) { 'DIFF(old=None)' }
                  elseif ($null -eq $newResult) { 'DIFF(new=None)' }
                  elseif ($oldResult -ceq $newResult) { 'EQUAL' }
                  else { 'DIFF(value)' }
        $envNote = if ($Env.Count -gt 0) { ($Env.Keys | Sort-Object | ForEach-Object { "$_=$($Env[$_])" }) -join ';' } else { '-' }
        $script:rows += [pscustomobject]@{
            Context  = $Name
            Env      = $envNote
            Gate     = "$($gateOld.Code):$($gateOld.Out)"
            OldProbe = if ($oldProbe) { "$($oldProbe.Code):$($oldProbe.Out)" } else { '(gate=null)' }
            NewProbe = if ($newProbe) { "$($newProbe.Code):$($newProbe.Out)" } else { '(gate=null)' }
            OldResult = $oldResult
            NewResult = $newResult
            Status   = $status
        }
    } finally {
        foreach ($k in $Env.Keys) { Set-EnvExact $k $saved[$k] }
    }
}

function New-Repo([string]$Path, [string]$Origin = '') {
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
    $null = & git -C $Path init -q -b main 2>$null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path (Join-Path $Path '.git/HEAD'))) { throw "git init failed at $Path" }
    if ($Origin -ne '') {
        $null = & git -C $Path remote add origin $Origin 2>$null
        if ($LASTEXITCODE -ne 0) { throw "git remote add failed at $Path" }
    }
    return $Path
}
function Commit-Empty([string]$Path) {
    $null = & git -C $Path -c user.name=check -c user.email=check@example.invalid commit -q --allow-empty -m init 2>$null
    if ($LASTEXITCODE -ne 0) { throw "commit failed at $Path" }
}

if (Test-Path $Root) { Remove-Item -LiteralPath $Root -Recurse -Force }
if (Test-Path $OutsideBase) { Remove-Item -LiteralPath $OutsideBase -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Root, $OutsideBase | Out-Null

# Sanity: the "outside" base must not be inside any git repository.
$outsideProbe = Invoke-Git @('-C', $OutsideBase, 'rev-parse', '--show-toplevel')
if ($outsideProbe.Code -eq 0) { throw "OutsideBase is inside a repo: $($outsideProbe.Out)" }
"outside base clean (not a git dir): $OutsideBase"

# ---- PRD five contexts -------------------------------------------------
$plain = New-Repo (Join-Path $Root 'plain') 'git@github.com:synthetic-owner/plain.git'
$sub = Join-Path $plain 'packages/app'; New-Item -ItemType Directory -Force -Path $sub | Out-Null
Compare-Context '1a plain root' $plain
Compare-Context '1b plain subdir' $sub
Compare-Context '1c plain subdir trailing slash' ($sub + '/')

$nested = New-Repo (Join-Path $plain 'vendor/inner') 'https://gitlab.example.com/team/inner.git'
Compare-Context '2a nested inner' $nested
Compare-Context '2b outer from root' $plain
Compare-Context '2c vendor parent (outer)' (Join-Path $plain 'vendor')

$bare = Join-Path $Root 'bare.git'
New-Item -ItemType Directory -Force -Path $bare | Out-Null
$null = & git -C $bare init -q --bare 2>$null
if ($LASTEXITCODE -ne 0) { throw 'bare init failed' }
$null = & git -C $bare remote add origin 'git@github.com:synthetic-owner/bare.git' 2>$null
Compare-Context '3a bare root' $bare
New-Item -ItemType Directory -Force -Path (Join-Path $bare 'refs/heads') | Out-Null
Compare-Context '3b bare subdir (refs/heads)' (Join-Path $bare 'refs/heads')

Compare-Context '4a .git dir cwd' (Join-Path $plain '.git')
New-Item -ItemType Directory -Force -Path (Join-Path $plain '.git/hooks') | Out-Null
Compare-Context '4b .git/hooks cwd' (Join-Path $plain '.git/hooks')

$nongit = Join-Path $OutsideBase 'nongit'; New-Item -ItemType Directory -Force -Path $nongit | Out-Null
Compare-Context '5a non-git dir (truly outside any repo)' $nongit
Compare-Context '5b nonexistent dir' (Join-Path $Root 'does-not-exist')

# ---- worktree / submodule ---------------------------------------------
$wtHost = New-Repo (Join-Path $Root 'wt-host') 'https://github.com/synthetic-owner/wt-host.git'
Commit-Empty $wtHost
$wt = Join-Path $Root 'wt-linked'
$null = & git -C $wtHost worktree add -q -b wt-branch $wt 2>$null
if ($LASTEXITCODE -ne 0) { throw 'worktree add failed' }
Commit-Empty $wt
New-Item -ItemType Directory -Force -Path (Join-Path $wt 'src/deep') | Out-Null
Compare-Context '6a linked worktree root' $wt
Compare-Context '6b linked worktree subdir' (Join-Path $wt 'src/deep')
Compare-Context '6c host after worktree add' $wtHost

$subsrc = New-Repo (Join-Path $Root 'subsrc') 'git@github.com:synthetic-owner/sub-src.git'
Commit-Empty $subsrc
$super = New-Repo (Join-Path $Root 'super') 'https://github.com/synthetic-owner/super.git'
Commit-Empty $super
$smOut = (& git -C $super -c protocol.file.allow=always submodule add -q $subsrc submod 2>&1 | Out-String)
if (-not (Test-Path (Join-Path $super 'submod/.git'))) { throw "submodule add failed: $smOut" }
$null = & git -C (Join-Path $super 'submod') remote set-url origin 'git@github.com:synthetic-owner/submodule.git' 2>$null
New-Item -ItemType Directory -Force -Path (Join-Path $super 'submod/deep/deeper') | Out-Null
Compare-Context '7a submodule root' (Join-Path $super 'submod')
Compare-Context '7b submodule deep subdir' (Join-Path $super 'submod/deep/deeper')
Compare-Context '7c superproject from submodule parent' $super
Compare-Context '7d inside submodule gitdir' (Join-Path $super '.git/modules/submod')

# ---- remote config shapes ---------------------------------------------
$noOrigin = New-Repo (Join-Path $Root 'no-origin')
Compare-Context '8 no origin' $noOrigin

$upperOrigin = New-Repo (Join-Path $Root 'upper-origin')
$null = & git -C $upperOrigin remote add ORIGIN 'git@github.com:synthetic-owner/upper.git' 2>$null
Compare-Context '9 uppercase ORIGIN only' $upperOrigin

$localPath = New-Repo (Join-Path $Root 'local-path-origin') ([System.IO.Path]::GetFullPath((Join-Path $Root 'subsrc')))
Compare-Context '10 local path origin' $localPath

$instead = New-Repo (Join-Path $Root 'insteadof') 'https://x.example.com/team/rewritten-repo.git'
$null = & git -C $instead config 'url.git@github.com:rewritten'.insteadOf 'https://x.example.com/team/rewritten-repo.git' 2>$null
Compare-Context '11 insteadOf rewrite (repo-local config)' $instead

$pushurlOnly = New-Repo (Join-Path $Root 'pushurl-only') 'https://github.com/synthetic-owner/pushurl.git'
$null = & git -C $pushurlOnly remote set-url --push origin 'https://github.com/synthetic-owner/push-only.git' 2>$null
Compare-Context '12 pushurl differs from fetch url' $pushurlOnly

$worktreeCfg = New-Repo (Join-Path $Root 'wtconfig') 'https://github.com/synthetic-owner/wtconfig.git'
Commit-Empty $worktreeCfg
$null = & git -C $worktreeCfg config extensions.worktreeConfig true 2>$null
$null = & git -C $worktreeCfg config --worktree remote.origin.url 'git@github.com:synthetic-owner/wtconfig-worktree.git' 2>$null
Compare-Context '13 worktree-local origin override' $worktreeCfg

# ---- env-variable combos ----------------------------------------------
$envRepo = New-Repo (Join-Path $Root 'envrepo') 'git@github.com:synthetic-owner/envrepo.git'
$envSub = Join-Path $envRepo 'x/y'; New-Item -ItemType Directory -Force -Path $envSub | Out-Null
$outside2 = Join-Path $OutsideBase 'outside'; New-Item -ItemType Directory -Force -Path $outside2 | Out-Null
Compare-Context '14a GIT_DIR absolute, cwd truly outside' $outside2 @{ GIT_DIR = (Join-Path $envRepo '.git') }
Compare-Context '14b GIT_DIR absolute, cwd = repo subdir' $envSub @{ GIT_DIR = (Join-Path $envRepo '.git') }
Compare-Context '14c GIT_DIR relative .git, cwd = repo subdir' $envSub @{ GIT_DIR = '.git' }
Compare-Context '14d GIT_DIR absolute + GIT_WORK_TREE=repo, cwd = subdir' $envSub @{ GIT_DIR = (Join-Path $envRepo '.git'); GIT_WORK_TREE = $envRepo }
Compare-Context '14e GIT_WORK_TREE only, cwd = repo subdir' $envSub @{ GIT_WORK_TREE = $outside2 }
Compare-Context '14f GIT_CEILING_DIRECTORIES = parent of repo, cwd = subdir' $envSub @{ GIT_CEILING_DIRECTORIES = $Root }

# Falsification attempt 1: relative GIT_DIR + GIT_WORK_TREE where GIT_DIR
# resolves to *different* repos from cwd vs from the reported toplevel.
$nestedUnder = New-Repo $envSub 'git@github.com:synthetic-owner/nested-under-envrepo.git'
Compare-Context '15 rel GIT_DIR resolves differently from cwd vs toplevel' $nestedUnder @{ GIT_DIR = '.git'; GIT_WORK_TREE = $envRepo }

# Falsification attempt 2: gitdir whose core.worktree points outside the
# discoverable tree; cwd inside the gitdir tree.
$det = Join-Path $Root 'detached'
$gd = Join-Path $det 'gd'; $w = Join-Path $det 'w'
New-Item -ItemType Directory -Force -Path $gd, $w | Out-Null
$null = & git -C $gd init -q --bare 2>$null
$null = & git --git-dir=$gd config core.bare false 2>$null
$null = & git --git-dir=$gd config core.worktree ($w -replace '\\','/') 2>$null
$null = & git --git-dir=$gd remote add origin 'git@github.com:synthetic-owner/detached.git' 2>$null
Compare-Context '16a core.worktree outside gitdir tree, cwd = gitdir' $gd
New-Item -ItemType Directory -Force -Path (Join-Path $gd 'sub') | Out-Null
Compare-Context '16b core.worktree outside gitdir tree, cwd = gitdir/sub' (Join-Path $gd 'sub')
Compare-Context '16c core.worktree outside gitdir tree, cwd = worktree (no .git)' $w

# Falsification attempt 3: normal repo with core.worktree explicitly set to its
# root; cwd = repo/.git (gate historically fails when core.worktree is unset).
$cw = New-Repo (Join-Path $Root 'cwrepo') 'git@github.com:synthetic-owner/cwrepo.git'
$null = & git -C $cw config core.worktree ($cw -replace '\\','/') 2>$null
Compare-Context '17a core.worktree=root, cwd = .git' (Join-Path $cw '.git')
Compare-Context '17b core.worktree=root, cwd = subdir' $cw

# Falsification attempt 4: bare repo with core.worktree set to an external dir.
$bareWt = Join-Path $Root 'bare-worktree'; $bareW = Join-Path $Root 'bare-w'
New-Item -ItemType Directory -Force -Path $bareWt, $bareW | Out-Null
$null = & git -C $bareWt init -q --bare 2>$null
$null = & git --git-dir=$bareWt config core.worktree ($bareW -replace '\\','/') 2>$null
$null = & git --git-dir=$bareWt remote add origin 'git@github.com:synthetic-owner/bare-worktree.git' 2>$null
Compare-Context '18a bare + core.worktree external, cwd = bare dir' $bareWt
New-Item -ItemType Directory -Force -Path (Join-Path $bareWt 'sub') | Out-Null
Compare-Context '18b bare + core.worktree external, cwd = bare/sub' (Join-Path $bareWt 'sub')

"`n===== MATRIX (v2) ====="
$fmt = '{0,-64} {1,-14} {2,-56} {3,-48}'
$fmt -f 'context','status','old probe (-C toplevel)','new probe (-C cwd)'
$script:rows | ForEach-Object {
    $o = "$($_.OldProbe)"; $n = "$($_.NewProbe)"
    if ($o.Length -gt 46) { $o = $o.Substring(0, 46) + '..' }
    if ($n.Length -gt 46) { $n = $n.Substring(0, 46) + '..' }
    $fmt -f $_.Context, $_.Status, $o, $n
}
"`n===== ENV NOTES ====="
$script:rows | Where-Object { $_.Env -ne '-' } | ForEach-Object { "{0} :: {1}" -f $_.Context, $_.Env }
"`n===== VERDICT ====="
$diff = @($script:rows | Where-Object { $_.Status -like 'DIFF*' -or $_.Status -eq 'GATE-DIFF' })
if ($diff.Count -eq 0) { 'ALL CONTEXTS EQUIVALENT (old serial vs new overlap)' }
else {
    "DIVERGENCES: $($diff.Count)"
    $diff | ForEach-Object { "  $($_.Context)`n    cfg: $($_.Env)`n    old=[$($_.OldProbe)]`n    new=[$($_.NewProbe)]" }
}
$json = Join-Path 'C:\AgentSessions\.trellis\tasks\10-07-repo-probe-overlap\research' 'check-semantics.json'
[System.IO.File]::WriteAllText($json, ($script:rows | ConvertTo-Json -Depth 6))
"wrote $json"
