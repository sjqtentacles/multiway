//! Zero-dependency benchmark harness: `cargo run --release --bin bench`.
//!
//! Fixed inputs, `Instant`-based timing, warmup + median-of-7. Emits
//! machine-readable `BENCH` lines and a markdown table (used to
//! regenerate the README numbers). Wall-clock times are never part of
//! deterministic exports — this binary is a separate diagnostic surface.

use multiway::confluence::{check, CheckCfg};
use multiway::matcher::find_matches;
use multiway::rule::{parse_rule, parse_state};
use multiway::system::{evolve, evolve_opts, EvolveOpts};
use std::time::Instant;

struct BenchResult {
    name: &'static str,
    median_ns: u128,
    min_ns: u128,
}

fn time_median(name: &'static str, warmup: usize, runs: usize, mut f: impl FnMut()) -> BenchResult {
    for _ in 0..warmup {
        f();
    }
    let mut samples: Vec<u128> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_nanos()
        })
        .collect();
    samples.sort_unstable();
    BenchResult {
        name,
        median_ns: samples[samples.len() / 2],
        min_ns: samples[0],
    }
}

fn ms(ns: u128) -> String {
    format!("{:.2}", ns as f64 / 1e6)
}

fn main() {
    let classic = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let growth = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let double_loop = parse_state("{{0,0},{0,0}}").unwrap();
    let single_loop = parse_state("{{0,0}}").unwrap();
    // fixed LOW-SYMMETRY state for canonization timing (a symmetric one
    // could trip the IR search's worst case and benchmark the pathology
    // instead of the common case)
    let chain20 = parse_state(
        "{{0,1},{1,2},{2,3},{3,4},{4,5},{5,6},{6,7},{7,8},{8,9},{9,10},\
         {10,11},{11,12},{12,13},{13,14},{14,15},{15,16},{16,17},{17,18},{18,19},{19,0}}",
    )
    .unwrap();
    let star30 = parse_state(&format!(
        "{{{}}}",
        (1..=30)
            .map(|i| format!("{{0,{}}}", i))
            .collect::<Vec<_>>()
            .join(",")
    ))
    .unwrap();

    let results = vec![
        time_median("classic_depth4", 1, 7, || {
            let _ = evolve(&classic, double_loop.clone(), 4);
        }),
        time_median("classic_depth5", 1, 7, || {
            let _ = evolve(&classic, double_loop.clone(), 5);
        }),
        time_median("classic_depth5_threads4", 1, 7, || {
            let _ = evolve_opts(
                &classic,
                double_loop.clone(),
                &EvolveOpts {
                    steps: 5,
                    threads: 4,
                    incremental: true,
                },
            );
        }),
        time_median("growth_depth8", 1, 7, || {
            let _ = evolve(&growth, single_loop.clone(), 8);
        }),
        time_median("canonicalize_chain20_x100", 1, 7, || {
            for _ in 0..100 {
                let _ = multiway::canon::canonicalize(&chain20);
            }
        }),
        time_median("find_matches_star30", 1, 7, || {
            let _ = find_matches(&star30, &classic);
        }),
        time_median("confluence_classic_check", 1, 7, || {
            let _ = check(
                std::slice::from_ref(&classic),
                &CheckCfg {
                    join_depth: 2,
                    max_states: 200,
                    pair_cap: 512,
                },
            );
        }),
    ];

    for r in &results {
        println!(
            "BENCH {} median_ns={} min_ns={}",
            r.name, r.median_ns, r.min_ns
        );
    }
    println!();
    println!("| scenario | median (ms) | min (ms) |");
    println!("|---|---:|---:|");
    for r in &results {
        println!("| {} | {} | {} |", r.name, ms(r.median_ns), ms(r.min_ns));
    }
}
