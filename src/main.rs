use multiway::causal;
use multiway::export::bundle_json;
use multiway::report;
use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;
use std::process::exit;

const TEMPLATE: &str = include_str!("../viewer/template.html");

const USAGE: &str = r#"multiway — a multiway hypergraph rewriting engine with e-graph-style state sharing

USAGE:
  multiway --rule "<rule>" --init "<state>" --steps N [options]

OPTIONS:
  --rule "<rule>"    rewrite rule, e.g. "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}"
  --init "<state>"   initial state, e.g. "{{0,0},{0,0}}"
  --steps N          multiway evolution depth
  --causal N         also run a single-path evolution for N events (causal graph)
  --json PATH        write the data bundle as JSON
  --html PATH        write a self-contained interactive viewer
  --quiet            suppress the stats table

EXAMPLES:
  multiway --rule "{{x,y}}->{{x,y},{y,z}}" --init "{{0,0}}" --steps 6 --html demo.html
  multiway --rule "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}" --init "{{0,0},{0,0}}" \
           --steps 4 --causal 40 --html demo.html
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut rule_s: Option<String> = None;
    let mut init_s: Option<String> = None;
    let mut steps: usize = 0;
    let mut causal_events: usize = 0;
    let mut json_path: Option<String> = None;
    let mut html_path: Option<String> = None;
    let mut quiet = false;

    let mut i = 0;
    while i < args.len() {
        let need = |i: usize| -> String {
            args.get(i + 1)
                .unwrap_or_else(|| {
                    eprintln!("missing value for {}\n\n{}", args[i], USAGE);
                    exit(2);
                })
                .clone()
        };
        match args[i].as_str() {
            "--rule" => {
                rule_s = Some(need(i));
                i += 2;
            }
            "--init" => {
                init_s = Some(need(i));
                i += 2;
            }
            "--steps" => {
                steps = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--steps must be an integer");
                    exit(2);
                });
                i += 2;
            }
            "--causal" => {
                causal_events = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--causal must be an integer");
                    exit(2);
                });
                i += 2;
            }
            "--json" => {
                json_path = Some(need(i));
                i += 2;
            }
            "--html" => {
                html_path = Some(need(i));
                i += 2;
            }
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            "--help" | "-h" => {
                println!("{}", USAGE);
                return;
            }
            other => {
                eprintln!("unknown argument {:?}\n\n{}", other, USAGE);
                exit(2);
            }
        }
    }

    let (rule_s, init_s) = match (rule_s, init_s) {
        (Some(r), Some(s)) => (r, s),
        _ => {
            eprintln!("{}", USAGE);
            exit(2);
        }
    };
    if steps == 0 && causal_events == 0 {
        eprintln!("nothing to do: set --steps and/or --causal\n\n{}", USAGE);
        exit(2);
    }

    let rule = parse_rule(&rule_s).unwrap_or_else(|e| {
        eprintln!("rule parse error: {}", e);
        exit(2);
    });
    let init = parse_state(&init_s).unwrap_or_else(|e| {
        eprintln!("state parse error: {}", e);
        exit(2);
    });

    let mw = evolve(&rule, init.clone(), steps);
    let causal_run = if causal_events > 0 {
        Some(causal::run(&rule, init, causal_events))
    } else {
        None
    };

    if !quiet {
        print!(
            "{}",
            report::stats_text(&rule.text, init_s.trim(), &mw, causal_run.as_ref())
        );
    }

    let json = bundle_json(&rule.text, init_s.trim(), &mw, causal_run.as_ref());
    if let Some(p) = json_path {
        std::fs::write(&p, &json).unwrap_or_else(|e| {
            eprintln!("cannot write {}: {}", p, e);
            exit(1);
        });
        if !quiet {
            println!("wrote {}", p);
        }
    }
    if let Some(p) = html_path {
        let html = TEMPLATE.replace("__DATA_JSON__", &json);
        std::fs::write(&p, html).unwrap_or_else(|e| {
            eprintln!("cannot write {}: {}", p, e);
            exit(1);
        });
        if !quiet {
            println!("wrote {}", p);
        }
    }
}
