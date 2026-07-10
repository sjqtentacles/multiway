//! Multiway evolution with canonical-state sharing.
//!
//! The naive multiway object is a tree: every state branches into one
//! child per match. We instead merge isomorphic states globally, so the
//! result is a DAG over canonical states — the e-graph move applied at
//! state granularity. `path_counts` reports how many naive tree paths
//! each canonical node absorbs, i.e. how much work the sharing saved.

use crate::canon::{canonicalize, Canon};
use crate::det::DetMap;
use crate::hypergraph::{Edge, State};
use crate::matcher::{apply_full, delta_matches, find_matches, Application, Match};
use crate::rule::Rule;

/// Monotonic profiling timer. wasm32-unknown-unknown has no runtime
/// clock — `Instant::now()` COMPILES there but traps at runtime — so the
/// wasm variant is a zero-cost stub returning 0 and the lib stays
/// wasm-clean. Timing values feed only [`EvolveStats`], which is never
/// serialized into any export (goldens cannot move).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
struct ProfTimer(std::time::Instant);

#[cfg(not(target_arch = "wasm32"))]
impl ProfTimer {
    fn start() -> Self {
        ProfTimer(std::time::Instant::now())
    }
    fn elapsed_ns(self) -> u128 {
        self.0.elapsed().as_nanos()
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
struct ProfTimer;

#[cfg(target_arch = "wasm32")]
impl ProfTimer {
    fn start() -> Self {
        ProfTimer
    }
    fn elapsed_ns(self) -> u128 {
        0
    }
}

/// One canonical state of the multiway system.
pub struct StateRec {
    /// Dense id (index into `MultiwaySystem::states`).
    pub id: usize,
    /// Step at which this canonical state was first reached.
    pub step: usize,
    /// Raw first-reached representative (kept for viewer readability —
    /// matches are found on this).
    pub state: State,
    /// Canonical form + witness (vertex_map, edge_slots). The form is the
    /// dedup key; the witness gives every edge instance a stable slot
    /// identity for the token-event graph.
    pub canon: Canon,
}

/// One rewrite event between canonical states.
pub struct Event {
    /// Dense id (index into `MultiwaySystem::events`).
    pub id: usize,
    /// Parent canonical state.
    pub from: usize,
    /// Child canonical state (possibly first reached earlier — a merge).
    pub to: usize,
    /// Step at which this event fired.
    pub step: usize,
}

/// Per-event token flow, in canonical-slot coordinates. A token is
/// `(state id, slot)` where `slot` indexes the state's canonical sorted
/// edge list — the engine's fixed token-identity convention (see `teg`).
pub struct EventTokens {
    /// Parent canonical slots consumed (sorted).
    pub consumed: Vec<usize>,
    /// Child canonical slots produced (sorted).
    pub produced: Vec<usize>,
    /// `(parent_slot, child_slot)` for every edge that rode through
    /// untouched, in parent-edge order.
    pub passthrough: Vec<(usize, usize)>,
}

/// A multiway evolution: canonical states, events, and derived structure.
pub struct MultiwaySystem {
    /// Canonical states in discovery order.
    pub states: Vec<StateRec>,
    /// Events in creation order.
    pub events: Vec<Event>,
    /// Token flow per event (parallel to `events`).
    pub event_tokens: Vec<EventTokens>,
    /// States first reached at each step (`layers[0]` holds the initial state).
    pub layers: Vec<Vec<usize>>,
    /// Branchial pairs: same-step sibling states produced from a common parent.
    pub branchial: Vec<(usize, usize)>,
    /// Events that merged into a state first reached at an *earlier* step.
    /// If nonzero, `path_counts` no longer aligns with the naive evolution
    /// tree: it counts walks in the merged DAG, which can over- OR
    /// under-state per-layer naive counts (see `path_counts`).
    pub back_merges: usize,
    /// Match-maintenance telemetry.
    pub stats: EvolveStats,
}

/// Telemetry counters for match maintenance (pinned by tests: incremental
/// evolve performs exactly ONE full search — the initial state — and one
/// delta per new canonical state; merged children never get match sets).
#[derive(Default)]
pub struct EvolveStats {
    /// Full `find_matches` invocations.
    pub full_match_calls: usize,
    /// `delta_matches` invocations.
    pub delta_match_calls: usize,
    /// Maximum worker threads spawned in any step (0 on the serial path)
    /// — distinguishes "parallel implemented" from "threads ignored".
    pub workers_spawned: usize,
    /// Wall-clock attribution (diagnostics only — NEVER exported; zero on
    /// wasm where no runtime clock exists): Phase A fan-out
    /// (apply + canonicalize).
    pub phase_a_ns: u128,
    /// Phase B serial bookkeeping (dedup, ids, tokens, branchial).
    pub phase_b_ns: u128,
    /// Phase C fan-out (match maintenance for the new layer).
    pub phase_c_ns: u128,
    /// Serial teardown of the per-step expansion buffers.
    pub drop_ns: u128,
}

/// Evolution options. `threads > 1` parallelizes the pure per-child work
/// (apply + canonicalize, then delta matching) with `std::thread::scope`;
/// output is byte-identical for every thread count by construction (see
/// `evolve_opts`). `incremental: false` selects the reference full-search
/// path (kept for the bit-identical differential test).
pub struct EvolveOpts {
    /// Multiway BFS depth.
    pub steps: usize,
    /// Worker threads (currently must be 1).
    pub threads: usize,
    /// Delta-maintain match sets instead of re-searching every state.
    pub incremental: bool,
}

impl Default for EvolveOpts {
    fn default() -> Self {
        EvolveOpts {
            steps: 0,
            threads: 1,
            incremental: true,
        }
    }
}

/// Evolve `steps` multiway layers with default options (serial,
/// incremental matching).
///
/// ```
/// let rule = multiway::rule::parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
/// let init = multiway::rule::parse_state("{{0,0}}").unwrap();
/// let mw = multiway::system::evolve(&rule, init, 2);
/// assert_eq!(mw.layers.iter().map(|l| l.len()).collect::<Vec<_>>(), vec![1, 1, 2]);
/// ```
pub fn evolve(rule: &Rule, init: State, steps: usize) -> MultiwaySystem {
    evolve_opts(
        rule,
        init,
        &EvolveOpts {
            steps,
            ..EvolveOpts::default()
        },
    )
}

/// Evolve with options. Parallel discipline (deterministic by
/// construction):
///
/// - **Phase A** fans the pure per-match work (`apply_full` and
///   `canonicalize`) across scoped workers on round-robin frontier
///   indices, collected by index;
/// - **Phase B** replays the bookkeeping (dedup, event ids, tokens,
///   branchial) serially in `(frontier, match)` order — a pure function
///   of the index-sorted Phase A results, so the output cannot depend on
///   thread timing;
/// - **Phase C** fans out `delta_matches` for the new layer the same way.
pub fn evolve_opts(rule: &Rule, init: State, opts: &EvolveOpts) -> MultiwaySystem {
    assert!(opts.threads >= 1, "threads must be >= 1");
    // Tier-2 profiling: fine Phase B attribution, opt-in via env, printed
    // to STDERR only (stdout is golden-compared). env::var returns Err on
    // wasm — safe everywhere.
    let profile = std::env::var("MULTIWAY_PROFILE").is_ok();
    let mut prof_lookup_ns: u128 = 0;
    let mut prof_clone_ns: u128 = 0;
    let mut prof_tokens_ns: u128 = 0;
    let mut prof_branchial_ns: u128 = 0;
    let steps = opts.steps;
    let mut mw = MultiwaySystem {
        states: Vec::new(),
        events: Vec::new(),
        event_tokens: Vec::new(),
        layers: Vec::new(),
        branchial: Vec::new(),
        back_merges: 0,
        stats: EvolveStats::default(),
    };
    // Dedup key: the canonical form's edge list. No bucket scans, no
    // in-loop isomorphism checks — form equality IS isomorphism.
    let mut canon_map: DetMap<Vec<Edge>, usize> = DetMap::default();

    let c0 = canonicalize(&init);
    canon_map.insert(c0.form.edges.clone(), 0);
    mw.states.push(StateRec {
        id: 0,
        step: 0,
        state: init,
        canon: c0,
    });
    mw.layers.push(vec![0]);

    mw.stats.full_match_calls += 1;
    let init_matches = find_matches(&mw.states[0].state, rule);
    let mut frontier: Vec<(usize, Vec<Match>)> = vec![(0, init_matches)];
    for step in 1..=steps {
        let mut new_layer: Vec<usize> = Vec::new();
        let mut branch_pairs: Vec<(usize, usize)> = Vec::new();

        // Phase A: pure per-match work, optionally parallel.
        let t = ProfTimer::start();
        let mut expanded: Vec<Vec<(Application, Option<Canon>)>> =
            phase_a(rule, &mw.states, &frontier, opts.threads, &mut mw.stats);
        mw.stats.phase_a_ns += t.elapsed_ns();

        // Phase B: serial bookkeeping in (frontier, match) order.
        let t = ProfTimer::start();
        // Pending delta computations for Phase C: (cid, fi, mi).
        let mut pending: Vec<(usize, usize, usize)> = Vec::new();
        for (fi, (sid, matches)) in frontier.iter().enumerate() {
            let sid = *sid;
            let mut children: Vec<usize> = Vec::new();

            for (mi, m) in matches.iter().enumerate() {
                let app = &expanded[fi][mi].0;
                let c = expanded[fi][mi].1.as_ref().unwrap();
                let bt = if profile {
                    Some(ProfTimer::start())
                } else {
                    None
                };

                // Token flow in canonical-slot coordinates. Matches are
                // found on the parent's raw representative, so the
                // parent's own edge_slots apply; the child's slots index
                // the SHARED canonical edge list even when the child
                // merges (byte-equal forms).
                let mut consumed: Vec<usize> = m
                    .edge_idx
                    .iter()
                    .map(|&i| mw.states[sid].canon.edge_slots[i])
                    .collect();
                consumed.sort_unstable();
                let mut produced: Vec<usize> =
                    app.produced.clone().map(|i| c.edge_slots[i]).collect();
                produced.sort_unstable();
                let passthrough: Vec<(usize, usize)> = app
                    .kept
                    .iter()
                    .map(|&(pi, ci)| (mw.states[sid].canon.edge_slots[pi], c.edge_slots[ci]))
                    .collect();

                if let Some(bt) = bt {
                    prof_tokens_ns += bt.elapsed_ns();
                }

                let lt = if profile {
                    Some(ProfTimer::start())
                } else {
                    None
                };
                let cid = match canon_map.get(&c.form.edges) {
                    Some(&cid) => {
                        if mw.states[cid].step < step {
                            mw.back_merges += 1;
                        }
                        cid
                    }
                    None => {
                        let cid = mw.states.len();
                        canon_map.insert(c.form.edges.clone(), cid);
                        // Lazy match maintenance: only NEW canonical
                        // states get a match set (merged children are
                        // never expanded); computed in Phase C.
                        pending.push((cid, fi, mi));
                        let canon = expanded[fi][mi].1.take().unwrap();
                        let ct = if profile {
                            Some(ProfTimer::start())
                        } else {
                            None
                        };
                        let child = expanded[fi][mi].0.child.clone();
                        if let Some(ct) = ct {
                            prof_clone_ns += ct.elapsed_ns();
                        }
                        mw.states.push(StateRec {
                            id: cid,
                            step,
                            state: child,
                            canon,
                        });
                        new_layer.push(cid);
                        cid
                    }
                };

                if let Some(lt) = lt {
                    prof_lookup_ns += lt.elapsed_ns();
                }

                let eid = mw.events.len();
                mw.events.push(Event {
                    id: eid,
                    from: sid,
                    to: cid,
                    step,
                });
                mw.event_tokens.push(EventTokens {
                    consumed,
                    produced,
                    passthrough,
                });
                if !children.contains(&cid) {
                    children.push(cid);
                }
            }

            for i in 0..children.len() {
                for j in (i + 1)..children.len() {
                    let a = children[i].min(children[j]);
                    let b = children[i].max(children[j]);
                    branch_pairs.push((a, b));
                }
            }
        }

        let brt = if profile {
            Some(ProfTimer::start())
        } else {
            None
        };
        branch_pairs.sort_unstable();
        branch_pairs.dedup();
        mw.branchial.extend(branch_pairs);
        if let Some(brt) = brt {
            prof_branchial_ns += brt.elapsed_ns();
        }
        mw.layers.push(new_layer);
        mw.stats.phase_b_ns += t.elapsed_ns();

        // Phase C: match sets for the new layer, optionally parallel;
        // assembled in pending (= new-state id) order.
        let t = ProfTimer::start();
        frontier = phase_c(rule, &frontier, &expanded, &pending, opts, &mut mw.stats);
        mw.stats.phase_c_ns += t.elapsed_ns();

        // Serial teardown of the expansion buffers is real time at wide
        // layers (~2M small allocs at depth 6) — attribute it.
        let t = ProfTimer::start();
        drop(expanded);
        mw.stats.drop_ns += t.elapsed_ns();

        if frontier.is_empty() {
            break;
        }
    }
    if profile {
        eprintln!(
            "PROFILE phases: a={}ms b={}ms c={}ms drop={}ms | phase-b buckets: \
             tokens={}ms lookup+intern={}ms state-clone={}ms branchial={}ms",
            mw.stats.phase_a_ns / 1_000_000,
            mw.stats.phase_b_ns / 1_000_000,
            mw.stats.phase_c_ns / 1_000_000,
            mw.stats.drop_ns / 1_000_000,
            prof_tokens_ns / 1_000_000,
            prof_lookup_ns / 1_000_000,
            prof_clone_ns / 1_000_000,
            prof_branchial_ns / 1_000_000,
        );
    }
    mw
}

/// Phase A: for every frontier state, apply every match and canonicalize
/// the child. Pure per-item work — workers own round-robin index sets and
/// return results collected BY INDEX, so the merged vector is identical
/// for any thread count or scheduling.
fn phase_a(
    rule: &Rule,
    states: &[StateRec],
    frontier: &[(usize, Vec<Match>)],
    threads: usize,
    stats: &mut EvolveStats,
) -> Vec<Vec<(Application, Option<Canon>)>> {
    let expand_one = |fi: usize| -> Vec<(Application, Option<Canon>)> {
        let (sid, matches) = &frontier[fi];
        let parent = &states[*sid].state;
        matches
            .iter()
            .map(|m| {
                let app = apply_full(parent, rule, m);
                let c = canonicalize(&app.child);
                (app, Some(c))
            })
            .collect()
    };

    let workers = threads.min(frontier.len());
    if workers <= 1 {
        return (0..frontier.len()).map(expand_one).collect();
    }
    stats.workers_spawned = stats.workers_spawned.max(workers);

    let mut merged: Vec<Vec<(Application, Option<Canon>)>> =
        (0..frontier.len()).map(|_| Vec::new()).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|t| {
                let expand_one = &expand_one;
                s.spawn(move || {
                    let mut out = Vec::new();
                    let mut fi = t;
                    while fi < frontier.len() {
                        out.push((fi, expand_one(fi)));
                        fi += workers;
                    }
                    out
                })
            })
            .collect();
        for h in handles {
            for (fi, cands) in h.join().expect("phase A worker panicked") {
                merged[fi] = cands;
            }
        }
    });
    merged
}

