# Historical CI evidence retention (2026-09-30)

- Run: `core-beta-evidence` 36665818855; source commit `bbb7c79533c13db584fa5ca2e1e05990d990c8da`.
- GitHub API: four artifacts unexpired when downloaded, scheduled expiry 2026-10-07. All four named jobs were successful.
- All four artifacts were downloaded with `gh run download` into ignored `target/historical-evidence/36665818855`; 14 files each, 56 total. Downloaded binaries were NOT executed.
- Each binary SHA-256 and byte length was independently recomputed and matches its artifact's environment report; all environment reports name the source commit above.
- Raw runner paths/logs/config reports and binaries remain ignored local output, not tracked task artifacts. This receipt contains portable metadata only.
- This is preservation of historical unsigned hosted-runner evidence, NOT validation of the reliability changes, an attestation, provider certification, a clean-machine/minimum-OS claim, or a completed release gate.

| Artifact | Target | Binary bytes | SHA-256 |
|---|---|---:|---|
| `core-beta-linux-gnu-x64-36665818855` | `x86_64-unknown-linux-gnu` | 7981752 | `309fcd9d290ddb0a1f639018d464722323f145d38154dcd29025adcddff844b9` |
| `core-beta-macos-arm64-36665818855` | `aarch64-apple-darwin` | 6796272 | `c4485dbdd5ed8a70b5c200310a4d38b2358d1132d25f158c01badd2920330489` |
| `core-beta-macos-intel-36665818855` | `x86_64-apple-darwin` | 7515544 | `9284bb78b51903c192e60f80f2b598ac25673004c03f9795355045e65a6c1c08` |
| `core-beta-windows-x64-36665818855` | `x86_64-pc-windows-msvc` | 7264256 | `43636d709a313fedcd4c11bad83cbe7c6cd98b681cabfbe51bada6d1c1f667f7` |
