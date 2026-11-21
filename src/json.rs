//! A small JSON reader and writer (std only): the scene comes in as JSON, the plan goes out as JSON.

#[derive(Clone, Debug)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

static NULL: J = J::Null;

impl J {
    pub fn get(&self, k: &str) -> &J {
        if let J::Obj(v) = self {
            for (kk, vv) in v {
                if kk == k {
                    return vv;
                }
            }
        }
        &NULL
    }
    pub fn has(&self, k: &str) -> bool {
        !matches!(self.get(k), J::Null)
    }
    pub fn f(&self) -> Option<f64> {
        if let J::Num(n) = self {
            Some(*n)
        } else {
            None
        }
    }
    pub fn f_or(&self, d: f64) -> f64 {
        self.f().unwrap_or(d)
    }
    pub fn s(&self) -> Option<&str> {
        if let J::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn b(&self) -> bool {
        matches!(self, J::Bool(true))
    }
    pub fn arr(&self) -> &[J] {
        if let J::Arr(v) = self {
            v
        } else {
            &[]
        }
    }
    pub fn nums(&self) -> Vec<f64> {
        self.arr().iter().filter_map(|x| x.f()).collect()
    }
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
    depth: u32,
}

/// Arrays and objects may nest this deep (the reader recurses; a file nested deeper is not a scene).
const MAX_DEPTH: u32 = 64;

pub fn parse(s: &str) -> Result<J, String> {
    let mut p = P { b: s.as_bytes(), i: 0, depth: 0 };
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("trailing data at byte {}", p.i));
    }
    Ok(v)
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn err<T>(&self, m: &str) -> Result<T, String> {
        Err(format!("{} at byte {}", m, self.i))
    }
    fn value(&mut self) -> Result<J, String> {
        if self.depth >= MAX_DEPTH {
            return self.err("nested too deep");
        }
        self.depth += 1;
        let v = self.value_in();
        self.depth -= 1;
        v
    }
    fn value_in(&mut self) -> Result<J, String> {
        self.ws();
        if self.i >= self.b.len() {
            return self.err("unexpected end");
        }
        match self.b[self.i] {
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.i < self.b.len() && self.b[self.i] == b'}' {
                    self.i += 1;
                    return Ok(J::Obj(v));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    if self.i >= self.b.len() || self.b[self.i] != b':' {
                        return self.err("expected ':'");
                    }
                    self.i += 1;
                    let val = self.value()?;
                    v.push((k, val));
                    self.ws();
                    if self.i >= self.b.len() {
                        return self.err("unexpected end in object");
                    }
                    match self.b[self.i] {
                        b',' => self.i += 1,
                        b'}' => {
                            self.i += 1;
                            return Ok(J::Obj(v));
                        }
                        _ => return self.err("expected ',' or '}'"),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.i < self.b.len() && self.b[self.i] == b']' {
                    self.i += 1;
                    return Ok(J::Arr(v));
                }
                loop {
                    v.push(self.value()?);
                    self.ws();
                    if self.i >= self.b.len() {
                        return self.err("unexpected end in array");
                    }
                    match self.b[self.i] {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            return Ok(J::Arr(v));
                        }
                        _ => return self.err("expected ',' or ']'"),
                    }
                }
            }
            b'"' => Ok(J::Str(self.string()?)),
            b't' => self.lit("true", J::Bool(true)),
            b'f' => self.lit("false", J::Bool(false)),
            b'n' => self.lit("null", J::Null),
            _ => {
                let s = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.b[s..self.i]).unwrap_or("");
                match t.parse::<f64>() {
                    Ok(n) if n.is_finite() => Ok(J::Num(n)),
                    Ok(_) => self.err("number out of range"),
                    Err(_) => self.err("bad number"),
                }
            }
        }
    }
    fn lit(&mut self, w: &str, v: J) -> Result<J, String> {
        if self.b[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            self.err("bad literal")
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        if self.i + 4 > self.b.len() {
            return self.err("short \\u escape");
        }
        let t = std::str::from_utf8(&self.b[self.i..self.i + 4]).unwrap_or("");
        self.i += 4;
        u32::from_str_radix(t, 16).map_err(|_| format!("bad \\u escape at byte {}", self.i))
    }
    fn string(&mut self) -> Result<String, String> {
        if self.i >= self.b.len() || self.b[self.i] != b'"' {
            return self.err("expected string");
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            if self.i >= self.b.len() {
                return self.err("unterminated string");
            }
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    if self.i >= self.b.len() {
                        return self.err("unterminated escape");
                    }
                    let e = self.b[self.i];
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.b[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return self.err("bad escape"),
                    }
                }
                _ => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| "string is not UTF-8".to_string())
    }
}

/// A value as JSON text (numbers in full, so reading it back gives the same value).
pub fn write(j: &J) -> String {
    fn go(j: &J, o: &mut String) {
        match j {
            J::Null => o.push_str("null"),
            J::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
            J::Num(n) => o.push_str(&format!("{}", n)),
            J::Str(s) => o.push_str(&quote(s)),
            J::Arr(v) => {
                o.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        o.push_str(", ");
                    }
                    go(x, o);
                }
                o.push(']');
            }
            J::Obj(v) => {
                o.push('{');
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        o.push_str(", ");
                    }
                    o.push_str(&quote(k));
                    o.push_str(": ");
                    go(x, o);
                }
                o.push('}');
            }
        }
    }
    let mut o = String::new();
    go(j, &mut o);
    o
}

/// A JSON string literal for `s`, quotes included.
pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// A coordinate in millimetres, short: six decimals, trailing zeros dropped.
pub fn num(v: f64) -> String {
    let mut s = format!("{:.6}", v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".to_string();
    }
    s
}
