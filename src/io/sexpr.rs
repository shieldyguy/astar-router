//! S-expressions, lossless: a KiCad file as a tree over its own bytes. Nothing of the file is thrown
//! away: a node is a span of the text, the space between two nodes is the text between their spans, so
//! a file written back with no edit is the file read, byte for byte, and an edit changes its own span
//! and nothing else.

use std::borrow::Cow;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// `( ... )`
    List,
    /// a bare word: a number, a name, `yes`
    Atom,
    /// `"..."`, with KiCad's backslash escapes
    Str,
}

/// A node: its span in the text and where its subtree ends in `Doc::nodes` (nodes are in file order,
/// a list before what it holds).
#[derive(Clone, Copy)]
struct Node {
    start: u32,
    end: u32,
    next: u32,
    kind: Kind,
}

/// Lists may nest this deep (a KiCad board nests about ten deep).
const MAX_DEPTH: usize = 500;

/// A node of a `Doc`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Id(u32);

pub struct Doc {
    text: String,
    nodes: Vec<Node>,
    /// the top-level nodes (a KiCad file has one)
    tops: Vec<Id>,
}

/// The children of a list, in order.
pub struct Kids<'a> {
    doc: &'a Doc,
    at: u32,
    end: u32,
}

impl<'a> Iterator for Kids<'a> {
    type Item = Id;
    fn next(&mut self) -> Option<Id> {
        if self.at >= self.end {
            return None;
        }
        let id = Id(self.at);
        self.at = self.doc.nodes[self.at as usize].next;
        Some(id)
    }
}

impl Doc {
    /// The text as a tree. An error says where: a bracket that is never closed, one closed too often,
    /// a string that never ends.
    pub fn parse(text: String) -> Result<Doc, String> {
        if text.len() >= u32::MAX as usize {
            return Err("the file is 4 GB or more".into());
        }
        let b = text.as_bytes();
        let mut nodes: Vec<Node> = Vec::with_capacity(b.len() / 12);
        let mut open: Vec<u32> = Vec::new();
        let mut tops = Vec::new();
        let mut i = 0usize;
        let where_ = |i: usize| {
            let line = b[..i].iter().filter(|&&c| c == b'\n').count() + 1;
            format!("line {}", line)
        };
        while i < b.len() {
            let c = b[i];
            match c {
                b' ' | b'\t' | b'\n' | b'\r' => i += 1,
                b'(' => {
                    if open.is_empty() {
                        tops.push(Id(nodes.len() as u32));
                    }
                    if open.len() >= MAX_DEPTH {
                        return Err(format!("{}: lists nested more than {} deep", where_(i), MAX_DEPTH));
                    }
                    open.push(nodes.len() as u32);
                    nodes.push(Node { start: i as u32, end: 0, next: 0, kind: Kind::List });
                    i += 1;
                }
                b')' => {
                    let Some(n) = open.pop() else { return Err(format!("{}: a ')' that closes nothing", where_(i))) };
                    i += 1;
                    nodes[n as usize].end = i as u32;
                    nodes[n as usize].next = nodes.len() as u32;
                }
                _ => {
                    let start = i;
                    let kind = if c == b'"' {
                        i += 1;
                        loop {
                            match b.get(i) {
                                None => return Err(format!("{}: a string that never ends", where_(start))),
                                Some(b'\\') => i += 2,
                                Some(b'"') => break,
                                Some(_) => i += 1,
                            }
                        }
                        i += 1;
                        Kind::Str
                    } else if c == b'|' {
                        // embedded data: |base64 over many lines|
                        i += 1;
                        while i < b.len() && b[i] != b'|' {
                            i += 1;
                        }
                        if i >= b.len() {
                            return Err(format!("{}: embedded data that never ends", where_(start)));
                        }
                        i += 1;
                        Kind::Atom
                    } else {
                        while i < b.len() && !matches!(b[i], b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')' | b'"') {
                            i += 1;
                        }
                        Kind::Atom
                    };
                    if i > b.len() {
                        return Err(format!("{}: a string that never ends", where_(start)));
                    }
                    if open.is_empty() {
                        tops.push(Id(nodes.len() as u32));
                    }
                    let at = nodes.len() as u32;
                    nodes.push(Node { start: start as u32, end: i as u32, next: at + 1, kind });
                }
            }
        }
        if let Some(n) = open.last() {
            return Err(format!("{}: a '(' that is never closed", where_(nodes[*n as usize].start as usize)));
        }
        Ok(Doc { text, nodes, tops })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The file's one list (`(kicad_pcb ...)`), when it is one.
    pub fn root(&self) -> Option<Id> {
        match self.tops.as_slice() {
            [id] if self.kind(*id) == Kind::List => Some(*id),
            _ => None,
        }
    }

    /// Every top-level node (a rules file is a row of lists).
    pub fn tops(&self) -> &[Id] {
        &self.tops
    }

    pub fn kind(&self, id: Id) -> Kind {
        self.nodes[id.0 as usize].kind
    }

    /// The node's own text, as the file has it.
    pub fn raw(&self, id: Id) -> &str {
        let n = &self.nodes[id.0 as usize];
        &self.text[n.start as usize..n.end as usize]
    }

    /// Where the node is in the text: its first byte and one past its last.
    pub fn span(&self, id: Id) -> (usize, usize) {
        let n = &self.nodes[id.0 as usize];
        (n.start as usize, n.end as usize)
    }

    pub fn kids(&self, id: Id) -> Kids<'_> {
        let n = &self.nodes[id.0 as usize];
        if n.kind == Kind::List {
            Kids { doc: self, at: id.0 + 1, end: n.next }
        } else {
            Kids { doc: self, at: 0, end: 0 }
        }
    }

