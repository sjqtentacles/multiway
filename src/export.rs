//! JSON bundling for the HTML viewer. Zero dependencies — the data is
//! numbers and brace-notation strings, so we build JSON by hand (with
//! string escaping for safety).

use crate::causal::CausalRun;
use crate::hypergraph::State;
use crate::system::MultiwaySystem;

/// JSON string escaping, hardened for the `<script>`-embedding context:
/// `<` becomes `<` so a hostile string can never smuggle a literal
/// `</script>` into the baked viewer (an HTML entity would corrupt the
/// JSON; the `\u` escape is legal in both JSON and JS), and U+2028/U+2029
/// are escaped because they are legal JSON but illegal in JS string
/// literals under pre-ES2019 parsers. Rule/state notation can't currently
/// produce any of these — this is defense in depth for future fields.
pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// A state's edge list as a JSON array of arrays.
pub fn edges_json(state: &State) -> String {
    let inner: Vec<String> = state
        .edges
        .iter()
        .map(|e| {
            let vs: Vec<String> = e.iter().map(|v| v.to_string()).collect();
            format!("[{}]", vs.join(","))
        })
        .collect();
    format!("[{}]", inner.join(","))
}

/// The multiway section: states, events, branchial pairs, layers, sharing.
pub fn multiway_json(mw: &MultiwaySystem) -> String {
    let pc = mw.path_counts();
    let states: Vec<String> = mw
        .states
        .iter()
        .map(|s| {
            format!(
                "{{\"id\":{},\"step\":{},\"edges\":{},\"paths\":\"{}\"}}",
                s.id,
                s.step,
                edges_json(&s.state),
                pc[s.id]
            )
        })
        .collect();
    let events: Vec<String> = mw
        .events
        .iter()
        .map(|e| {
            format!(
                "{{\"from\":{},\"to\":{},\"step\":{}}}",
                e.from, e.to, e.step
            )
        })
        .collect();
    let branchial: Vec<String> = mw
        .branchial()
        .iter()
        .map(|(a, b)| format!("[{},{}]", a, b))
        .collect();
    let layers: Vec<String> = mw
        .layers
        .iter()
        .map(|l| {
            let ids: Vec<String> = l.iter().map(|i| i.to_string()).collect();
            format!("[{}]", ids.join(","))
        })
        .collect();
    let sharing: Vec<String> = mw
        .sharing_per_layer()
        .iter()
        .map(|(step, paths, canon)| {
            format!(
                "{{\"step\":{},\"treeNodes\":\"{}\",\"canonical\":{}}}",
                step, paths, canon
            )
        })
        .collect();
    format!(
        "{{\"states\":[{}],\"events\":[{}],\"branchial\":[{}],\"layers\":[{}],\"sharing\":[{}],\"backMerges\":{}}}",
        states.join(","),
        events.join(","),
        branchial.join(","),
        layers.join(","),
        sharing.join(","),
        mw.back_merges
    )
}

/// The single-path causal section: event count, deps, final state.
pub fn causal_json(c: &CausalRun) -> String {
    let deps: Vec<String> = c
        .deps
        .iter()
        .map(|(a, b)| format!("[{},{}]", a, b))
        .collect();
    format!(
        "{{\"nEvents\":{},\"deps\":[{}],\"finalEdges\":{}}}",
        c.n_events,
        deps.join(","),
        edges_json(&c.final_state)
    )
}

/// Token-event graph section: multiway-wide causal edges and event-level
/// branchial pairs. Creator sets are deliberately NOT exported — fixed
/// schema, byte-stable, and the viewer only draws the two graphs.
pub fn teg_json(t: &crate::teg::TokenEventGraph) -> String {
    let causal: Vec<String> = t
        .causal
        .iter()
        .map(|(a, b)| format!("[{},{}]", a, b))
        .collect();
    let branchial: Vec<String> = t
        .branchial_events
        .iter()
        .map(|(a, b)| format!("[{},{}]", a, b))
        .collect();
    format!(
        "{{\"causal\":[{}],\"branchialEvents\":[{}]}}",
        causal.join(","),
        branchial.join(",")
    )
}

/// The complete data bundle the CLI writes and the viewer consumes.
pub fn bundle_json(
    rule_text: &str,
    init_text: &str,
    mw: &MultiwaySystem,
    causal: Option<&CausalRun>,
) -> String {
    let causal_part = match causal {
        Some(c) => causal_json(c),
        None => "null".to_string(),
    };
    let teg = crate::teg::build(mw);
    format!(
        "{{\"rule\":\"{}\",\"init\":\"{}\",\"multiway\":{},\"teg\":{},\"causal\":{}}}",
        esc(rule_text),
        esc(init_text),
        multiway_json(mw),
        teg_json(&teg),
        causal_part
    )
}
