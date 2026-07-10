//! Deterministic allocation-count regression pin — the perf test that
//! wall-clock timing can't give (A1's profiling showed the depth-6 cost
//! is dominated by allocation volume, not compute).
//!
//! This file contains EXACTLY ONE test: the counter is process-global
//! and libtest runs tests within a binary concurrently, but each file
//! under tests/ is its own binary, so isolation is structural.
//!
//! The pin is an UPPER BOUND with stated headroom, not an exact count:
//! std container allocation patterns differ across {stable, 1.63} × 3
//! OSes, and an exact pin minted on one cell would flake on another.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingAlloc;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static COUNTER: CountingAlloc = CountingAlloc;

/// Classic rule, depth 4 (496 events): the whole evolution must stay
/// under an allocation budget per event. Pre-diet baseline: 180
/// allocs/event (the bound below was red against it). Post-diet
/// (reference-sorted refinement, moved form edges, moved children,
/// tokens built once): 126 observed — the 150 bound holds ~19% headroom
/// for cross-toolchain variation in std containers.
#[test]
fn allocations_per_event_bounded() {
    let rule = multiway::rule::parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = multiway::rule::parse_state("{{0,0},{0,0}}").unwrap();

    // warm-up: touch every code path once so one-time setup (parser
    // interning, lazy statics) doesn't pollute the measured window
    let _ = multiway::system::evolve(&rule, init.clone(), 2);

    let before = ALLOCS.load(Ordering::Relaxed);
    let mw = multiway::system::evolve(&rule, init, 4);
    let after = ALLOCS.load(Ordering::Relaxed);

    let events = mw.events.len();
    assert_eq!(events, 496, "precondition drifted");
    let per_event = (after - before) / events;
    eprintln!("allocations per event: {}", per_event);
    assert!(
        per_event <= 150,
        "allocation regression: {} allocs/event exceeds the 150 budget",
        per_event
    );
}
