//! Playground invariants, mirroring tests/viewer.rs: the second
//! placeholder's contract, the panel's DOM surface, the hand-rolled
//! base64 encoder's RFC vectors (through the real binary), and the
//! end-to-end `--playground` bake with a fake-bytes wasm fixture.

use std::process::Command;

fn template() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/viewer/template.html");
    std::fs::read_to_string(path).unwrap()
}

/// The wasm placeholder appears exactly once (the bake replaces it with
/// base64 or the empty string — never leaves it).
#[test]
fn template_has_wasm_placeholder_exactly_once() {
    let tpl = template();
    let n = tpl.matches("__WASM_B64__").count();
    assert_eq!(n, 1, "__WASM_B64__ must appear exactly once, found {}", n);
}

/// The playground panel's promised DOM surface: inputs, the Run button,
/// an accessible error line, the copy-bundle affordance, and the
/// loader's no-network instantiation path.
#[test]
fn template_has_playground_panel() {
    let tpl = template();
    for needle in [
        "id=\"playPanel\"",
        "id=\"pgRule\"",
        "id=\"pgInit\"",
        "id=\"pgSteps\"",
        "id=\"pgCausal\"",
        "id=\"pgOrder\"",
        "id=\"pgRun\"",
        "id=\"pgErr\"",
        "role=\"alert\"",
        "id=\"pgCopy\"",
        "WebAssembly.instantiate(",
        "max=\"8\"",
    ] {
        assert!(tpl.contains(needle), "template missing {:?}", needle);
    }
    // the loader must NOT stream-fetch (fetch( is a banned needle and
    // the page must work from file://); atob path only
    assert!(tpl.contains("atob("), "loader must decode inline base64");
}

fn bake(dir: &std::path::Path, wasm_bytes: &[u8]) -> String {
    let wasm_path = dir.join("fixture.wasm");
    let out_path = dir.join("playground.html");
    std::fs::write(&wasm_path, wasm_bytes).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_multiway"))
        .args([
            "--playground",
            out_path.to_str().unwrap(),
            "--wasm",
            wasm_path.to_str().unwrap(),
            "--quiet",
        ])
        .output()
        .expect("failed to run multiway");
    assert!(out.status.success(), "bake failed: {:?}", out);
    std::fs::read_to_string(&out_path).unwrap()
}

/// End-to-end `--playground` bake: DATA is null (no baked bundle), the
/// wasm placeholder is consumed, and the fixture bytes appear as
/// base64.
#[test]
fn playground_bake_end_to_end() {
    let dir = std::env::temp_dir().join(format!("multiway-pg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html = bake(&dir, b"\0asm\x01\0\0\0 fake module for the bake test");
    assert!(!html.contains("__WASM_B64__"), "placeholder left baked");
    assert!(!html.contains("__DATA_JSON__"), "data placeholder left");
    assert!(
        html.contains("const BOOT_DATA = null;"),
        "playground must boot with DATA null"
    );
    assert!(
        html.contains("AGFzbQEAAAAgZmFrZSBtb2R1bGUgZm9yIHRoZSBiYWtlIHRlc3Q="),
        "fixture bytes must appear as standard base64"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// The hand-rolled encoder against the RFC 4648 test vectors, through
/// the real binary (each vector baked and located in the page).
#[test]
fn base64_rfc_vectors() {
    let dir = std::env::temp_dir().join(format!("multiway-b64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let vectors: [(&[u8], &str); 6] = [
        (b"f", "Zg=="),
        (b"fo", "Zm8="),
        (b"foo", "Zm9v"),
        (b"foob", "Zm9vYg=="),
        (b"fooba", "Zm9vYmE="),
        (b"foobar", "Zm9vYmFy"),
    ];
    for (bytes, expect) in vectors {
        let html = bake(&dir, bytes);
        let needle = format!("const WASM_B64 = \"{}\";", expect);
        assert!(
            html.contains(&needle),
            "vector {:?} must bake as {:?}",
            bytes,
            expect
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// `--html` mode bakes the wasm slot EMPTY: same template, panel
/// hidden, zero divergence between the two modes' code paths.
#[test]
fn html_mode_bakes_empty_wasm_slot() {
    let dir = std::env::temp_dir().join(format!("multiway-pg-html-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out_path = dir.join("demo.html");
    let out = Command::new(env!("CARGO_BIN_EXE_multiway"))
        .args([
            "--rule",
            "{{x,y}}->{{x,y},{y,z}}",
            "--init",
            "{{0,0}}",
            "--steps",
            "2",
            "--html",
            out_path.to_str().unwrap(),
            "--quiet",
        ])
        .output()
        .expect("failed to run multiway");
    assert!(out.status.success(), "html bake failed: {:?}", out);
    let html = std::fs::read_to_string(&out_path).unwrap();
    assert!(!html.contains("__WASM_B64__"), "placeholder left in --html");
    assert!(
        html.contains("const WASM_B64 = \"\";"),
        "--html must bake an empty wasm slot"
    );
    std::fs::remove_dir_all(&dir).ok();
}