/// Phase C: compute the next frontier's match sets (delta or full),
/// fanned out the same way and assembled in new-state-id order.
fn phase_c(
    rule: &Rule,
    frontier: &[(usize, Vec<Match>)],
    expanded: &[Vec<(Application, Option<Canon>)>],
    pending: &[(usize, usize, usize)],
    opts: &EvolveOpts,
    stats: &mut EvolveStats,
) -> Vec<(usize, Vec<Match>)> {
    let compute_one = |&(cid, fi, mi): &(usize, usize, usize)| -> (usize, Vec<Match>) {
        let (_, matches) = &frontier[fi];
        let app = &expanded[fi][mi].0;
        let ms = if opts.incremental {
            delta_matches(rule, matches, &matches[mi], app)
        } else {
            find_matches(&app.child, rule)
        };
        (cid, ms)
    };
    if opts.incremental {
        stats.delta_match_calls += pending.len();
    } else {
        stats.full_match_calls += pending.len();
    }

    let workers = opts.threads.min(pending.len());
    if workers <= 1 {
        return pending.iter().map(compute_one).collect();
    }
    stats.workers_spawned = stats.workers_spawned.max(workers);

    let mut merged: Vec<Option<(usize, Vec<Match>)>> = vec![None; pending.len()];
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|t| {
                let compute_one = &compute_one;
                s.spawn(move || {
                    let mut out = Vec::new();
                    let mut pi = t;
                    while pi < pending.len() {
                        out.push((pi, compute_one(&pending[pi])));
                        pi += workers;
                    }
                    out
                })
            })
            .collect();
        for h in handles {
            for (pi, entry) in h.join().expect("phase C worker panicked") {
                merged[pi] = Some(entry);
            }
        }
    });
    merged.into_iter().map(|e| e.unwrap()).collect()
}

