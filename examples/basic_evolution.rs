//! The README quick-start as library code: evolve a rule, print the
//! sharing table data. Run with `cargo run --example basic_evolution`.

use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;

fn main() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();

    let mw = evolve(&rule, init, 4);

    println!("rule {}", rule.text);
    for (step, tree_nodes, canonical) in mw.sharing_per_layer() {
        println!(
            "step {}: {} naive tree nodes -> {} canonical states",
            step, tree_nodes, canonical
        );
    }
    println!(
        "{} states, {} events, {} branchial pairs",
        mw.states.len(),
        mw.events.len(),
        mw.branchial().len()
    );
}
