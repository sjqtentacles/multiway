//! Recursive-descent JSON well-formedness checker, written independently
//! against RFC 8259's grammar — deliberately sharing zero code with the
//! handwritten emitter in `export.rs`, so the two can't be wrong in the
//! same way.

pub fn check_json(s: &str) -> Result<(), String> {
    let b = s.as_bytes();
    let mut i = 0usize;
    skip_ws(b, &mut i);
    value(b, &mut i)?;
    skip_ws(b, &mut i);
    if i != b.len() {
        return Err(format!("trailing input at byte {}", i));
    }
    Ok(())
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], b' ' | b'\t' | b'\n' | b'\r') {
        *i += 1;
    }
}

fn value(b: &[u8], i: &mut usize) -> Result<(), String> {
    skip_ws(b, i);
    match b.get(*i) {
        Some(b'{') => object(b, i),
        Some(b'[') => array(b, i),
        Some(b'"') => string(b, i),
        Some(b't') => literal(b, i, b"true"),
        Some(b'f') => literal(b, i, b"false"),
        Some(b'n') => literal(b, i, b"null"),
        Some(c) if *c == b'-' || c.is_ascii_digit() => number(b, i),
        Some(c) => Err(format!("unexpected byte {:?} at {}", *c as char, i)),
        None => Err("unexpected end of input".into()),
    }
}

fn literal(b: &[u8], i: &mut usize, lit: &[u8]) -> Result<(), String> {
    if b.len() - *i >= lit.len() && &b[*i..*i + lit.len()] == lit {
        *i += lit.len();
        Ok(())
    } else {
        Err(format!("bad literal at byte {}", i))
    }
}

fn object(b: &[u8], i: &mut usize) -> Result<(), String> {
    *i += 1; // '{'
    skip_ws(b, i);
    if b.get(*i) == Some(&b'}') {
        *i += 1;
        return Ok(());
    }
    loop {
        skip_ws(b, i);
        if b.get(*i) != Some(&b'"') {
            return Err(format!("expected object key at byte {}", i));
        }
        string(b, i)?;
        skip_ws(b, i);
        if b.get(*i) != Some(&b':') {
            return Err(format!("expected ':' at byte {}", i));
        }
        *i += 1;
        value(b, i)?;
        skip_ws(b, i);
        match b.get(*i) {
            Some(b',') => *i += 1,
            Some(b'}') => {
                *i += 1;
                return Ok(());
            }
            _ => return Err(format!("expected ',' or '}}' at byte {}", i)),
        }
    }
}

fn array(b: &[u8], i: &mut usize) -> Result<(), String> {
    *i += 1; // '['
    skip_ws(b, i);
    if b.get(*i) == Some(&b']') {
        *i += 1;
        return Ok(());
    }
    loop {
        value(b, i)?;
        skip_ws(b, i);
        match b.get(*i) {
            Some(b',') => *i += 1,
            Some(b']') => {
                *i += 1;
                return Ok(());
            }
            _ => return Err(format!("expected ',' or ']' at byte {}", i)),
        }
    }
}

fn string(b: &[u8], i: &mut usize) -> Result<(), String> {
    *i += 1; // '"'
    loop {
        match b.get(*i) {
            None => return Err("unterminated string".into()),
            Some(b'"') => {
                *i += 1;
                return Ok(());
            }
            Some(b'\\') => {
                *i += 1;
                match b.get(*i) {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => *i += 1,
                    Some(b'u') => {
                        *i += 1;
                        for _ in 0..4 {
                            match b.get(*i) {
                                Some(c) if c.is_ascii_hexdigit() => *i += 1,
                                _ => return Err(format!("bad \\u escape at byte {}", i)),
                            }
                        }
                    }
                    _ => return Err(format!("bad escape at byte {}", i)),
                }
            }
            Some(c) if *c < 0x20 => {
                return Err(format!("raw control byte {:#04x} in string at {}", c, i))
            }
            Some(_) => *i += 1, // includes multi-byte UTF-8 continuation
        }
    }
}

fn number(b: &[u8], i: &mut usize) -> Result<(), String> {
    if b.get(*i) == Some(&b'-') {
        *i += 1;
    }
    match b.get(*i) {
        Some(b'0') => *i += 1,
        Some(c) if c.is_ascii_digit() => {
            while matches!(b.get(*i), Some(c) if c.is_ascii_digit()) {
                *i += 1;
            }
        }
        _ => return Err(format!("bad number at byte {}", i)),
    }
    if b.get(*i) == Some(&b'.') {
        *i += 1;
        if !matches!(b.get(*i), Some(c) if c.is_ascii_digit()) {
            return Err(format!("bad fraction at byte {}", i));
        }
        while matches!(b.get(*i), Some(c) if c.is_ascii_digit()) {
            *i += 1;
        }
    }
    if matches!(b.get(*i), Some(b'e' | b'E')) {
        *i += 1;
        if matches!(b.get(*i), Some(b'+' | b'-')) {
            *i += 1;
        }
        if !matches!(b.get(*i), Some(c) if c.is_ascii_digit()) {
            return Err(format!("bad exponent at byte {}", i));
        }
        while matches!(b.get(*i), Some(c) if c.is_ascii_digit()) {
            *i += 1;
        }
    }
    Ok(())
}
