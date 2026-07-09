use multiway::causal;
use multiway::causal::UpdateOrder;
use multiway::confluence::{check, CheckCfg};
use multiway::export::bundle_json;
use multiway::lint::lint;
use multiway::report;
use multiway::rule::{parse_rule, parse_state, Rule};
use multiway::system::{evolve_opts, EvolveOpts};
use std::process::exit;

const TEMPLATE: &str = include_str!("../viewer/template.html");

const USAGE: &str = r#"multiway — a multiway hypergraph rewriting engine with e-graph-style state sharing

USAGE:
  multiway --rule "<rule>" --init "<state>" --steps N [options]
  multiway --check-confluence --rule "<rule>" [--rule "<rule>" ...] [bounds]
  multiway --lint --rule "<rule>" [--rule "<rule>" ...]

OPTIONS:
  --rule "<rule>"    rewrite rule, e.g. "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}"
                     (repeatable in --check-confluence / --lint modes;
                      evolution takes exactly one)
  --init "<state>"   initial state, e.g. "{{0,0},{0,0}}"
  --steps N          multiway evolution depth
  --causal N         also run a single-path evolution for N events (causal graph)
  --json PATH        write the data bundle as JSON
  --html PATH        write a self-contained interactive viewer
  --quiet            suppress the stats table
  --threads N        parallel evolve workers (default 1; output is
                     byte-identical for every N)
  --order MODE       causal updating order: sequential (default) or
                     standard (maximal disjoint generations)

  --check-confluence run critical-pair analysis instead of evolving
  --join-depth N     joinability search depth per side (default 8)
  --max-states N     joinability state budget per side (default 2000)
  --lint             print static rule analysis (conservation, termination)

EXAMPLES:
  multiway --rule "{{x,y}}->{{x,y},{y,z}}" --init "{{0,0}}" --steps 6 --html demo.html
  multiway --rule "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}" --init "{{0,0},{0,0}}" \
           --steps 4 --causal 40 --html demo.html
  multiway --check-confluence --rule "{{x,y}}->{{x}}" --rule "{{x,y}}->{{y}}"
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut rule_texts: Vec<String> = Vec::new();
    let mut init_s: Option<String> = None;
    let mut steps: usize = 0;
    let mut causal_events: usize = 0;
    let mut json_path: Option<String> = None;
    let mut html_path: Option<String> = None;
    let mut quiet = false;
    let mut mode_confluence = false;
    let mut mode_lint = false;
    let mut check_cfg = CheckCfg::default();
    let mut threads: usize = 1;
    let mut order = UpdateOrder::Sequential;

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
                rule_texts.push(need(i));
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
            "--threads" => {
                threads = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--threads must be an integer");
                    exit(2);
                });
                if threads == 0 {
                    eprintln!("--threads must be >= 1");
                    exit(2);
                }
                i += 2;
            }
            "--order" => {
                order = match need(i).as_str() {
                    "sequential" => UpdateOrder::Sequential,
                    "standard" => UpdateOrder::StandardGenerations,
                    other => {
                        eprintln!("unknown --order {:?} (sequential | standard)", other);
                        exit(2);
                    }
                };
                i += 2;
            }
            "--check-confluence" => {
                mode_confluence = true;
                i += 1;
            }
            "--lint" => {
                mode_lint = true;
                i += 1;
            }
            "--join-depth" => {
                check_cfg.join_depth = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--join-depth must be an integer");
                    exit(2);
                });
                i += 2;
            }
            "--max-states" => {
                check_cfg.max_states = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--max-states must be an integer");
                    exit(2);
                });
                i += 2;
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

    // analysis modes take one or more rules and no initial state
    if mode_lint || mode_confluence {
        if rule_texts.is_empty() {
            eprintln!("analysis modes need at least one --rule\n\n{}", USAGE);
            exit(2);
        }
        let rules: Vec<Rule> = rule_texts
            .iter()
            .map(|t| {
                parse_rule(t).unwrap_or_else(|e| {
                    eprintln!("rule parse error: {}", e);
                    exit(2);
                })
            })
            .collect();
        if mode_lint {
            for rule in &rules {
                print!("{}", lint(rule).render(rule));
            }
        }
        if mode_confluence {
            match check(&rules, &check_cfg) {
                Ok(report) => print!("{}", report.render(&rules)),
                Err(e) => {
                    eprintln!("confluence check error: {}", e);
                    exit(1);
                }
            }
        }
        return;
    }

    let rule_s = match (rule_texts.len(), init_s.as_ref()) {
        (1, Some(_)) => rule_texts.remove(0),
        (n, _) if n > 1 => {
            eprintln!(
                "evolution takes exactly one --rule (got {})\n\n{}",
                n, USAGE
            );
            exit(2);
        }
        _ => {
            eprintln!("{}", USAGE);
            exit(2);
        }
    };
    let init_s = init_s.unwrap();
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

    let mw = evolve_opts(
        &rule,
        init.clone(),
        &EvolveOpts {
            steps,
            threads,
            incremental: true,
        },
    );
    let causal_run = if causal_events > 0 {
        Some(causal::run_ordered(&rule, init, causal_events, order))
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