    /// A list's name: its first child when that is a bare word. `""` otherwise.
    pub fn head(&self, id: Id) -> &str {
        match self.kids(id).next() {
            Some(k) if self.kind(k) == Kind::Atom => self.raw(k),
            _ => "",
        }
    }

    /// The value of an atom or a string (escapes undone). `""` for a list.
    pub fn val(&self, id: Id) -> Cow<'_, str> {
        let raw = self.raw(id);
        match self.kind(id) {
            Kind::Atom => Cow::Borrowed(raw),
            Kind::List => Cow::Borrowed(""),
            Kind::Str => {
                let inner = &raw[1..raw.len() - 1];
                if !inner.contains('\\') {
                    return Cow::Borrowed(inner);
                }
                let mut out = String::with_capacity(inner.len());
                let mut it = inner.chars();
                while let Some(c) = it.next() {
                    if c != '\\' {
                        out.push(c);
                        continue;
                    }
                    match it.next() {
                        Some('n') => out.push('\n'),
                        Some('r') => out.push('\r'),
                        Some('t') => out.push('\t'),
                        Some(o) => out.push(o),
                        None => {}
                    }
                }
                Cow::Owned(out)
            }
        }
    }

    /// The list's k-th argument (the name is not counted): `(at 1 2 90)` has arguments 0, 1, 2.
    pub fn arg(&self, id: Id, k: usize) -> Option<Id> {
        self.kids(id).nth(k + 1)
    }

    /// The k-th argument's value when it is a word or a string.
    pub fn arg_str(&self, id: Id, k: usize) -> Option<Cow<'_, str>> {
        self.arg(id, k).filter(|a| self.kind(*a) != Kind::List).map(|a| self.val(a))
    }

    /// The k-th argument as a number.
    pub fn arg_num(&self, id: Id, k: usize) -> Option<f64> {
        self.arg(id, k).filter(|a| self.kind(*a) == Kind::Atom).and_then(|a| self.raw(a).parse::<f64>().ok()).filter(|v| v.is_finite())
    }

    /// The first child list of this name.
    pub fn find(&self, id: Id, name: &str) -> Option<Id> {
        self.kids(id).find(|k| self.kind(*k) == Kind::List && self.head(*k) == name)
    }

    /// Every child list of this name.
    pub fn all<'a>(&'a self, id: Id, name: &'a str) -> impl Iterator<Item = Id> + 'a {
        self.kids(id).filter(move |k| self.kind(*k) == Kind::List && self.head(*k) == name)
    }

    /// Every child that is a list.
    pub fn lists(&self, id: Id) -> impl Iterator<Item = Id> + '_ {
        self.kids(id).filter(move |k| self.kind(*k) == Kind::List)
    }

    /// Does the list hold this bare word among its arguments (`(pad "1" smd rect ... locked)`)?
    pub fn has_word(&self, id: Id, word: &str) -> bool {
        self.kids(id).skip(1).any(|k| self.kind(k) == Kind::Atom && self.raw(k) == word)
    }

    /// `(name value)`: the value of the first child list of this name.
    pub fn str_of(&self, id: Id, name: &str) -> Option<Cow<'_, str>> {
        self.find(id, name).and_then(|n| self.arg_str(n, 0))
    }

    pub fn num_of(&self, id: Id, name: &str) -> Option<f64> {
        self.find(id, name).and_then(|n| self.arg_num(n, 0))
    }

    /// Is `(name yes)` there? A bare `(name)` and `(name true)` count as yes, as KiCad reads them.
    pub fn flag(&self, id: Id, name: &str) -> bool {
        match self.find(id, name) {
            None => false,
            Some(n) => !matches!(self.arg_str(n, 0).as_deref(), Some("no") | Some("false")),
        }
    }

    /// The tree written out again from its nodes: every node's own text and the text between nodes.
    /// It is the file read, byte for byte; `tests` hold it to that on every board of the corpus.
    pub fn write(&self) -> String {
        let mut out = String::with_capacity(self.text.len());
        let mut at = 0usize;
        for t in &self.tops {
            self.emit(*t, &mut at, &mut out);
        }
        out.push_str(&self.text[at..]);
        out
    }

    fn emit(&self, id: Id, at: &mut usize, out: &mut String) {
        let n = self.nodes[id.0 as usize];
        out.push_str(&self.text[*at..n.start as usize]);
        *at = n.start as usize;
        if n.kind != Kind::List {
            out.push_str(&self.text[n.start as usize..n.end as usize]);
            *at = n.end as usize;
            return;
        }
        out.push('(');
        *at += 1;
        for k in self.kids(id) {
            self.emit(k, at, out);
        }
        out.push_str(&self.text[*at..n.end as usize]);
        *at = n.end as usize;
    }
}

