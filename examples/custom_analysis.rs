//! This is a library, not just a CLI: compute things the CLI doesn't
//! print by walking `MultiwaySystem` directly — here, the per-layer
//! vertex-count distribution, the multiway DAG's maximum in-degree, and
//! the token-event graph's causal edge count.
//! Run with `cargo run --example custom_analysis`.

use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;
use multiway::teg;

fn main() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve(&rule, init, 4);

    // per-layer vertex counts (this rule mints exactly one vertex per step)
    for (step, layer) in mw.layers.iter().enumerate() {
        let counts: Vec<usize> = layer
            .iter()
            .map(|&id| mw.states[id].state.vertices().len())
            .collect();
        let min = counts.iter().min().unwrap();
        let max = counts.iter().max().unwrap();
        println!(
            "layer {}: {} states, {}..={} vertices",
            step,
            layer.len(),
            min,
            max
        );
    }

    // which canonical state absorbs the most incoming events?
    let mut in_degree = vec![0usize; mw.states.len()];
    for e in &mw.events {
        in_degree[e.to] += 1;
    }
    let (busiest, degree) = in_degree
        .iter()
        .enumerate()
        .max_by_key(|&(_, d)| d)
        .unwrap();
    println!(
        "busiest state: id {} with {} incoming events ({} naive paths)",
        busiest,
        degree,
        mw.path_counts()[busiest]
    );

    // the token-event graph: causal structure across ALL updating orders
    let t = teg::build(&mw);
    println!(
        "token-event graph: {} causal edges, {} branchial event pairs",
        t.causal.len(),
        t.branchial_events.len()
    );
}
