use multiway::atlas::showcase_seed;
use multiway::causal;
use multiway::causal::UpdateOrder;
use multiway::confluence::{check, CheckCfg};
use multiway::export::bundle_json;
use multiway::lint::lint;
use multiway::probe::{seeds_for, ProbeBudget};
use multiway::report;
use multiway::rule::{parse_rule, parse_state, Rule};
use multiway::rulespace::{space_size, SpaceBudget};
use multiway::scan::{scan, scan_json, scan_text, ScanOpts};
use multiway::stats::group_digits;
use multiway::system::{evolve_opts, EvolveOpts};
use std::process::exit;

const TEMPLATE: &str = include_str!("../viewer/template.html");
const ATLAS_TEMPLATE: &str = include_str!("../viewer/atlas.html");

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

SCAN MODE (rule-space pattern discovery):
  --scan             probe every rule class in a space, rank by
                     interestingness, print an atlas (same flags =>
                     byte-identical output for any --threads)
  --max-lhs N        LHS edges per rule (default 2)
  --max-rhs N        RHS edges per rule (default 3)
  --min-arity N      minimum edge arity (default 1)
  --max-arity N      maximum edge arity (default 2)
  --max-vars N       variable universe size, <= 6 (default 4)
  --count            print the exact class count (Burnside) and exit
  --sample N         probe a deterministic sample instead of the full
                     space (required beyond 200,000 classes)
  --seed 0xHEX       sample seed (default 0xC0FFEE)
  --top N            atlas rows to keep (default 25)
  --budget-states N  probe state budget per seed (default 500)
  --budget-edges N   probe edges-per-state budget (default 64)
  --steps N          probe multiway depth (default 5)
  --atlas DIR        write DIR/index.html + a baked viewer per rule
  --scan-json PATH   write the full scan result as JSON

EXAMPLES:
  multiway --rule "{{x,y}}->{{x,y},{y,z}}" --init "{{0,0}}" --steps 6 --html demo.html
  multiway --rule "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}" --init "{{0,0},{0,0}}" \
           --steps 4 --causal 40 --html demo.html
  multiway --check-confluence --rule "{{x,y}}->{{x}}" --rule "{{x,y}}->{{y}}"
  multiway --scan --top 15 --atlas atlas/
  multiway --scan --max-arity 3 --sample 5000 --seed 0xBEEF --top 25
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
    let mut mode_scan = false;
    let mut count_only = false;
    let mut space = SpaceBudget {
        max_lhs: 2,
        max_rhs: 3,
        min_arity: 1,
        max_arity: 2,
        max_vars: 4,
    };
    let mut probe_budget = ProbeBudget::default();
    let mut sample_n: Option<usize> = None;
    let mut sample_seed: u64 = 0xC0FFEE;
    let mut top: usize = 25;
    let mut atlas_dir: Option<String> = None;
    let mut scan_json_path: Option<String> = None;
    let mut steps_set = false;

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
                steps_set = true;
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
            "--scan" => {
                mode_scan = true;
                i += 1;
            }
            "--count" => {
                count_only = true;
                i += 1;
            }
            "--max-lhs" | "--max-rhs" | "--min-arity" | "--max-arity" | "--max-vars" => {
                let flag = args[i].clone();
                let v: usize = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("{} must be an integer", flag);
                    exit(2);
                });
                match flag.as_str() {
                    "--max-lhs" => space.max_lhs = v,
                    "--max-rhs" => space.max_rhs = v,
                    "--min-arity" => space.min_arity = v,
                    "--max-arity" => space.max_arity = v,
                    _ => space.max_vars = v,
                }
                i += 2;
            }
            "--sample" => {
                sample_n = Some(need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--sample must be an integer");
                    exit(2);
                }));
                i += 2;
            }
            "--seed" => {
                let raw = need(i);
                let parsed = match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
                    Some(hex) => u64::from_str_radix(hex, 16),
                    None => raw.parse(),
                };
                sample_seed = parsed.unwrap_or_else(|_| {
                    eprintln!("--seed must be an integer (decimal or 0xHEX)");
                    exit(2);
                });
                i += 2;
            }
            "--top" => {
                top = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--top must be an integer");
                    exit(2);
                });
                i += 2;
            }
            "--budget-states" => {
                probe_budget.max_states = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--budget-states must be an integer");
                    exit(2);
                });
                i += 2;
            }
            "--budget-edges" => {
                probe_budget.max_edges = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--budget-edges must be an integer");
                    exit(2);
                });
                i += 2;
            }
            "--atlas" => {
                atlas_dir = Some(need(i));
                i += 2;
            }
            "--scan-json" => {
                scan_json_path = Some(need(i));
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

    if mode_scan {
        if steps_set {
            probe_budget.steps = steps;
        }
        if count_only {
            println!("{} rule classes", group_digits(space_size(&space)));
            return;
        }
        let opts = ScanOpts {
            space,
            probe: probe_budget,
            sample: sample_n.map(|n| (n, sample_seed)),
            top,
            threads,
        };
        let entries = scan(&opts).unwrap_or_else(|e| {
            eprintln!("{}", e);
            exit(2);
        });
        if !quiet {
            print!("{}", scan_text(&opts, &entries));
        }
        let json = scan_json(&opts, &entries);
        if let Some(p) = scan_json_path {
            std::fs::write(&p, &json).unwrap_or_else(|e| {
                eprintln!("cannot write {}: {}", p, e);
                exit(1);
            });
            if !quiet {
                println!("wrote {}", p);
            }
        }
        if let Some(dir) = atlas_dir {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| {
                eprintln!("cannot create {}: {}", dir.display(), e);
                exit(1);
            });
            let index = ATLAS_TEMPLATE.replace("__ATLAS_JSON__", &json);
            std::fs::write(dir.join("index.html"), index).unwrap_or_else(|e| {
                eprintln!("cannot write index.html: {}", e);
                exit(1);
            });
            for (i, e) in entries.iter().enumerate() {
                // bake a full viewer for the showcase seed. The evolve
                // is bounded by construction: it reruns exactly the
                // layers the budget-capped probe already completed
                // (probe layers == evolve layers — the differential
                // prop), never the aborted partial layer.
                let rule = e.rule.to_rule();
                let si = showcase_seed(&e.probe);
                let init = seeds_for(&rule).swap_remove(si);
                let bake_steps = e.probe.seeds[si].layers.len().saturating_sub(1);
                let mw = evolve_opts(
                    &rule,
                    init.clone(),
                    &EvolveOpts {
                        steps: bake_steps,
                        threads: 1,
                        incremental: true,
                    },
                );
                let bundle = bundle_json(&rule.text, &init.to_notation(), &mw, None);
                let page = TEMPLATE.replace("__DATA_JSON__", &bundle);
                let name = format!("rule-{:03}.html", i + 1);
                std::fs::write(dir.join(&name), page).unwrap_or_else(|err| {
                    eprintln!("cannot write {}: {}", name, err);
                    exit(1);
                });
            }
            if !quiet {
                println!(
                    "wrote {}/index.html + {} rule pages",
                    dir.display(),
                    entries.len()
                );
            }
        }
        return;
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
