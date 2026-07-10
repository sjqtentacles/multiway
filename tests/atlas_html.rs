//! Atlas HTML invariants: the template's placeholder contract and
//! self-containment (same banned-substring-halves trick as viewer.rs),
//! plus an end-to-end `--scan --atlas DIR` bake through the real binary
//! and the `--count` / cap-refusal CLI pins.

use std::process::Command;

fn template() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/viewer/atlas.html");
    std::fs::read_to_string(path).unwrap()
}

/// One placeholder, zero network: the baked atlas must work from
/// file:// with no external requests and no nondeterminism.
#[test]
fn atlas_template_is_self_contained() {
    let tpl = template();
    let n = tpl.matches("__ATLAS_JSON__").count();
    assert_eq!(n, 1, "placeholder must appear exactly once, found {}", n);
    // needles assembled from halves so THIS file never contains them
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
            "atlas template must not contain {:?}",
            needle
        );
    }
}

fn scan_args(dir: &std::path::Path) -> Vec<String> {
    [
        "--scan",
        "--max-lhs",
        "1",
        "--max-rhs",
        "2",
        "--max-arity",
        "2",
        "--max-vars",
        "3",
        "--steps",
        "3",
        "--budget-states",
        "60",
        "--budget-edges",
        "24",
        "--top",
        "5",
        "--quiet",
        "--atlas",
        dir.to_str().unwrap(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// End-to-end: `--scan --atlas DIR` writes index.html (baked, no
/// placeholder left, atlas data present) and one rule-NNN.html per
/// entry (a full viewer page, baked through the existing template).
#[test]
fn atlas_bake_end_to_end() {
    let dir = std::env::temp_dir().join(format!("multiway-atlas-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_multiway"))
        .args(scan_args(&dir))
        .output()
        .expect("failed to run multiway");
    assert!(out.status.success(), "scan failed: {:?}", out);

    let index = std::fs::read_to_string(dir.join("index.html")).unwrap();
    assert!(!index.contains("__ATLAS_JSON__"), "placeholder left baked");
    assert!(index.contains("\"entries\""), "atlas data missing");

    for i in 1..=5 {
        let page = dir.join(format!("rule-{:03}.html", i));
        let html = std::fs::read_to_string(&page)
            .unwrap_or_else(|e| panic!("missing {}: {}", page.display(), e));
        assert!(
            !html.contains("__DATA_JSON__"),
            "rule page {} left unbaked",
            i
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// `--count` prints the exact Burnside size instantly — no probing.
#[test]
fn scan_count_exact() {
    let out = Command::new(env!("CARGO_BIN_EXE_multiway"))
        .args([
            "--scan",
            "--count",
            "--max-lhs",
            "2",
            "--max-rhs",
            "3",
            "--max-arity",
            "3",
            "--max-vars",
            "4",
        ])
        .output()
        .expect("failed to run multiway");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("16,184,498"),
        "--count must print the exact class count: {}",
        stdout
    );
}

/// Exhaustive scans beyond the cap refuse with exit 2 and the exact
/// size — never a silent truncation.
#[test]
fn scan_cap_refusal_exit_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_multiway"))
        .args([
            "--scan",
            "--max-lhs",
            "2",
            "--max-rhs",
            "3",
            "--max-arity",
            "3",
            "--max-vars",
            "4",
        ])
        .output()
        .expect("failed to run multiway");
    assert_eq!(out.status.code(), Some(2), "cap refusal must exit 2");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("16184498") || stderr.contains("16,184,498"),
        "refusal must print the exact size: {}",
        stderr
    );
    assert!(stderr.contains("--sample"), "refusal must suggest --sample");
}
