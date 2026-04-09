# Contributing to mcp-fetch-patents

## Dev Setup

### Rust

```bash
just install-rs          # install Rust toolchain via rustup
```

All `cargo` commands require `CC=gcc` (the default `cc` wrapper breaks `aws-lc-sys`):

```bash
CC=gcc cargo build --manifest-path src/rust/Cargo.toml
```

Use the `just` recipes instead — they set `CC=gcc` automatically:

```bash
just build-rust          # debug build
just test-rust           # run 237+ Rust tests
just lint-rust           # clippy with -D warnings
just check-rust          # quick type-check
```

### Python

```bash
pip install -e ".[dev]"  # editable install with test deps
just test                # run Python fast tests
```

## Running the Full Suite

```bash
just ci                  # Python fast + Rust tests
```

## Code Style

- **No comments** unless explicitly requested.
- **Error handling**: `anyhow::Result` everywhere in Rust.
- **Regex**: use `OnceLock<Regex>` statics, never `Regex::new(...).unwrap()` inline.
- **Tests**: same file as the code (`#[cfg(test)] mod tests`).
- **Async**: tokio runtime, reqwest for HTTP, chromiumoxide for browser automation.
- **Dynamic metadata**: `serde_json::Value`.
- Follow existing patterns in the file you're editing.

## PR Expectations

- `just ci` passes cleanly.
- `just lint-rust` passes (clippy, no warnings).
- New Rust features include tests in the same module.
- See `AGENTS.md` for full conventions and module layout.
