# B8 round-2 check: semantic harness with the NARROWED model.
# Model under test (mirrors crates/agent-session-grep-cli/src/repo_identity.rs @ 14b28838):
#   guard = any of GIT_DIR/GIT_WORK_TREE/GIT_COMMON_DIR/GIT_CEILING_DIRECTORIES/
#           GIT_DISCOVERY_ACROSS_FILESYSTEM present (var_os().is_some(), empty counts)
#   miss path (by_directory):
#     if guard -> new = OLD SERIAL (rev-parse -C cwd; probe remote get-url -C toplevel)
#     else    -> overlap pair (-C cwd, both), gate = rev-parse result:
#                gate None -> None (URL discarded)
#                gate Some(T):  adopt cwd-probe URL iff <T>/.git exists
#                               else serial re-probe remote get-url -C T (OLD semantics)
# OLD SERIAL is the pre-change composition. Any DIVERGENT row is a contract drift.
param(
    [string]$Root = 'C:\AgentSessions\.trellis\.runtime\b8-check2\semantics',
    [string]$OutsideBase = "$env:TEMP\b8-check2-outside"
)
$ErrorActionPreference = 'Stop'
$GUARD = @('GIT_DIR','GIT_WORK_TREE','GIT_COMMON_DIR','GIT_CEILING_DIRECTORIES','GIT_DISCOVERY_ACROSS_FILESYSTEM')

function Invoke-Git { param([string[]]$GitArgs)
    $out = (& git @GitArgs 2>$null | Out-String)
    return [pscustomobject]@{ Code = $LASTEXITCODE; Out = $out.Trim() }
}
function Set-EnvExact { param([string]$Name, $Value)
    if ($null -eq $Value) { Remove-Item -LiteralPath "Env:$Name" -ErrorAction SilentlyContinue }
    else { [System.Environment]::SetEnvironmentVariable($Name, $Value, 'Process') }
}
"baseline guard vars present: " + (($GUARD | Where-Object { $null -ne [System.Environment]::GetEnvironmentVariable($_, 'Process') }) -join ',')

