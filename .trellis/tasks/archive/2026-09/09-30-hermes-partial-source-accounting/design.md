# Design: trustworthy partial accounting

Use a bounded exact source-row census under the existing max_messages limit, and reconcile it with actual emitted rows and classified skips. Do not use MAX_ORPHAN_CENSUS's cap+1 result as an exact total. Include rows excluded by session validation, not only rows reached by the session walk. Preserve honest per-session diagnostics and avoid double-counting a bad row.

The existing ParseReport.skipped -> SourceBatch.relation_complete path is the interface; no new protocol flag is needed. Valid messages continue through staging, skipped > 0 revokes completeness, and storage retains prior claims. Resource-cap/structural accounting failures remain explicit errors, not partial truncation. Ensure duplicate or otherwise inconsistent schema input cannot make subtraction underflow or falsely report completeness.

Do not change the whole-source byte ceiling, adopt rowids as native IDs, widen discovery, or alter JSON parsing. Source changes belong only in the Hermes crate; cross-layer warnings and integration tests belong to B.
