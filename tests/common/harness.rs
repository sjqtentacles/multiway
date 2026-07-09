//! Zero-dependency property-test runner.
//!
//! Every case derives its own seed as `mix(file_seed ^ case_index)`, so any
//! failure is reproducible in isolation:
//!
//! ```text
//! MULTIWAY_PROP_SEED=0x... MULTIWAY_PROP_CASE=41 cargo test <test-fn-name>
//! ```
//!
//! Env knobs (all optional):
//! - `MULTIWAY_PROP_CASES`: absolute case count (default 100; CI cranks it)
//! - `MULTIWAY_PROP_CASE`: run exactly one case index
//! - `MULTIWAY_PROP_SEED`: override the file seed (decimal or 0x-hex)

use super::prng::Rng;
use multiway::det::mix;
use multiway::hypergraph::State;

pub const DEFAULT_CASES: usize = 100;

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok()?.parse().ok()
}

fn env_u64(name: &str) -> Option<u64> {
    let v = std::env::var(name).ok()?;
    if let Some(hex) = v.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        v.parse().ok()
    }
}

/// Prints a one-line repro command if (and only if) the case panics.
/// Drop-based so it covers `assert!` failures and unexpected panics inside
/// library code alike, with no `Result` plumbing (default test profile
/// unwinds).
struct CaseGuard<'a> {
    name: &'a str,
    seed: u64,
    case: usize,
}

impl Drop for CaseGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "PROP FAIL {} — repro: MULTIWAY_PROP_SEED={:#x} MULTIWAY_PROP_CASE={} cargo test {}",
                self.name, self.seed, self.case, self.name
            );
        }
    }
}

/// Run `f` for each case with an independently seeded [`Rng`]. `name` must
/// be the test fn name so the printed repro line filters correctly.
pub fn prop(file_seed: u64, name: &str, mut f: impl FnMut(&mut Rng, usize)) {
    let cases = env_usize("MULTIWAY_PROP_CASES").unwrap_or(DEFAULT_CASES);
    let only = env_usize("MULTIWAY_PROP_CASE");
    let seed = env_u64("MULTIWAY_PROP_SEED").unwrap_or(file_seed);
    let range = match only {
        Some(c) => c..c + 1,
        None => 0..cases,
    };
    for i in range {
        let _guard = CaseGuard {
            name,
            seed,
            case: i,
        };
        let mut rng = Rng::new(mix(seed ^ i as u64));
        f(&mut rng, i);
    }
}

/// Greedy edge-drop shrinking: repeatedly remove one edge while the failure
/// persists. Deliberately the only shrinker we ship — canon properties take
/// a single state, engine-property inputs are generated tiny already, and
/// structured rule shrinking would cost ~150 LOC for marginal benefit.
pub fn shrink_state(mut s: State, still_fails: impl Fn(&State) -> bool) -> State {
    loop {
        let mut shrunk = false;
        for i in 0..s.edges.len() {
            let mut edges = s.edges.clone();
            edges.remove(i);
            let t = State::new(edges);
            if still_fails(&t) {
                s = t;
                shrunk = true;
                break;
            }
        }
        if !shrunk {
            return s;
        }
    }
}
