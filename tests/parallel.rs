//! Deterministic parallelism: thread-count invariance of evolve, and the
//! standard (maximal pairwise-disjoint generations) updating order.
//!
//! Genuinely red before implementation: M5 shipped `threads > 1` as a
//! panic ("parallel evolve lands in M6"), so the first test below failed
//! by construction until the parallel path existed.

mod common;

use multiway::causal::{run, run_ordered, UpdateOrder};
use multiway::export::bundle_json;
use multiway::rule::{parse_rule, parse_state};
use multiway::system::{evolve, evolve_opts, EvolveOpts};

const CLASSIC: &str = "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}";

/// Output is a pure function of the input for EVERY thread count: byte-
/// equal bundles across t ∈ {1,2,3,7} and against the plain evolve()
/// wrapper. workers_spawned distinguishes "parallel implemented" from
/// "threads silently ignored".
#[test]
fn parallel_equals_serial_all_thread_counts() {
    let rule = parse_rule(CLASSIC).unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();

    let reference = bundle_json(
        &rule.text,
        "{{0,0},{0,0}}",
        &evolve(&rule, init.clone(), 4),
        None,
    );
    for t in [1usize, 2, 3, 7] {
        let mw = evolve_opts(
            &rule,
            init.clone(),
            &EvolveOpts {
                steps: 4,
                threads: t,
                incremental: true,
            },
        );
        assert_eq!(
            bundle_json(&rule.text, "{{0,0},{0,0}}", &mw, None),
            reference,
            "thread count {} changed the output",
            t
        );
        if t > 1 {
            // widest frontier is 18 (step 3->4 expansion), so all workers engage
            assert_eq!(
                mw.stats.workers_spawned, t,
                "threads silently ignored at t={}",
                t
            );
        } else {
            assert_eq!(mw.stats.workers_spawned, 0, "serial path spawns nothing");
        }
    }
}

/// Same thread count twice: byte-equal (catches scheduling-dependent
/// leaks a single cross-count comparison might miss).
#[test]
fn parallel_repeat_stability() {
    let rule = parse_rule(CLASSIC).unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let render = || {
        let mw = evolve_opts(
            &rule,
            init.clone(),
            &EvolveOpts {
                steps: 4,
                threads: 4,
                incremental: true,
            },
        );
        bundle_json(&rule.text, "{{0,0},{0,0}}", &mw, None)
    };
    assert_eq!(render(), render());
}

/// Standard updating order: two vertex-disjoint growth matches fire in
/// ONE generation; each event consumes 1 edge and adds 2.
#[test]
fn standard_order_disjoint_generation() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let init = parse_state("{{0,0},{1,1}}").unwrap();
    let c = run_ordered(&rule, init, 6, UpdateOrder::StandardGenerations);

    assert_eq!(c.generations, vec![2, 4], "gen 1: both edges; gen 2: all 4");
    assert_eq!(c.n_events, 7); // event 0 + 6 rewrites
    assert_eq!(c.final_state.edge_count(), 8); // +1 edge per event
}

/// Overlapping matches must NOT fire together: the classic init's two
/// matches consume the same two edges, so each generation has exactly
/// one event.
#[test]
fn standard_order_overlap_respected() {
    let rule = parse_rule(CLASSIC).unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let c = run_ordered(&rule, init, 3, UpdateOrder::StandardGenerations);
    assert_eq!(c.generations[0], 1, "overlapping matches fired together");
}

/// Sequential mode through run_ordered is byte-identical to run().
#[test]
fn sequential_mode_unchanged() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let a = run(&rule, init.clone(), 5);
    let b = run_ordered(&rule, init, 5, UpdateOrder::Sequential);
    assert_eq!(a.deps, b.deps);
    assert_eq!(a.n_events, b.n_events);
    assert_eq!(a.final_state.edges, b.final_state.edges);
    assert_eq!(b.generations, vec![1; 5]);
}

/// Cumulative baseline re-pin, parallel path included.
#[test]
fn baseline_final() {
    let rule = parse_rule(CLASSIC).unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve_opts(
        &rule,
        init,
        &EvolveOpts {
            steps: 4,
            threads: 2,
            incremental: true,
        },
    );
    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 3, 18, 156]);
    assert_eq!(mw.back_merges, 0);
}

/// A1 profiling: the always-on phase timers populate on any real run.
/// (On wasm32 the cfg-gated shim returns 0 — the lib stays wasm-clean —
/// but native tests must see real attribution.)
#[test]
fn phase_timers_populate() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let mw = evolve(&rule, init, 3);
    assert!(mw.stats.phase_a_ns > 0, "phase A untimed");
    assert!(mw.stats.phase_b_ns > 0, "phase B untimed");
    assert!(mw.stats.phase_c_ns > 0, "phase C untimed");
    // drop_ns can be ~0 for tiny runs but the field must exist and be
    // populated by the timed drop path (>= 0 is trivially true; the
    // compile-time existence is the pin).
    let _ = mw.stats.drop_ns;
}

/// Depth-6 thread byte-equality — the regime that actually stresses
/// Phase B ordering (24.7k-state final layer). Release-only via the CI
/// perf job (`cargo test --release -- --ignored`).
#[test]
#[ignore]
fn depth6_thread_byte_equality() {
    let rule = parse_rule(CLASSIC).unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let render = |t: usize| {
        let mw = evolve_opts(
            &rule,
            init.clone(),
            &EvolveOpts {
                steps: 6,
                threads: t,
                incremental: true,
            },
        );
        bundle_json(&rule.text, "{{0,0},{0,0}}", &mw, None)
    };
    let serial = render(1);
    assert_eq!(serial, render(4), "4 threads changed depth-6 output");
}

/// Scaling smoke with generous margin (measured 0.77x at 4 threads on a
/// warm process; shared CI runners are noisy, so the bound only catches
/// a wholesale loss of parallelism). Release-only.
#[test]
#[ignore]
fn depth6_scaling_smoke() {
    let rule = parse_rule(CLASSIC).unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let time_one = |t: usize| {
        let start = std::time::Instant::now();
        let _ = evolve_opts(
            &rule,
            init.clone(),
            &EvolveOpts {
                steps: 6,
                threads: t,
                incremental: true,
            },
        );
        start.elapsed()
    };
    let serial = time_one(1);
    let threaded = time_one(4);
    assert!(
        threaded < serial,
        "4 threads slower than serial: {:?} vs {:?}",
        threaded,
        serial
    );
}