/// Changes to a `Doc`'s text, each on its own span. `apply` gives the new text: what no edit names is
/// left byte for byte.
#[derive(Default)]
pub struct Edits {
    list: Vec<(usize, usize, String)>,
}

impl Edits {
    pub fn new() -> Edits {
        Edits::default()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The node's text becomes `with`.
    pub fn replace(&mut self, doc: &Doc, id: Id, with: String) {
        let (a, b) = doc.span(id);
        self.list.push((a, b, with));
    }

    /// The node goes, and with it the line it stood on alone (the space before it back to the line's
    /// start, and the line's end).
    pub fn remove(&mut self, doc: &Doc, id: Id) {
        let (mut a, mut b) = doc.span(id);
        let t = doc.text.as_bytes();
        let mut s = a;
        while s > 0 && matches!(t[s - 1], b' ' | b'\t') {
            s -= 1;
        }
        let mut e = b;
        while e < t.len() && matches!(t[e], b' ' | b'\t' | b'\r') {
            e += 1;
        }
        if (s == 0 || t[s - 1] == b'\n') && e < t.len() && t[e] == b'\n' {
            a = s;
            b = e + 1;
        }
        self.list.push((a, b, String::new()));
    }

    /// `text` goes in as the list's last children: before its closing bracket, on lines of their own
    /// when the bracket stands on a line of its own (as KiCad writes a board).
    pub fn append(&mut self, doc: &Doc, list: Id, text: &str) {
        let (_, b) = doc.span(list);
        let close = b - 1;
        let t = doc.text.as_bytes();
        let own_line = close > 0 && t[close - 1] == b'\n';
        let mut with = String::with_capacity(text.len() + 2);
        if !own_line {
            with.push('\n');
        }
        with.push_str(text);
        if !text.ends_with('\n') {
            with.push('\n');
        }
        self.list.push((close, close, with));
    }

    /// The text with every edit made. Two edits on one span of text are an error (a bug in the caller).
    pub fn apply(mut self, doc: &Doc) -> Result<String, String> {
        // edits at one place keep the order they were made in
        self.list.sort_by_key(|e| (e.0, e.1));
        let mut out = String::with_capacity(doc.text.len() + self.list.iter().map(|e| e.2.len()).sum::<usize>());
        let mut at = 0usize;
        for (a, b, with) in &self.list {
            if *a < at {
                return Err("two edits of the same text".into());
            }
            out.push_str(&doc.text[at..*a]);
            out.push_str(with);
            at = *b;
        }
        out.push_str(&doc.text[at..]);
        Ok(out)
    }
}

/// A string as KiCad writes one: in quotes, with `"`, `\` and line ends escaped.
pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            _ => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "(kicad_pcb\n\t(version 20260206)\n\t(net 1 \"A \\\"b\\\" (c)\")\n\t(segment\n\t\t(start 1 2)\n\t\t(end 3 -4.5)\n\t)\n\t(embedded (data |ab(c\n\t\tde\"f|))\n)\n";

