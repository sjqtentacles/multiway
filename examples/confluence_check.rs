//! Drive the confluence checker from code: analyze a rule set and print
//! the honest verdict. Run with `cargo run --example confluence_check`.

use multiway::confluence::{check, CheckCfg, Verdict};
use multiway::lint::lint;
use multiway::rule::parse_rule;

fn main() {
    // A rule set with a genuine unjoinable divergence:
    let rules = vec![
        parse_rule("{{x,y}}->{{x}}").unwrap(),
        parse_rule("{{x,y}}->{}").unwrap(),
    ];
    for rule in &rules {
        print!("{}", lint(rule).render(rule));
    }

    let report = check(&rules, &CheckCfg::default()).unwrap();
    match &report.verdict {
        Verdict::AllCriticalPairsStronglyJoinable {
            pairs_checked,
            confluent,
            ..
        } => println!(
            "all {} critical pairs strongly joinable (confluent: {})",
            pairs_checked, confluent
        ),
        Verdict::NotConfluent { pair, s1, s2 } => println!(
            "NOT confluent: host {} diverges to {} vs {}",
            pair.host.to_notation(),
            s1.to_notation(),
            s2.to_notation()
        ),
        Verdict::Inconclusive { reason, .. } => println!("inconclusive: {:?}", reason),
    }
}
