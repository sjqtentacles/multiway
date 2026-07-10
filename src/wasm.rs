//! The playground's engine entry point: a host-testable core
//! ([`run_json`], the length-prefix codec) plus a cfg-gated C ABI shim
//! for wasm32.
//!
//! **Byte-identity is structural.** [`run_json`] calls the exact
//! evolution + export path the CLI uses (`evolve_opts` at `threads: 1`,
//! optional `run_ordered`, `bundle_json` verbatim), so "the browser
//! produces the same bytes as the terminal" reduces to the engine's own
//! determinism — u32 vertices, u64 hashing, hash-free canonical
//! identity, no floats in exports, no map-iteration order anywhere near
//! output. The native test suite pins `run_json` against the committed
//! goldens on every CI cell; the wasm CI job re-checks across the real
//! architecture boundary with a node script.
//!
//! **Panic policy** (wasm build is `panic = "abort"`): every
//! input-shaped failure — parse errors, the nothing-to-do corner — is
//! caught before evolution and returned as `{"error":"..."}`. A trap is
//! therefore an engine bug; the page try/catches the RuntimeError,
//! reports it, and re-instantiates from cached bytes.
//!
//! **No threads parameter.** wasm32-unknown-unknown is single-threaded;
//! exposing a thread knob would be an ABI lie. Output is byte-identical
//! for every thread count anyway (the parallel suite pins it), so
//! nothing is lost.

use crate::causal::{run_ordered, UpdateOrder};
use crate::export::{bundle_json, esc};
use crate::rule::{parse_rule, parse_state};
use crate::system::{evolve_opts, EvolveOpts};

/// Evolve and export exactly as the CLI would: multiway `steps` layers
/// (threads 1, incremental), plus a `causal_events`-event single-path
/// run when nonzero, bundled by `export::bundle_json`. All failures
/// return `{"error":"..."}` JSON — this function never panics on any
/// input.
pub fn run_json(
    rule_s: &str,
    init_s: &str,
    steps: usize,
    causal_events: usize,
    order_standard: bool,
) -> String {
    let err = |msg: &str| format!("{{\"error\":\"{}\"}}", esc(msg));
    if steps == 0 && causal_events == 0 {
        return err("nothing to do: set steps and/or causal events");
    }
    let rule = match parse_rule(rule_s) {
        Ok(r) => r,
        Err(e) => return err(&format!("rule parse error: {}", e)),
    };
    let init = match parse_state(init_s) {
        Ok(s) => s,
        Err(e) => return err(&format!("state parse error: {}", e)),
    };
    let order = if order_standard {
        UpdateOrder::StandardGenerations
    } else {
        UpdateOrder::Sequential
    };

    let mw = evolve_opts(
        &rule,
        init.clone(),
        &EvolveOpts {
            steps,
            threads: 1,
            incremental: true,
        },
    );
    let causal_run = if causal_events > 0 {
        Some(run_ordered(&rule, init, causal_events, order))
    } else {
        None
    };
    bundle_json(&rule.text, init_s.trim(), &mw, causal_run.as_ref())
}

/// Encode a result string for the ABI: 4-byte u32 LE length prefix +
/// UTF-8 payload, in an exact-capacity boxed slice (the wasm side hands
/// the pointer to JS, which reads the prefix then the payload and calls
/// `mw_dealloc` with `4 + len`).
pub fn encode_result(s: &str) -> Box<[u8]> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out.into_boxed_slice()
}

/// Decode the codec (test-side inverse; the JS loader does the same
/// reads via DataView + TextDecoder).
pub fn decode_result(buf: &[u8]) -> String {
    let n = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    String::from_utf8(buf[4..4 + n].to_vec()).expect("payload is UTF-8 by construction")
}

// --- the wasm32 C ABI shim -------------------------------------------
//
// GREEN evidence for this block is honestly DEFERRED to the wasm CI
// job's node byte-identity check — the shim is unexecutable natively.
// It stays clippy-clean under `--target wasm32-unknown-unknown`.

/// Allocate `len` bytes for the host to write an input string into.
/// # Safety
/// The returned pointer owns exactly `len` bytes; the host must hand it
/// back via `mw_run` (which frees it) or `mw_dealloc` with the same
/// length.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn mw_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Free a buffer previously returned by `mw_alloc` or `mw_run`.
/// # Safety
/// `ptr` must come from those functions with the exact same `len`
/// (for `mw_run` results: `4 + payload length`).
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn mw_dealloc(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len));
}

/// Run the engine on UTF-8 rule/init strings written into `mw_alloc`ed
/// buffers; returns a length-prefixed JSON result (see
/// [`encode_result`]). Invalid UTF-8 is an error JSON, never a trap.
/// Consumes (frees) both input buffers.
/// # Safety
/// `rule_ptr`/`init_ptr` must be `mw_alloc(rule_len)`/`mw_alloc(init_len)`
/// buffers fully written by the host.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn mw_run(
    rule_ptr: *mut u8,
    rule_len: usize,
    init_ptr: *mut u8,
    init_len: usize,
    steps: usize,
    causal_events: usize,
    order_standard: usize,
) -> *mut u8 {
    let rule_buf = Vec::from_raw_parts(rule_ptr, rule_len, rule_len);
    let init_buf = Vec::from_raw_parts(init_ptr, init_len, init_len);
    let out = match (String::from_utf8(rule_buf), String::from_utf8(init_buf)) {
        (Ok(rule), Ok(init)) => run_json(&rule, &init, steps, causal_events, order_standard != 0),
        _ => format!("{{\"error\":\"{}\"}}", esc("inputs must be valid UTF-8")),
    };
    let boxed = encode_result(&out);
    let ptr = boxed.as_ptr() as *mut u8;
    std::mem::forget(boxed);
    ptr
}