    #[test]
    fn untouched_is_identical() {
        let d = Doc::parse(SMALL.to_string()).unwrap();
        assert_eq!(d.write(), SMALL);
        assert_eq!(Edits::new().apply(&d).unwrap(), SMALL);
    }

    #[test]
    fn reads_values() {
        let d = Doc::parse(SMALL.to_string()).unwrap();
        let root = d.root().unwrap();
        assert_eq!(d.head(root), "kicad_pcb");
        let net = d.find(root, "net").unwrap();
        assert_eq!(d.arg_num(net, 0), Some(1.0));
        assert_eq!(d.arg_str(net, 1).unwrap(), "A \"b\" (c)");
        let seg = d.find(root, "segment").unwrap();
        assert_eq!(d.arg_num(d.find(seg, "end").unwrap(), 1), Some(-4.5));
        assert_eq!(d.lists(root).count(), 4);
        assert_eq!(quote("A \"b\" \\ (c)"), "\"A \\\"b\\\" \\\\ (c)\"");
    }

    #[test]
    fn one_node_replaced_removed_added() {
        let d = Doc::parse(SMALL.to_string()).unwrap();
        let root = d.root().unwrap();
        let seg = d.find(root, "segment").unwrap();
        let mut e = Edits::new();
        e.replace(&d, d.find(seg, "end").unwrap(), "(end 9 9)".into());
        e.remove(&d, d.find(root, "net").unwrap());
        e.append(&d, root, "\t(via\n\t)");
        let out = e.apply(&d).unwrap();
        assert_eq!(out, "(kicad_pcb\n\t(version 20260206)\n\t(segment\n\t\t(start 1 2)\n\t\t(end 9 9)\n\t)\n\t(embedded (data |ab(c\n\t\tde\"f|))\n\t(via\n\t)\n)\n");
        // and what was written reads again
        assert!(Doc::parse(out).is_ok());
    }

    /// Every board of the corpus, and the rule files beside them, read and written back: identical.
    #[test]
    fn the_corpus_round_trips() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/boards");
        let (mut boards, mut bytes) = (0, 0);
        for d in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            for f in std::fs::read_dir(d.path()).into_iter().flatten().flatten() {
                let p = f.path();
                if !matches!(p.extension().and_then(|e| e.to_str()), Some("kicad_pcb") | Some("kicad_dru")) {
                    continue;
                }
                let text = std::fs::read_to_string(&p).unwrap();
                let doc = Doc::parse(text.clone()).unwrap_or_else(|e| panic!("{}: {}", p.display(), e));
                assert!(doc.write() == text, "{} does not come back as it was read", p.display());
                assert!(Edits::new().apply(&doc).unwrap() == text);
                boards += 1;
                bytes += text.len();
            }
        }
        eprintln!("round trip: {} files, {} bytes, identical", boards, bytes);
        assert!(boards >= 40 || !dir.exists(), "the corpus is not where it was: {}", dir.display());
    }

    #[test]
    fn says_what_is_wrong() {
        let err = |t: &str| Doc::parse(t.into()).err().unwrap_or_default();
        assert!(err("(a (b)").contains("never closed"));
        assert!(err("(a))").contains("closes nothing"));
        assert!(err("(a \"b)").contains("never ends"));
        assert!(Doc::parse("(a \"b\\".into()).is_err());
    }
}
