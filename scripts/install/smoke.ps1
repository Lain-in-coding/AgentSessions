#!/usr/bin/env pwsh
# End-to-end surface smoke against an ALREADY-BUILT binary: robot envelopes,
# documented exit codes, and the MCP stdio handshake. This script never builds —
# building is the job of install.ps1 or CI, and a smoke run that builds cannot
# tell a broken artifact from a broken build.
#
# The fixture is synthetic and inlined below. No real transcript is ever read.

[CmdletBinding()]
param(
    [string]$Binary
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Assertions here own the interpretation of native exit codes; PowerShell must
# not abort on a non-zero exit before the assertion can report actual output.
$PSNativeCommandUseErrorActionPreference = $false

$script:Failures = 0
$script:TempDir = $null

function Write-Pass {
    param([string]$Message)
    Write-Host "smoke: pass  $Message"
}

function Write-Fail {
    param([string]$Message, [string]$Actual)
    $script:Failures++
    Write-Host "smoke: FAIL  $Message" -ForegroundColor Red
    if ($Actual) {
        Write-Host "smoke:       actual: $Actual" -ForegroundColor Red
    }
}

function Assert-That {
    param([bool]$Condition, [string]$Message, [string]$Actual)
    if ($Condition) { Write-Pass $Message } else { Write-Fail $Message $Actual }
}

function Exit-Smoke {
    param([int]$Code)
    # The temp store is disposable and must go even on the failure path, but only
    # after diagnostics have been printed.
    if ($script:TempDir -and (Test-Path -LiteralPath $script:TempDir)) {
        Remove-Item -LiteralPath $script:TempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    exit $Code
}

function Abort {
    param([string]$Message)
    Write-Host "smoke: error: $Message" -ForegroundColor Red
    Exit-Smoke 1
}

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if ([string]::IsNullOrWhiteSpace($Binary)) {
    $exeSuffix = if ($IsWindows) { '.exe' } else { '' }
    $Binary = Join-Path $repoRoot (Join-Path 'target/release' "agentsessions$exeSuffix")
}
if (-not (Test-Path -LiteralPath $Binary)) {
    Abort "binary not found: $Binary (build it first: cargo build --locked --release -p agentsessions-cli)"
}
$Binary = (Resolve-Path -LiteralPath $Binary).Path

# Robot invocation returning both the parsed first stdout frame and the exit
# code: every assertion below is about that pair.
function Invoke-Robot {
    param([string[]]$Arguments)
    $lines = & $Binary @Arguments
    $code = $LASTEXITCODE
    $text = ($lines | Out-String).Trim()
    $frame = $null
    if ($text) {
        $first = ($text -split "`r?`n")[0]
        try { $frame = $first | ConvertFrom-Json } catch { $frame = $null }
    }
    return @{ Code = $code; Frame = $frame; Text = $text }
}

function Test-HasProperty {
    param($Object, [string]$Name)
    if ($null -eq $Object) { return $false }
    return [bool]($Object.PSObject.Properties.Name -contains $Name)
}

$script:TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("agentsessions-smoke-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $script:TempDir -Force | Out-Null
$db = Join-Path $script:TempDir 'smoke.db'
$fixture = Join-Path $script:TempDir 'fixture.jsonl'

# Synthetic Claude Code transcript: root -> reply -> sidechain probe. The
# retrieval term is fixed so the search assertion does not depend on scoring.
$fixtureLines = @(
    '{"type":"user","uuid":"smoke-root-1","parentUuid":null,"sessionId":"smoke-session-1","timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"smokezylograph root question"}}'
    '{"type":"assistant","uuid":"smoke-reply-2","parentUuid":"smoke-root-1","sessionId":"smoke-session-1","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"smokezylograph mainline answer"}]}}'
    '{"type":"assistant","uuid":"smoke-probe-3","parentUuid":"smoke-reply-2","sessionId":"smoke-session-1","isSidechain":true,"timestamp":"2026-01-01T00:00:02.000Z","message":{"role":"assistant","content":"smokezylograph sidechain probe"}}'
)
Set-Content -LiteralPath $fixture -Value ($fixtureLines -join "`n") -Encoding utf8NoBOM -NoNewline

Write-Host "smoke: binary  $Binary"
Write-Host "smoke: workdir $script:TempDir"

# 1. doctor against a fresh store: reports the store as openable with a numeric schema.
$r = Invoke-Robot @('--db', $db, '--robot', 'doctor')
Assert-That ($r.Code -eq 0) 'doctor exits 0' "exit=$($r.Code) stdout=$($r.Text)"
Assert-That (($null -ne $r.Frame) -and ($r.Frame.data.db -eq 'ok')) 'doctor reports data.db == ok' $r.Text
$schema = if ($null -ne $r.Frame) { $r.Frame.data.schema } else { $null }
Assert-That (($schema -is [int]) -or ($schema -is [int64]) -or ($schema -is [double])) 'doctor reports a numeric data.schema' $r.Text

# 2. sync the fixture: all three conversational records are ingested in one batch.
$r = Invoke-Robot @('--db', $db, '--robot', 'sync', $fixture)
Assert-That ($r.Code -eq 0) 'sync exits 0' "exit=$($r.Code) stdout=$($r.Text)"
Assert-That ($null -ne $r.Frame -and $r.Frame.ok -eq $true) 'sync envelope reports ok:true' $r.Text
Assert-That ($null -ne $r.Frame -and $r.Frame.data.messages -eq 3) 'sync reports data.messages == 3' $r.Text

# 3. search for the fixture term: hits are message-level entities.
$r = Invoke-Robot @('--db', $db, '--robot', 'search', 'smokezylograph')
$hitId = $null
Assert-That ($r.Code -eq 0) 'search exits 0' "exit=$($r.Code) stdout=$($r.Text)"
if ($null -ne $r.Frame) {
    $hits = @($r.Frame.data.hits)
    Assert-That ($hits.Count -gt 0) 'search returns a non-empty data.hits' $r.Text
    if ($hits.Count -gt 0) {
        $hitId = $hits[0].id
        Assert-That ($hitId -like 'msg_v1_*') 'first search hit id starts with msg_v1_' "id=$hitId"
    }
} else {
    Write-Fail 'search returns a non-empty data.hits' $r.Text
}

# 4. get the first hit: the stored canonical payload comes back verbatim. The
# session wire id is read out of that payload rather than hardcoded.
$sessionId = $null
if ($hitId) {
    $r = Invoke-Robot @('--db', $db, '--robot', 'get', $hitId)
    Assert-That ($r.Code -eq 0) 'get <hit-id> exits 0' "exit=$($r.Code) stdout=$($r.Text)"
    $payload = if ($null -ne $r.Frame) { $r.Frame.data.payload } else { $null }
    Assert-That (-not [string]::IsNullOrWhiteSpace($payload)) 'get <hit-id> returns a non-empty data.payload' $r.Text
    if (-not [string]::IsNullOrWhiteSpace($payload)) {
        $sessionId = ($payload | ConvertFrom-Json).session
    }
} else {
    Write-Fail 'get <hit-id> exits 0' 'no search hit id to resolve'
}

# 5. context for that session: mainline messages plus their evidence spans.
if ($sessionId) {
    $r = Invoke-Robot @('--db', $db, '--robot', 'context', $sessionId)
    Assert-That ($r.Code -eq 0) 'context <ses-id> exits 0' "exit=$($r.Code) stdout=$($r.Text)"
    if ($null -ne $r.Frame) {
        Assert-That (@($r.Frame.data.messages).Count -gt 0) 'context returns non-empty data.messages' $r.Text
        Assert-That (@($r.Frame.data.evidence).Count -gt 0) 'context returns non-empty data.evidence' $r.Text
    } else {
        Write-Fail 'context returns non-empty data.messages' $r.Text
    }
} else {
    Write-Fail 'context <ses-id> exits 0' 'no session id extracted from the hit payload'
}

# 6. status: 3 messages + 1 session + 1 document entity.
$r = Invoke-Robot @('--db', $db, '--robot', 'status')
Assert-That ($r.Code -eq 0) 'status exits 0' "exit=$($r.Code) stdout=$($r.Text)"
$count = if ($null -ne $r.Frame) { [int]$r.Frame.data.catalog_count } else { -1 }
Assert-That ($count -ge 5) 'status reports data.catalog_count >= 5' "catalog_count=$count"

# 7. get on a well-formed but absent id: documented as exit 0 with a null
# payload (absence is data, not an error). See the deviation note in the task
# report: design §2.5 expected exit 4 here, the product returns 0.
$r = Invoke-Robot @('--db', $db, '--robot', 'get', 'ses_v1_nope')
Assert-That ($r.Code -eq 0) 'get <absent id> exits 0' "exit=$($r.Code) stdout=$($r.Text)"
Assert-That ($null -ne $r.Frame -and $null -eq $r.Frame.data.payload) 'get <absent id> returns data.payload == null' $r.Text

# 8. context on an absent session: this is the surface that carries not_found.
$r = Invoke-Robot @('--db', $db, '--robot', 'context', 'ses_v1_nope')
Assert-That ($r.Code -eq 4) 'context <absent session> exits 4' "exit=$($r.Code) stdout=$($r.Text)"
Assert-That ($null -ne $r.Frame -and $r.Frame.error.code -eq 'not_found') 'context <absent session> reports error.code == not_found' $r.Text

# 9. garbage cursor: rejected as a request error, not silently reset to page 1.
$r = Invoke-Robot @('--db', $db, '--robot', 'search', 'smokezylograph', '--cursor', 'garbage')
Assert-That ($r.Code -eq 2) 'search with a garbage cursor exits 2' "exit=$($r.Code) stdout=$($r.Text)"
Assert-That ($null -ne $r.Frame -and $r.Frame.error.code -eq 'cursor_invalid') 'garbage cursor reports error.code == cursor_invalid' $r.Text

# 10. MCP stdio handshake. stdout is the protocol channel, so every line must be
# a complete JSON-RPC frame; anything else means diagnostics leaked into it.
$mcpInput = @(
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}'
    '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
    '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_status","arguments":{}}}'
)
$mcpOut = $mcpInput | & $Binary --db $db mcp
$mcpCode = $LASTEXITCODE
$mcpText = ($mcpOut | Out-String).Trim()
Assert-That ($mcpCode -eq 0) 'mcp server exits 0 on stdin EOF' "exit=$mcpCode stdout=$mcpText"

$frames = @()
$pureJson = $true
foreach ($line in ($mcpText -split "`r?`n")) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    try { $frames += ($line | ConvertFrom-Json) } catch { $pureJson = $false }
}
Assert-That $pureJson 'every mcp stdout line is valid JSON' $mcpText

$listFrame = $frames | Where-Object { (Test-HasProperty $_ 'id') -and $_.id -eq 2 } | Select-Object -First 1
$toolCount = if ($null -ne $listFrame) { @($listFrame.result.tools).Count } else { -1 }
Assert-That ($toolCount -eq 6) 'tools/list returns exactly 6 tools' "tools=$toolCount"

$callFrame = $frames | Where-Object { (Test-HasProperty $_ 'id') -and $_.id -eq 3 } | Select-Object -First 1
Assert-That ($null -ne $callFrame -and $callFrame.result.isError -eq $false) 'tools/call get_status returns isError:false' $mcpText

if ($script:Failures -gt 0) {
    Write-Host "smoke: $script:Failures assertion(s) failed" -ForegroundColor Red
    Exit-Smoke 1
}
Write-Host 'smoke: all assertions passed'
Exit-Smoke 0
