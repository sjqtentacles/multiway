//! Parsing Wolfram-model-style notation.
//!
//! Rules: `{{x,y},{x,z}} -> {{x,z},{x,w},{y,w},{z,w}}`
//! States: `{{0,0},{0,1}}`
//!
//! In rules, every identifier is a pattern variable. Variables appearing
//! only on the right-hand side denote *fresh* vertices, minted at
//! application time. Distinct variables may bind the same vertex
//! (Wolfram-model semantics: matching is not vertex-injective), but each
//! LHS pattern edge must match a distinct edge *instance*.

use crate::hypergraph::State;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Rule {
    /// Pattern edges as sequences of variable ids.
    pub lhs: Vec<Vec<usize>>,
    /// Replacement edges as sequences of variable ids (may introduce new vars).
    pub rhs: Vec<Vec<usize>>,
    /// Total number of distinct variables (LHS vars first, then RHS-only).
    pub n_vars: usize,
    pub var_names: Vec<String>,
    pub text: String,
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn new(s: &'a str) -> Self {
        P {
            s: s.as_bytes(),
            i: 0,
        }
    }
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.i < self.s.len() && self.s[self.i] == c {
            self.i += 1;
            Ok(())
        } else {
            Err(format!(
                "expected '{}' at byte {} in {:?}",
                c as char,
                self.i,
                String::from_utf8_lossy(self.s)
            ))
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn ident(&mut self) -> Result<String, String> {
        self.ws();
        let start = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        if self.i == start {
            return Err(format!("expected identifier at byte {}", start));
        }
        Ok(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }
    fn done(&mut self) -> bool {
        self.ws();
        self.i >= self.s.len()
    }
}

/// Parse `{{a,b},{c,d},...}` into a list of identifier lists.
fn parse_edge_lists(s: &str) -> Result<Vec<Vec<String>>, String> {
    let mut p = P::new(s);
    let mut out = Vec::new();
    p.eat(b'{')?;
    if p.peek() == Some(b'}') {
        p.eat(b'}')?;
    } else {
        loop {
            p.eat(b'{')?;
            let mut edge = Vec::new();
            if p.peek() != Some(b'}') {
                loop {
                    edge.push(p.ident()?);
                    if p.peek() == Some(b',') {
                        p.eat(b',')?;
                    } else {
                        break;
                    }
                }
            }
            p.eat(b'}')?;
            out.push(edge);
            if p.peek() == Some(b',') {
                p.eat(b',')?;
            } else {
                break;
            }
        }
        p.eat(b'}')?;
    }
    if !p.done() {
        return Err(format!("trailing input after edge list in {:?}", s));
    }
    Ok(out)
}

pub fn parse_rule(s: &str) -> Result<Rule, String> {
    let arrow = s
        .find("->")
        .ok_or_else(|| format!("rule must contain '->': {:?}", s))?;
    let (l, r) = (&s[..arrow], &s[arrow + 2..]);
    let lhs_names = parse_edge_lists(l.trim())?;
    let rhs_names = parse_edge_lists(r.trim())?;
    if lhs_names.is_empty() {
        return Err("rule left-hand side must contain at least one edge".into());
    }

    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut var_names: Vec<String> = Vec::new();
    let intern = |name: String, ids: &mut HashMap<String, usize>, var_names: &mut Vec<String>| {
        *ids.entry(name.clone()).or_insert_with(|| {
            var_names.push(name);
            var_names.len() - 1
        })
    };

    let lhs: Vec<Vec<usize>> = lhs_names
        .into_iter()
        .map(|e| {
            e.into_iter()
                .map(|n| intern(n, &mut ids, &mut var_names))
                .collect()
        })
        .collect();
    let rhs: Vec<Vec<usize>> = rhs_names
        .into_iter()
        .map(|e| {
            e.into_iter()
                .map(|n| intern(n, &mut ids, &mut var_names))
                .collect()
        })
        .collect();

    Ok(Rule {
        lhs,
        rhs,
        n_vars: var_names.len(),
        var_names,
        text: s.trim().to_string(),
    })
}

pub fn parse_state(s: &str) -> Result<State, String> {
    let lists = parse_edge_lists(s.trim())?;
    let mut edges = Vec::with_capacity(lists.len());
    for e in lists {
        let mut edge = Vec::with_capacity(e.len());
        for ident in e {
            let v: u32 = ident.parse().map_err(|_| {
                format!(
                    "state vertices must be non-negative integers, got {:?}",
                    ident
                )
            })?;
            edge.push(v);
        }
        edges.push(edge);
    }
    Ok(State::new(edges))
}
