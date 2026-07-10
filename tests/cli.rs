//! Binary-level CLI behavior (via `CARGO_BIN_EXE_`, zero dev-deps).
//! The stdout/JSON goldens live in tests/golden.rs; this file covers the
//! argument-handling surface.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_multiway"))
}

#[test]
fn cli_rejects_missing_args_with_usage() {
    let out = bin().output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("USAGE"), "no usage text: {}", stderr);
}

#[test]
fn cli_rejects_unknown_argument() {
    let out = bin().arg("--frobnicate").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown argument"));
}

#[test]
fn cli_rejects_bad_rule_with_parse_error() {
    let out = bin()
        .args(["--rule", "{{x,y}", "--init", "{{0,0}}", "--steps", "1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("rule parse error"));
}

#[test]
fn cli_html_embeds_json_and_leaves_no_placeholder() {
    let tmp = std::env::temp_dir().join(format!("multiway-cli-{}.html", std::process::id()));
    let out = bin()
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
    assert!(
        !html.contains("__DATA_JSON__"),
        "placeholder survived baking"
    );
    assert!(html.contains("\"rule\":\""), "bundle not embedded");
    assert!(html.contains("\"teg\":"), "teg section missing from bundle");
}

/// Evolution mode takes exactly one rule; the analysis modes are the
/// repeatable-rule surface.
#[test]
fn cli_evolution_rejects_multiple_rules() {
    let out = bin()
        .args([
            "--rule",
            "{{x,y}}->{{x,y},{y,z}}",
            "--rule",
            "{{x,y}}->{{y,x}}",
            "--init",
            "{{0,0}}",
            "--steps",
            "1",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("exactly one"));
}

#[test]
fn cli_check_confluence_smoke() {
    let out = bin()
        .args(["--check-confluence", "--rule", "{{x,y},{x,y}}->{{x,y}}"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("confluent: YES"), "got: {}", stdout);
}

#[test]
fn cli_threads_flag_byte_identical() {
    let render = |threads: &str| {
        let tmp = std::env::temp_dir().join(format!(
            "multiway-threads-{}-{}.json",
            threads,
            std::process::id()
        ));
        let out = bin()
            .args([
                "--rule",
                "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}",
                "--init",
                "{{0,0},{0,0}}",
                "--steps",
                "3",
                "--threads",
                threads,
                "--quiet",
                "--json",
                tmp.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        let json = std::fs::read(&tmp).unwrap();
        std::fs::remove_file(&tmp).ok();
        json
    };
    assert_eq!(render("1"), render("4"));
}

/// A1: profiling output must NEVER reach stdout (stdout is golden-
/// compared; MULTIWAY_PROFILE writes to stderr only).
#[test]
fn prop_profile_env_never_reaches_stdout() {
    let args = [
        "--rule",
        "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}",
        "--init",
        "{{0,0},{0,0}}",
        "--steps",
        "3",
    ];
    let plain = bin().args(args).output().unwrap();
    let profiled = bin()
        .args(args)
        .env("MULTIWAY_PROFILE", "1")
        .output()
        .unwrap();
    assert!(plain.status.success() && profiled.status.success());
    assert_eq!(
        plain.stdout, profiled.stdout,
        "MULTIWAY_PROFILE leaked into stdout"
    );
    assert!(
        String::from_utf8_lossy(&profiled.stderr).contains("PROFILE"),
        "profiling requested but nothing on stderr"
    );
}
