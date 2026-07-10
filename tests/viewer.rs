//! Viewer template invariants (via `CARGO_MANIFEST_DIR`) and end-to-end
//! HTML baking through the real binary (via `CARGO_BIN_EXE_`, zero
//! dev-deps). Rendering behavior itself is browser territory; these tests
//! pin what Rust can see: self-containment, the placeholder contract, and
//! the DOM/JS surface the CLI docs promise.

use std::process::Command;

fn template() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/viewer/template.html");
    std::fs::read_to_string(path).unwrap()
}

/// One placeholder, zero network: the baked page must work from file://
/// with no external requests and no nondeterminism in layout code.
/// Checked over the WHOLE file, so the template cannot smuggle these
/// substrings in anywhere — not even in comments.
#[test]
fn template_is_self_contained() {
    let tpl = template();
    let n = tpl.matches("__DATA_JSON__").count();
    assert_eq!(n, 1, "placeholder must appear exactly once, found {}", n);
    // needles assembled from halves so THIS source file never contains them
    let banned: &[(&str, &str)] = &[
        ("http", "://"),
        ("https", "://"),
        ("src=\"", "//"),
        ("@", "import"),
        ("fetch", "("),
        ("XML", "HttpRequest"),
        ("Math", ".random"),
        ("Date", ".now"),
    ];
    for (a, b) in banned {
        let needle = format!("{}{}", a, b);
        assert!(
            !tpl.contains(&needle),
            "template must not contain {:?} (anywhere, comments included)",
            needle
        );
    }
}

/// The DOM/JS surface the viewer upgrade promises: three tabs, ARIA
/// tablist semantics, touch support, rAF-batched drawing, PNG export,
/// and the manual theme override hook.
#[test]
fn template_has_required_elements() {
    let tpl = template();
    let required = [
        "id=\"cv\"",
        "id=\"inspector\"",
        "id=\"tabMw\"",
        "id=\"tabCa\"",
        "id=\"tabTe\"",
        "role=\"tablist\"",
        "aria-label",
        "touch-action",
        "id=\"tip\"",
        "requestAnimationFrame",
        "pointerdown",
        "toBlob",
        "data-theme",
    ];
    for needle in required {
        assert!(tpl.contains(needle), "template missing {:?}", needle);
    }
}

/// End to end: the binary bakes a complete page — no surviving
/// placeholder, teg section present, and the data actually inlined.
#[test]
fn baked_html_is_complete() {
    let tmp = std::env::temp_dir().join(format!("multiway-viewer-{}.html", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_multiway"))
        .args([
            "--rule",
            "{{x,y}}->{{x,y},{y,z}}",
            "--init",
            "{{0,0}}",
            "--steps",
            "2",
            "--quiet",
            "--html",
            tmp.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let html = std::fs::read_to_string(&tmp).unwrap();
    std::fs::remove_file(&tmp).ok();
    assert_eq!(
        html.matches("__DATA_JSON__").count(),
        0,
        "placeholder survived baking"
    );
    assert!(html.contains("\"teg\":"), "teg section missing from bundle");
    assert!(
        html.contains("const BOOT_DATA = {"),
        "bundle not inlined into the boot binding"
    );
}