$script:rows = @()
function Compare-Context {
    param([string]$Name, [string]$Cwd, [hashtable]$Env = @{}, [string]$Expect = '')
    $saved = @{}
    foreach ($k in $Env.Keys) {
        $saved[$k] = [System.Environment]::GetEnvironmentVariable($k, 'Process')
        [System.Environment]::SetEnvironmentVariable($k, [string]$Env[$k], 'Process')
    }
    try {
        $envPresent = @($GUARD | Where-Object { $null -ne [System.Environment]::GetEnvironmentVariable($_, 'Process') }).Count -gt 0
        # OLD SERIAL (pre-change)
        $gateOld = Invoke-Git @('-C', $Cwd, 'rev-parse', '--show-toplevel')
        $oldProbe = if ($gateOld.Code -eq 0) { Invoke-Git @('-C', $gateOld.Out, 'remote', 'get-url', 'origin') } else { $null }
        $oldResult = if ($gateOld.Code -eq 0 -and $oldProbe.Code -eq 0) { $oldProbe.Out } else { $null }
        # NEW (narrowed)
        if ($envPresent) {
            $newResult = $oldResult
            $newPath = 'serial(env)'
        } else {
            $gateNew = Invoke-Git @('-C', $Cwd, 'rev-parse', '--show-toplevel')
            if ($gateNew.Code -ne 0) { $newResult = $null; $newPath = 'overlap(gate=None)' }
            else {
                $cwdProbe = Invoke-Git @('-C', $Cwd, 'remote', 'get-url', 'origin')
                $rediscoverable = Test-Path -LiteralPath (Join-Path $gateNew.Out '.git')
                if ($rediscoverable) {
                    $newResult = if ($cwdProbe.Code -eq 0) { $cwdProbe.Out } else { $null }
                    $newPath = 'overlap(cwd-url)'
                } else {
                    $serial = Invoke-Git @('-C', $gateNew.Out, 'remote', 'get-url', 'origin')
                    $newResult = if ($serial.Code -eq 0) { $serial.Out } else { $null }
                    $newPath = 'overlap+serial-reprobe'
                }
            }
        }
        $status = if ($null -eq $oldResult -and $null -eq $newResult) { 'EQUAL(None)' }
                  elseif ($null -eq $oldResult) { 'DIFF(old=None)' }
                  elseif ($null -eq $newResult) { 'DIFF(new=None)' }
                  elseif ($oldResult -ceq $newResult) { 'EQUAL' }
                  else { 'DIFF(value)' }
        $lit = if ($Expect -ne '') { if ($null -eq $newResult) { if ($Expect -eq 'None') { 'lit=ok' } else { 'lit=FAIL' } } elseif ($newResult -ceq $Expect) { 'lit=ok' } else { 'lit=FAIL' } } else { '' }
        $envNote = if ($Env.Count -gt 0) { ($Env.Keys | Sort-Object | ForEach-Object { "$_=$($Env[$_])" }) -join ';' } else { '-' }
        $script:rows += [pscustomobject]@{
            Context = $Name; Env = $envNote; Path = $newPath; Status = $status; Lit = $lit
            Old = $oldResult; New = $newResult
        }
    } finally {
        foreach ($k in $Env.Keys) { Set-EnvExact $k $saved[$k] }
    }
}
function New-Repo([string]$Path, [string]$Origin = '') {
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
    $null = & git -C $Path init -q -b main 2>$null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path (Join-Path $Path '.git/HEAD'))) { throw "git init failed at $Path" }
    if ($Origin -ne '') { $null = & git -C $Path remote add origin $Origin 2>$null; if ($LASTEXITCODE -ne 0) { throw "remote add failed at $Path" } }
    return $Path
}
function Commit-Empty([string]$Path) {
    $null = & git -C $Path -c user.name=check -c user.email=check@example.invalid commit -q --allow-empty -m init 2>$null
    if ($LASTEXITCODE -ne 0) { throw "commit failed at $Path" }
}
if (Test-Path $Root) { Remove-Item -LiteralPath $Root -Recurse -Force }
if (Test-Path $OutsideBase) { Remove-Item -LiteralPath $OutsideBase -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Root, $OutsideBase | Out-Null
$op = Invoke-Git @('-C', $OutsideBase, 'rev-parse', '--show-toplevel')
if ($op.Code -eq 0) { throw "OutsideBase inside a repo: $($op.Out)" }

# ---- PRD five contexts (literal expectations) ----
$plain = New-Repo (Join-Path $Root 'plain') 'git@github.com:synthetic-owner/plain.git'
$sub = Join-Path $plain 'packages/app'; New-Item -ItemType Directory -Force -Path $sub | Out-Null
Compare-Context '1a plain root' $plain @{} 'git@github.com:synthetic-owner/plain.git'
Compare-Context '1b plain subdir' $sub @{} 'git@github.com:synthetic-owner/plain.git'
Compare-Context '1c plain subdir trailing slash' ($sub + '/') @{} 'git@github.com:synthetic-owner/plain.git'
$nested = New-Repo (Join-Path $plain 'vendor/inner') 'https://gitlab.example.com/team/inner.git'
Compare-Context '2a nested inner' $nested @{} 'https://gitlab.example.com/team/inner.git'
Compare-Context '2b outer from root' $plain @{} 'git@github.com:synthetic-owner/plain.git'
Compare-Context '2c vendor parent (outer)' (Join-Path $plain 'vendor') @{} 'git@github.com:synthetic-owner/plain.git'
$bare = Join-Path $Root 'bare.git'; New-Item -ItemType Directory -Force -Path $bare | Out-Null
$null = & git -C $bare init -q --bare 2>$null
$null = & git -C $bare remote add origin 'git@github.com:synthetic-owner/bare.git' 2>$null
Compare-Context '3a bare root' $bare @{} 'None'
New-Item -ItemType Directory -Force -Path (Join-Path $bare 'refs/heads') | Out-Null
Compare-Context '3b bare subdir (refs/heads)' (Join-Path $bare 'refs/heads') @{} 'None'
Compare-Context '4a .git dir cwd' (Join-Path $plain '.git') @{} 'None'
New-Item -ItemType Directory -Force -Path (Join-Path $plain '.git/hooks') | Out-Null
Compare-Context '4b .git/hooks cwd' (Join-Path $plain '.git/hooks') @{} 'None'
$nongit = Join-Path $OutsideBase 'nongit'; New-Item -ItemType Directory -Force -Path $nongit | Out-Null
Compare-Context '5a non-git dir (outside any repo)' $nongit @{} 'None'
Compare-Context '5b nonexistent dir' (Join-Path $Root 'does-not-exist') @{} 'None'

# ---- worktree / submodule ----
$wtHost = New-Repo (Join-Path $Root 'wt-host') 'https://github.com/synthetic-owner/wt-host.git'; Commit-Empty $wtHost
$wt = Join-Path $Root 'wt-linked'
$null = & git -C $wtHost worktree add -q -b wt-branch $wt 2>$null
if ($LASTEXITCODE -ne 0) { throw 'worktree add failed' }
Commit-Empty $wt
New-Item -ItemType Directory -Force -Path (Join-Path $wt 'src/deep') | Out-Null
Compare-Context '6a linked worktree root' $wt @{} 'https://github.com/synthetic-owner/wt-host.git'
Compare-Context '6b linked worktree subdir' (Join-Path $wt 'src/deep') @{} 'https://github.com/synthetic-owner/wt-host.git'
Compare-Context '6c host after worktree add' $wtHost @{} 'https://github.com/synthetic-owner/wt-host.git'
$subsrc = New-Repo (Join-Path $Root 'subsrc') 'git@github.com:synthetic-owner/sub-src.git'; Commit-Empty $subsrc
$super = New-Repo (Join-Path $Root 'super') 'https://github.com/synthetic-owner/super.git'; Commit-Empty $super
$smOut = (& git -C $super -c protocol.file.allow=always submodule add -q $subsrc submod 2>&1 | Out-String)
if (-not (Test-Path (Join-Path $super 'submod/.git'))) { throw "submodule add failed: $smOut" }
$null = & git -C (Join-Path $super 'submod') remote set-url origin 'git@github.com:synthetic-owner/submodule.git' 2>$null
New-Item -ItemType Directory -Force -Path (Join-Path $super 'submod/deep/deeper') | Out-Null
Compare-Context '7a submodule root' (Join-Path $super 'submod') @{} 'git@github.com:synthetic-owner/submodule.git'
Compare-Context '7b submodule deep subdir' (Join-Path $super 'submod/deep/deeper') @{} 'git@github.com:synthetic-owner/submodule.git'
Compare-Context '7c superproject from submodule parent' $super @{} 'https://github.com/synthetic-owner/super.git'
Compare-Context '7d inside submodule gitdir' (Join-Path $super '.git/modules/submod') @{} 'git@github.com:synthetic-owner/submodule.git'

# ---- remote config shapes ----
$noOrigin = New-Repo (Join-Path $Root 'no-origin')
Compare-Context '8 no origin' $noOrigin @{} 'None'
$upperOrigin = New-Repo (Join-Path $Root 'upper-origin')
$null = & git -C $upperOrigin remote add ORIGIN 'git@github.com:synthetic-owner/upper.git' 2>$null
Compare-Context '9 uppercase ORIGIN only' $upperOrigin @{} 'None'
$localPath = New-Repo (Join-Path $Root 'local-path-origin') ([System.IO.Path]::GetFullPath((Join-Path $Root 'subsrc')))
Compare-Context '10 local path origin' $localPath @{} ([System.IO.Path]::GetFullPath((Join-Path $Root 'subsrc')))
$instead = New-Repo (Join-Path $Root 'insteadof') 'https://x.example.com/team/rewritten-repo.git'
$null = & git -C $instead config 'url.git@github.com:rewritten'.insteadOf 'https://x.example.com/team/rewritten-repo.git' 2>$null
Compare-Context '11 insteadOf rewrite' $instead @{} 'https://x.example.com/team/rewritten-repo.git'
$pushurlOnly = New-Repo (Join-Path $Root 'pushurl-only') 'https://github.com/synthetic-owner/pushurl.git'
$null = & git -C $pushurlOnly remote set-url --push origin 'https://github.com/synthetic-owner/push-only.git' 2>$null
Compare-Context '12 pushurl differs from fetch url' $pushurlOnly @{} 'https://github.com/synthetic-owner/pushurl.git'
$worktreeCfg = New-Repo (Join-Path $Root 'wtconfig') 'https://github.com/synthetic-owner/wtconfig.git'; Commit-Empty $worktreeCfg
$null = & git -C $worktreeCfg config extensions.worktreeConfig true 2>$null
$null = & git -C $worktreeCfg config --worktree remote.origin.url 'git@github.com:synthetic-owner/wtconfig-worktree.git' 2>$null
Compare-Context '13 worktree-local origin override' $worktreeCfg @{} 'https://github.com/synthetic-owner/wtconfig.git'

# ---- env combos (round-1 R1/R2) -> must now be EQUAL via env guard ----
$envRepo = New-Repo (Join-Path $Root 'envrepo') 'git@github.com:synthetic-owner/envrepo.git'
$envSub = Join-Path $envRepo 'x/y'; New-Item -ItemType Directory -Force -Path $envSub | Out-Null
$outside2 = Join-Path $OutsideBase 'outside'; New-Item -ItemType Directory -Force -Path $outside2 | Out-Null
Compare-Context '14a GIT_DIR absolute, cwd outside' $outside2 @{ GIT_DIR = (Join-Path $envRepo '.git') } 'git@github.com:synthetic-owner/envrepo.git'
Compare-Context '14b GIT_DIR absolute, cwd = repo subdir' $envSub @{ GIT_DIR = (Join-Path $envRepo '.git') } 'git@github.com:synthetic-owner/envrepo.git'
Compare-Context '14c GIT_DIR relative .git, cwd = repo subdir' $envSub @{ GIT_DIR = '.git' }
Compare-Context '14d GIT_DIR absolute + GIT_WORK_TREE=repo, cwd = subdir' $envSub @{ GIT_DIR = (Join-Path $envRepo '.git'); GIT_WORK_TREE = $envRepo } 'git@github.com:synthetic-owner/envrepo.git'
Compare-Context '14e R1: GIT_WORK_TREE only, cwd = repo subdir' $envSub @{ GIT_WORK_TREE = $outside2 }
Compare-Context '14f GIT_CEILING_DIRECTORIES = parent of repo' $envSub @{ GIT_CEILING_DIRECTORIES = $Root } 'git@github.com:synthetic-owner/envrepo.git'
$nestedUnder = New-Repo $envSub 'git@github.com:synthetic-owner/nested-under-envrepo.git'
Compare-Context '15 R2: rel GIT_DIR + GIT_WORK_TREE, cwd = nested repo' $nestedUnder @{ GIT_DIR = '.git'; GIT_WORK_TREE = $envRepo } 'git@github.com:synthetic-owner/envrepo.git'

# ---- R3: core.worktree outside gitdir tree (no env) -> must now be EQUAL ----
$det = Join-Path $OutsideBase 'detached'; $gd = Join-Path $det 'gd'; $w = Join-Path $det 'w'
New-Item -ItemType Directory -Force -Path $gd, $w | Out-Null
$null = & git -C $gd init -q --bare 2>$null
$null = & git --git-dir=$gd config core.bare false 2>$null
$null = & git --git-dir=$gd config core.worktree ($w -replace '\\','/') 2>$null
$null = & git --git-dir=$gd remote add origin 'git@github.com:synthetic-owner/detached.git' 2>$null
Compare-Context '16a R3: core.worktree outside, cwd = gitdir' $gd @{} 'None'
New-Item -ItemType Directory -Force -Path (Join-Path $gd 'sub') | Out-Null
Compare-Context '16b R3: core.worktree outside, cwd = gitdir/sub' (Join-Path $gd 'sub') @{} 'None'
Compare-Context '16c R3: core.worktree outside, cwd = worktree (no .git)' $w @{} 'None'
$cw = New-Repo (Join-Path $Root 'cwrepo') 'git@github.com:synthetic-owner/cwrepo.git'
$null = & git -C $cw config core.worktree ($cw -replace '\\','/') 2>$null
Compare-Context '17a core.worktree=root, cwd = .git' (Join-Path $cw '.git') @{} 'git@github.com:synthetic-owner/cwrepo.git'
Compare-Context '17b core.worktree=root, cwd = subdir' $cw @{} 'git@github.com:synthetic-owner/cwrepo.git'
$bareWt = Join-Path $Root 'bare-worktree'; $bareW = Join-Path $Root 'bare-w'
New-Item -ItemType Directory -Force -Path $bareWt, $bareW | Out-Null
$null = & git -C $bareWt init -q --bare 2>$null
$null = & git --git-dir=$bareWt config core.worktree ($bareW -replace '\\','/') 2>$null
$null = & git --git-dir=$bareWt remote add origin 'git@github.com:synthetic-owner/bare-worktree.git' 2>$null
Compare-Context '18a bare + core.worktree external, cwd = bare dir' $bareWt @{} 'None'
Compare-Context '18b bare + core.worktree external, cwd = bare/sub' (Join-Path $bareWt 'sub') @{} 'None'

# ---- FALSIFICATION of the narrowed guard: <toplevel>/.git exists but leads
#      AWAY from the repo discovered from cwd ----
# H1: detached gitdir gd (core.worktree=W) where W is itself a DIFFERENT repo.
$h1g = Join-Path $Root 'h1/gd'; $h1w = Join-Path $Root 'h1/w'
New-Item -ItemType Directory -Force -Path $h1g, $h1w | Out-Null
$null = & git -C $h1g init -q --bare 2>$null
$null = & git --git-dir=$h1g config core.bare false 2>$null
$null = & git --git-dir=$h1g config core.worktree ($h1w -replace '\\','/') 2>$null
$null = & git --git-dir=$h1g remote add origin 'git@github.com:synthetic-owner/h1-gitdir.git' 2>$null
New-Repo $h1w 'git@github.com:synthetic-owner/h1-worktree-repo.git' | Out-Null
Compare-Context 'H1 toplevel/.git belongs to ANOTHER repo, cwd = gitdir' $h1g
# H2: same layout but W/.git is an INVALID (empty) dir.
$h2g = Join-Path $Root 'h2/gd'; $h2w = Join-Path $Root 'h2/w'
New-Item -ItemType Directory -Force -Path $h2g, $h2w | Out-Null
$null = & git -C $h2g init -q --bare 2>$null
$null = & git --git-dir=$h2g config core.bare false 2>$null
$null = & git --git-dir=$h2g config core.worktree ($h2w -replace '\\','/') 2>$null
$null = & git --git-dir=$h2g remote add origin 'git@github.com:synthetic-owner/h2-gitdir.git' 2>$null
New-Item -ItemType Directory -Force -Path (Join-Path $h2w '.git') | Out-Null
Compare-Context 'H2 toplevel/.git invalid empty dir, cwd = gitdir' $h2g
# H3: W/.git is a POINTER FILE back to gd (separate-git-dir style) -> same repo.
$h3g = Join-Path $Root 'h3/gd'; $h3w = Join-Path $Root 'h3/w'
New-Item -ItemType Directory -Force -Path $h3g, $h3w | Out-Null
$null = & git -C $h3g init -q --bare 2>$null
$null = & git --git-dir=$h3g config core.bare false 2>$null
$null = & git --git-dir=$h3g config core.worktree ($h3w -replace '\\','/') 2>$null
$null = & git --git-dir=$h3g remote add origin 'git@github.com:synthetic-owner/h3-gitdir.git' 2>$null
[System.IO.File]::WriteAllText((Join-Path $h3w '.git'), "gitdir: $($h3g -replace '\\','/')`n")
Compare-Context 'H3 toplevel/.git pointer file back to gitdir, cwd = gitdir' $h3g

"`n===== MATRIX (round 2, narrowed model) ====="
$fmt = '{0,-62} {1,-16} {2,-24} {3,-44}'
$fmt -f 'context','status','new path','old vs new probe target'
$script:rows | ForEach-Object {
    $promise = "{0} | {1}" -f ($_.Old -replace '.*[:/]',''), ($_.New -replace '.*[:/]','')
    if ($promise.Length -gt 42) { $promise = $promise.Substring(0,42) + '..' }
    $fmt -f $_.Context, ($_.Status + ' ' + $_.Lit).Trim(), $_.Path, $promise
}
"`n===== VERDICT ====="
$diff = @($script:rows | Where-Object { $_.Status -like 'DIFF*' })
$litFail = @($script:rows | Where-Object { $_.Lit -eq 'lit=FAIL' })
if ($diff.Count -eq 0) { 'ALL CONTEXTS EQUIVALENT (narrowed model == pre-change serial semantics)' } else { "DIVERGENCES: $($diff.Count)"; $diff | ForEach-Object { "  $($_.Context)`n    old=[$($_.Old)]`n    new=[$($_.New)]`n    env=$($_.Env) path=$($_.Path)" } }
if ($litFail.Count -gt 0) { "LITERAL EXPECTATION FAILURES: $($litFail.Count)"; $litFail | ForEach-Object { "  $($_.Context): new=[$($_.New)]" } }
$json = 'C:\AgentSessions\.trellis\tasks\10-07-repo-probe-overlap\research\check2-semantics.json'
[System.IO.File]::WriteAllText($json, ($script:rows | ConvertTo-Json -Depth 6))
"wrote $json"

