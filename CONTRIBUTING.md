# Contributing

## Build & test

```sh
cargo test                                   # full suite (fast, debug)
cargo test --release -- --ignored            # depth-5 baseline + perf smoke
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --doc
```

MSRV is **1.63** (`rust-version` in Cargo.toml, proven by CI). Check
locally with `cargo +1.63 test --all-targets`.

## The two hard policies (PR-rejection criteria)

1. **Zero dependencies — including dev-dependencies.** No proptest, no
   criterion, no rand. Hand-roll it or don't ship it. The test PRNG,
   property harness, oracles, JSON checker, and bench harness are all
   in-tree; extend those.
   *Policy interpretation:* "zero dependencies" means **crates and
   third-party CI actions**. Tools preinstalled on the GitHub runners
   (rustup, node) are in-bounds — the wasm job's ~30-line inline
   `node -e` byte-identity check is the same policy class as building
   with the preinstalled rustup. GitHub-owned actions pinned by SHA
   (checkout, upload-artifact) are likewise in-bounds; anything from a
   third-party org is not.
2. **Determinism.** Identical inputs must produce byte-identical `--json`
   output. No wall clock in outputs, no global RNG, and no `HashMap`
   iteration order reaching output — any map whose iteration order can
   reach output must be a `det::DetMap` iterated in sorted-key order, or
   a `Vec`. The golden tests and `prop_evolve_deterministic` enforce
   this; the same committed golden bytes passing on Linux/macOS/Windows
   in CI is the cross-platform proof.

## TDD conventions

Tests land red-first; the commit message body records the evidence:

```
RED: <test names> — E0425 (fn does not exist), then failing on
     seed=0x... case=41
GREEN: <what was implemented>; full suite passes
```

For characterization tests of already-correct behavior, honest red is a
*named temporary mutation* (never committed):

```
MUTATION-CHECKED: removed incid sort in wl_hash -> 97/100 cases fail
```

Property tests: fn names start with `prop_` (the CI `props` job filters
on this), driven by `tests/common/harness.rs::prop` with a per-file seed
constant. Env knobs: `MULTIWAY_PROP_CASES` (absolute count, default
100), `MULTIWAY_PROP_CASE`, `MULTIWAY_PROP_SEED`. Every failure prints a
one-line repro.

Golden files: `MULTIWAY_BLESS=1 cargo test --test golden --
--test-threads=1` regenerates and then FAILS (a blessing run can never
pass CI); rerun without the env var to verify, and review golden diffs
like code.

## Misc policies

- **Lockfile**: `Cargo.lock` must stay at a version the MSRV cargo can
  read (currently v3; regenerate with `cargo +1.63 generate-lockfile`
  after `rm Cargo.lock` if it ever upgrades).
- **Clippy churn**: new stable lints are fixed, not `#[allow]`ed.
- **rustfmt**: default configuration, no rustfmt.toml.
- **Viewer**: `viewer/template.html` stays a single self-contained file —
  no external URLs, no `Math.random`/`Date.now` (deterministic layouts);
  `tests/viewer.rs` pins this.
- **Screenshots**: regenerate `demo.html` via the README quick-start,
  capture light + dark at ~1640px wide, save under `docs/` (the
  `.gitignore` has a `!docs/*.png` exception), keep each under ~400KB.

## Benchmarks

`cargo run --release --bin bench` — warmup + median-of-7 on fixed
inputs; emits `BENCH` lines and a markdown table. Timings never go into
goldens.
