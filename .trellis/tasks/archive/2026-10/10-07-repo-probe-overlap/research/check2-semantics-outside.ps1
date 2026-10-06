# B8 round-2b check: detached-gitdir family in a location TRULY outside any repo,
# so "pre-change serial" literals are uncontaminated by an enclosing repository.
param([string]$Base = "$env:TEMP\b8-check2b")
$ErrorActionPreference = 'Stop'
function Invoke-Git { param([string[]]$GitArgs)
    $out = (& git @GitArgs 2>$null | Out-String)
    return [pscustomobject]@{ Code = $LASTEXITCODE; Out = $out.Trim() }
}
$script:rows = @()
function Compare-Context {
    param([string]$Name, [string]$Cwd, [string]$Expect)
    $gateOld = Invoke-Git @('-C', $Cwd, 'rev-parse', '--show-toplevel')
    $oldProbe = if ($gateOld.Code -eq 0) { Invoke-Git @('-C', $gateOld.Out, 'remote', 'get-url', 'origin') } else { $null }
    $oldResult = if ($gateOld.Code -eq 0 -and $oldProbe.Code -eq 0) { $oldProbe.Out } else { $null }
    $gateNew = Invoke-Git @('-C', $Cwd, 'rev-parse', '--show-toplevel')
    if ($gateNew.Code -ne 0) { $newResult = $null; $path = 'overlap(gate=None)' }
    else {
        $cwdProbe = Invoke-Git @('-C', $Cwd, 'remote', 'get-url', 'origin')
        if (Test-Path -LiteralPath (Join-Path $gateNew.Out '.git')) {
            $newResult = if ($cwdProbe.Code -eq 0) { $cwdProbe.Out } else { $null }; $path = 'overlap(cwd-url)'
        } else {
            $serial = Invoke-Git @('-C', $gateNew.Out, 'remote', 'get-url', 'origin')
            $newResult = if ($serial.Code -eq 0) { $serial.Out } else { $null }; $path = 'overlap+serial-reprobe'
        }
    }
    $status = if ($null -eq $oldResult -and $null -eq $newResult) { 'EQUAL(None)' }
              elseif ($null -eq $oldResult) { 'DIFF(old=None)' } elseif ($null -eq $newResult) { 'DIFF(new=None)' }
              elseif ($oldResult -ceq $newResult) { 'EQUAL' } else { 'DIFF(value)' }
    $lit = if ($null -eq $newResult) { if ($Expect -eq 'None') { 'lit=ok' } else { 'lit=FAIL' } }
           elseif ($newResult -ceq $Expect) { 'lit=ok' } else { 'lit=FAIL' }
    $gateNote = if ($gateOld.Code -eq 0) { $gateOld.Out } else { '(none)' }
    $script:rows += [pscustomobject]@{ Context=$Name; Status=$status; Path=$path; Lit=$lit
        Gate=$gateNote; Old=$oldResult; New=$newResult }
}
function Init-Bare([string]$Path) {
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
    $null = & git -C $Path init -q --bare 2>$null
    if ($LASTEXITCODE -ne 0) { throw "bare init failed: $Path" }
    $null = & git --git-dir=$Path config core.bare false 2>$null
}
function Set-Worktree([string]$GitDir, [string]$W) { $null = & git --git-dir=$GitDir config core.worktree ($W -replace '\\','/') 2>$null }
function Set-Origin([string]$GitDir, [string]$Url) { $null = & git --git-dir=$GitDir remote add origin $Url 2>$null }

if (Test-Path $Base) { Remove-Item -LiteralPath $Base -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Base | Out-Null
$probe = Invoke-Git @('-C', $Base, 'rev-parse', '--show-toplevel')
if ($probe.Code -eq 0) { throw "base is inside a repo: $($probe.Out)" }

# R3: core.worktree outside gitdir tree, W has NO .git
$r3g = Join-Path $Base 'r3/gd'; $r3w = Join-Path $Base 'r3/w'
New-Item -ItemType Directory -Force -Path $r3w | Out-Null
Init-Bare $r3g; Set-Worktree $r3g $r3w; Set-Origin $r3g 'git@github.com:synthetic-owner/r3-detached.git'
Compare-Context 'R3a core.worktree outside, cwd = gitdir' $r3g 'None'
New-Item -ItemType Directory -Force -Path (Join-Path $r3g 'sub') | Out-Null
Compare-Context 'R3b core.worktree outside, cwd = gitdir/sub' (Join-Path $r3g 'sub') 'None'
Compare-Context 'R3c core.worktree outside, cwd = worktree(no .git)' $r3w 'None'

# H1: W is itself a different repo
$h1g = Join-Path $Base 'h1/gd'; $h1w = Join-Path $Base 'h1/w'
Init-Bare $h1g; Set-Worktree $h1g $h1w
New-Item -ItemType Directory -Force -Path $h1w | Out-Null
$null = & git -C $h1w init -q -b main 2>$null
$null = & git -C $h1w remote add origin 'git@github.com:synthetic-owner/h1-worktree-repo.git' 2>$null
Set-Origin $h1g 'git@github.com:synthetic-owner/h1-gitdir.git'
Compare-Context 'H1 toplevel/.git belongs to ANOTHER repo, cwd = gitdir' $h1g 'git@github.com:synthetic-owner/h1-worktree-repo.git'
# H2: W/.git invalid empty dir
$h2g = Join-Path $Base 'h2/gd'; $h2w = Join-Path $Base 'h2/w'
Init-Bare $h2g; Set-Worktree $h2g $h2w
New-Item -ItemType Directory -Force -Path (Join-Path $h2w '.git') | Out-Null
Set-Origin $h2g 'git@github.com:synthetic-owner/h2-gitdir.git'
Compare-Context 'H2 toplevel/.git invalid empty dir, cwd = gitdir' $h2g 'None'
# H3: W/.git pointer file back to gd (separate-git-dir style)
$h3g = Join-Path $Base 'h3/gd'; $h3w = Join-Path $Base 'h3/w'
Init-Bare $h3g; Set-Worktree $h3g $h3w
New-Item -ItemType Directory -Force -Path $h3w | Out-Null
Set-Origin $h3g 'git@github.com:synthetic-owner/h3-gitdir.git'
[System.IO.File]::WriteAllText((Join-Path $h3w '.git'), "gitdir: $($h3g -replace '\\','/')\n".Replace('\n', "`n"))
Compare-Context 'H3 toplevel/.git pointer file back to gitdir, cwd = gitdir' $h3g 'git@github.com:synthetic-owner/h3-gitdir.git'

$fmt = '{0,-62} {1,-14} {2,-24} {3}'
$fmt -f 'context','status','new path','gate'
$script:rows | ForEach-Object { $fmt -f $_.Context, ($_.Status + ' ' + $_.Lit), $_.Path, $_.Gate }
"`nold -> new:"
$script:rows | ForEach-Object { "  {0}`n    old=[{1}]`n    new=[{2}]" -f $_.Context, $_.Old, $_.New }
$d = @($script:rows | Where-Object { $_.Status -like 'DIFF*' }); $l = @($script:rows | Where-Object { $_.Lit -eq 'lit=FAIL' })
if ($d.Count -eq 0 -and $l.Count -eq 0) { "`nVERDICT: all clean-location contexts EQUAL and literals ok" } else { "`nVERDICT: DIFF=$($d.Count) litFAIL=$($l.Count)" }
[System.IO.File]::WriteAllText('C:\AgentSessions\.trellis\tasks\10-07-repo-probe-overlap\research\check2-semantics-outside.json', ($script:rows | ConvertTo-Json -Depth 6))


