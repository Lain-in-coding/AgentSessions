# Design: one lexicographic time key

Replace pair-dependent UTC-vs-raw fallback with a single per-message class/value key: Missing < Invalid(raw bytes) < Valid(parsed instant). The comparator then applies the unchanged document/ordinal/placement tie-breaks. Keep parsing once per message and preserve the raw timestamp in output/storage. Use private types/helpers only.

The concrete counterexample is `2026-01-01T00:00:00+10:00`, `2025-12-31T20:00:00Z`, and `2026`; the first two compare chronologically opposite to their raw strings, while the invalid value closes a comparison cycle. Test all placement permutations, not just one initial vector.

Official Rust Ord/slice documentation requires a consistent transitive order; Context7 resolved `/rust-lang/rust` and its slice/RELEASES documentation was checked on 2026-09-30. This task asserts deterministic results, not an unobserved production panic.
