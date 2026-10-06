# merge-coverage.ps1 - merges verified per-project coverage receipts into coverage-index.json
# Audit trail: records exact source receipts, hash verification, and per-project state transitions.
$ErrorActionPreference = 'Stop'
$research = "C:\AgentSessions\.trellis\tasks\10-05-competitive-source-audit-plan\research"
$repoRoot = "C:\AgentSessions\Github_src"

$idxPath = Join-Path $research 'coverage-index.json'
$idx = Get-Content -LiteralPath $idxPath -Raw | ConvertFrom-Json

function Get-ProjectEntry([string]$name) {
  return $idx.projects | Where-Object { $_.project -eq $name }
}

function Verify-Hash([string]$project, [string]$relPath, [string[]]$expected) {
  $full = Join-Path (Join-Path $repoRoot $project) $relPath
  if (-not (Test-Path -LiteralPath $full)) { return @{ ok = $false; actual = $null; reason = 'missing_on_disk' } }
  $actual = (Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash.ToLower()
  foreach ($e in $expected) { if ($e -and $e.ToLower() -eq $actual) { return @{ ok = $true; actual = $actual; reason = 'matched' } } }
  return @{ ok = $false; actual = $actual; reason = 'none_of_expected_matched' }
}

# ---------- agf ----------
$agfReceipt = Get-Content -LiteralPath (Join-Path $research 'agf-file-coverage.json') -Raw | ConvertFrom-Json
$tuiReceipt = Get-Content -LiteralPath (Join-Path $research 'coverage-agf-tui.json') -Raw | ConvertFrom-Json
$proj = Get-ProjectEntry 'agf'
$agfFull = 0; $agfPartial = 0; $agfExcluded = 0; $agfHashFail = @()
foreach ($pf in $proj.files) {
  $rf = $agfReceipt.files | Where-Object { $_.path -eq $pf.path }
  if (-not $rf) { continue }
  $vf = Verify-Hash 'agf' $pf.path @($rf.sha256)
  if (-not $vf.ok) { $agfHashFail += $pf.path }
  $ranges = @()
  $status = $rf.status
  $receipts = @('research/agf-file-coverage.json')
  if ($pf.path -eq $tuiReceipt.repository_relative_path -and $tuiReceipt.status -eq 'full') {
    $status = 'full'
    foreach ($iv in $tuiReceipt.read_intervals) { $ranges += , @([int]$iv.start_line, [int]$iv.end_line) }
    $receipts += 'research/coverage-agf-tui.json'
    $vf = Verify-Hash 'agf' $pf.path @($tuiReceipt.current_file_sha256, $rf.sha256)
    if (-not $vf.ok) { $agfHashFail += $pf.path }
  } else {
    if ($rf.reviewed_ranges) { foreach ($r in $rf.reviewed_ranges) { $ranges += , @([int]$r.start, [int]$r.end) } }
  }
  switch ($status) {
    'full' { $pf.state = 'read_full'; $agfFull++ }
    'partial' { $pf.state = 'read_partial'; $agfPartial++ }
    'excluded' { $pf.state = 'excluded'; $agfExcluded++ }
    default { $pf.state = 'unread' }
  }
  $pf.read_ranges = $ranges
  $pf | Add-Member -NotePropertyName receipt -NotePropertyValue ($receipts -join '; ') -Force
  $pf | Add-Member -NotePropertyName receipt_status -NotePropertyValue $status -Force
  $pf | Add-Member -NotePropertyName hash_verified -NotePropertyValue $vf.ok -Force
  $pf | Add-Member -NotePropertyName verified_at -NotePropertyValue '2026-10-06' -Force
  if ($rf.exclusion_reason) { $pf | Add-Member -NotePropertyName exclusion_reason -NotePropertyValue $rf.exclusion_reason -Force }
}
$proj | Add-Member -NotePropertyName coverage_merge -NotePropertyValue ([pscustomobject]@{
  date = '2026-10-06'
  receipts = @('research/agf-file-coverage.json', 'research/coverage-agf-tui.json')
  full = $agfFull; partial = $agfPartial; excluded = $agfExcluded
  unread = $proj.files.Count - $agfFull - $agfPartial - $agfExcluded
  hashes_verified = ($agfHashFail.Count -eq 0)
  hash_failures = $agfHashFail
  notes = 'TUI single-file receipt (src/tui/mod.rs, 2686/2686 lines, 23 intervals) supersedes the parent partial ledger at 2182-2686. demo.gif excluded (binary).'
}) -Force

# ---------- sessiongrep ----------
$sgReceipt = Get-Content -LiteralPath (Join-Path $research 'coverage-sessiongrep.json') -Raw | ConvertFrom-Json
$proj = Get-ProjectEntry 'sessiongrep'
$sgFull = 0; $sgPartial = 0; $sgExcluded = 0; $sgHashFail = @()
foreach ($pf in $proj.files) {
  $rf = $sgReceipt.files | Where-Object { $_.path -eq $pf.path }
  if (-not $rf) { continue }
  $vf = Verify-Hash 'sessiongrep' $pf.path @($rf.sha256_final, $rf.sha256_initial)
  if (-not $vf.ok) { $sgHashFail += $pf.path }
  $ranges = @()
  if ($rf.read_ranges) { foreach ($r in $rf.read_ranges) { $ranges += , @([int]$r[0], [int]$r[1]) } }
  switch ($rf.status) {
    'full' { $pf.state = 'read_full'; $sgFull++ }
    'partial' { $pf.state = 'read_partial'; $sgPartial++ }
    'excluded' { $pf.state = 'excluded'; $sgExcluded++ }
    default { $pf.state = 'unread' }
  }
  $pf.read_ranges = $ranges
  $pf | Add-Member -NotePropertyName receipt -NotePropertyValue 'research/coverage-sessiongrep.json' -Force
  $pf | Add-Member -NotePropertyName receipt_status -NotePropertyValue $rf.status -Force
  $pf | Add-Member -NotePropertyName hash_verified -NotePropertyValue $vf.ok -Force
  $pf | Add-Member -NotePropertyName verified_at -NotePropertyValue '2026-10-06' -Force
  if ($rf.exclusion_reason) { $pf | Add-Member -NotePropertyName exclusion_reason -NotePropertyValue $rf.exclusion_reason -Force }
}
$proj | Add-Member -NotePropertyName coverage_merge -NotePropertyValue ([pscustomobject]@{
  date = '2026-10-06'
  receipts = @('research/coverage-sessiongrep.json')
  full = $sgFull; partial = $sgPartial; excluded = $sgExcluded
  unread = $proj.files.Count - $sgFull - $sgPartial - $sgExcluded
  hashes_verified = ($sgHashFail.Count -eq 0)
  hash_failures = $sgHashFail
  notes = 'Cargo.lock completed to full in the 20:52 JSON receipt (11 contiguous ranges, 1-1660). The audit MD caveat section still reflects the 20:47 partial snapshot; MD superseded by JSON receipt for Cargo.lock only. Worker b956 errored (provider 402) after JSON update.'
}) -Force

# ---------- top-level log ----------
$logEntry = [pscustomobject]@{
  date = '2026-10-06'
  merged_by = 'main-session'
  projects = @('agf', 'sessiongrep')
  method = 'Hash re-verification against GitHub_src working tree + receipt range normalization (agf: reviewed_ranges/read_intervals; sessiongrep: read_ranges)'
}
if ($idx.PSObject.Properties.Name -contains 'merge_log') { $idx.merge_log = @($idx.merge_log) + $logEntry } else { $idx | Add-Member -NotePropertyName merge_log -NotePropertyValue @($logEntry) -Force }
$idx.updated = '2026-10-06T21:30:00+08:00'

$idx | ConvertTo-Json -Depth 14 -Compress:$false | Set-Content -LiteralPath $idxPath -Encoding UTF8
Write-Output 'merge done'