impl MultiwaySystem {
    /// For each canonical state, the number of distinct paths from the
    /// initial state — i.e. how many nodes of the naive (unshared)
    /// evolution tree this single node represents. Computed by DP in
    /// event-creation order, which is topological as long as no event
    /// merges into an earlier layer.
    ///
    /// With back-merges (`back_merges > 0`) the DP is NOT a lower bound —
    /// it counts walks in the merged DAG, which can over- or under-state
    /// per-layer naive counts (a cyclic merge can even report the initial
    /// state as having multiple "tree nodes"). Gate any per-layer display
    /// on `back_merges == 0`.
    pub fn path_counts(&self) -> Vec<u128> {
        let mut p = vec![0u128; self.states.len()];
        if !p.is_empty() {
            p[0] = 1;
        }
        for e in &self.events {
            p[e.to] = p[e.to].saturating_add(p[e.from]);
        }
        p
    }

    /// Naive-tree node count per layer vs canonical count per layer.
    pub fn sharing_per_layer(&self) -> Vec<(usize, u128, usize)> {
        let pc = self.path_counts();
        self.layers
            .iter()
            .enumerate()
            .map(|(step, ids)| {
                let paths: u128 = ids.iter().map(|&i| pc[i]).sum();
                (step, paths, ids.len())
            })
            .collect()
    }
}
