# Build Feedback Toolchain Subcommand Resolution

## Scenario: pinned Cargo subcommands

### 1. Scope / Trigger

Build-feedback jobs invoke a direct pinned `cargo.exe` but Cargo resolves
external subcommands such as `cargo-clippy.exe` through `PATH`. A pinned Cargo
executable alone is insufficient when a rustup proxy appears earlier on PATH.

### 2. Signatures

- `tool_environment(protocol: dict, config: dict, *, feedback: bool = False) -> dict[str, str]`
- `release_environment(protocol: dict, arm_record: dict) -> dict[str, str]`
- `_prepend_path_directories(environment: dict[str, str], directories: list[str]) -> None`

### 3. Contracts

The resulting `PATH` order is:

1. pinned Cargo executable directory;
2. pinned Rustc executable directory;
3. MSVC compiler/linker directories;
4. inherited environment paths.

Directories are deduplicated by normalized path while preserving first
occurrence. Existing CARGO/RUST/SCCACHE/MBX filtering, `CARGO_HOME`, direct
Rustc binding and MSVC environment receipts remain unchanged.

### 4. Validation & Error Matrix

- Rustup proxy precedes the pinned toolchain -> pinned Cargo/Rustc directories must still be first.
- Duplicate PATH entry -> retained once at its first position.
- Missing frozen `cargo-clippy.exe` -> real qualification fails; synthetic PATH tests cannot claim clippy success.
- Release environment -> uses the same helper and does not duplicate pinned directories.

### 5. Good/Base/Bad Cases

- Good: `cargo clippy --version` under the frozen environment reports Clippy 0.1.97.
- Base: native `cargo check` and `cargo test` resolve the same direct Rustc binding.
- Bad: direct `cargo.exe` plus inherited PATH resolves `cargo-clippy.exe` from the default stable rustup proxy.

### 6. Tests Required

- Exercise the production `tool_environment()` with competing pinned and proxy `cargo-clippy` markers.
- Assert the first PATH entries are pinned Cargo, pinned Rustc and MSVC directories.
- Assert normalized duplicates occur once and the pinned marker wins resolution.
- Run Ruff, Mypy and scoped whitespace checks.

### 7. Wrong vs Correct

#### Wrong

```python
environment["PATH"] = os.pathsep.join([msvc_bin, environment["PATH"]])
```

This leaves rustup's proxy ahead of the pinned Cargo subcommand directory.

#### Correct

```python
_prepend_path_directories(environment, [
    str(Path(protocol["toolchain"]["cargo"]["path"]).parent),
    str(Path(protocol["toolchain"]["rustc"]["path"]).parent),
    str(Path(compiler).parent),
    str(Path(linker).parent),
])
```
