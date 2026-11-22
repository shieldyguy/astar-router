//! A board file opened: its tree, its copper layers, how it names its nets, and the small readers
//! every part of the reader shares.

use super::num::{nm, P};
use super::sexpr::{Doc, Id, Kind};
use crate::json::J;
use std::collections::HashMap;

pub struct Board {
    pub doc: Doc,
    pub root: Id,
    /// the file's `(version N)`: 20171130 is KiCad 5, 2022... 6 and 7, 2024... 8 and 9, 2025... 10
    pub version: u64,
    /// the copper layers by their canonical names, top to bottom
    pub copper: Vec<String>,
    /// every layer's canonical name by any name it goes by (its own, the user's)
    canon: HashMap<String, String>,
    /// nets by number, where the file numbers them (KiCad 9 and before)
    net_names: HashMap<i64, String>,
    /// what the reader has to say about the board: what it could not read, what it made up
    pub notes: Vec<String>,
    /// what the board asks that was not read and KiCad may hold the routed board to
    pub doubts: Vec<String>,
}

fn layer_rank(name: &str) -> i64 {
    match name {
        "F.Cu" => -1,
        "B.Cu" => i64::MAX,
        _ => name.trim_start_matches("In").trim_end_matches(".Cu").parse().unwrap_or(i64::MAX - 1),
    }
}

impl Board {
    pub fn open(text: String) -> Result<Board, String> {
        let doc = Doc::parse(text)?;
        let root = doc.root().ok_or("not a KiCad board: the file is not one list")?;
        if doc.head(root) != "kicad_pcb" {
            return Err(format!("not a KiCad board: the file starts with ({} and not (kicad_pcb", doc.head(root)));
        }
        let version = doc.num_of(root, "version").unwrap_or(0.0) as u64;
        let mut copper = Vec::new();
        let mut canon = HashMap::new();
        if let Some(layers) = doc.find(root, "layers") {
            for l in doc.lists(layers) {
                // (0 "F.Cu" signal ["user name"])
                let mut it = doc.kids(l).skip(1);
                let Some(name) = it.next().map(|k| doc.val(k).into_owned()) else { continue };
                let user = it.nth(1).map(|k| doc.val(k).into_owned());
                if name.ends_with(".Cu") {
                    copper.push(name.clone());
                }
                if let Some(u) = user {
                    canon.insert(u, name.clone());
                }
                canon.insert(name.clone(), name);
            }
        }
        copper.sort_by_key(|n| layer_rank(n));
        copper.dedup();
        if copper.len() < 2 {
            return Err(format!("the board has {} copper layers: a board has two or more", copper.len()));
        }
        let mut net_names = HashMap::new();
        for n in doc.all(root, "net") {
            if let (Some(id), Some(name)) = (doc.arg_num(n, 0), doc.arg_str(n, 1)) {
                net_names.insert(id as i64, name.into_owned());
            }
        }
        Ok(Board { doc, root, version, copper, canon, net_names, notes: Vec::new(), doubts: Vec::new() })
    }

    pub fn doubt(&mut self, what: String) {
        if !self.doubts.contains(&what) {
            self.doubts.push(what);
        }
    }
    pub fn note(&mut self, what: String) {
        if !self.notes.contains(&what) {
            self.notes.push(what);
        }
    }

    /// A layer's canonical name, by whatever name the file uses.
    pub fn layer<'a>(&'a self, name: &'a str) -> &'a str {
        self.canon.get(name).map_or(name, |s| s.as_str())
    }

    /// The copper layer's place in the stack.
    pub fn cu(&self, name: &str) -> Option<usize> {
        let c = self.layer(name);
        self.copper.iter().position(|l| l == c)
    }

    /// The copper layers a `(layers ...)` or `(layer ...)` child of `id` names: `*.Cu` is all of them,
    /// `F&B.Cu` the two outer ones.
    pub fn cu_layers(&self, id: Id) -> Vec<usize> {
        let mut out = Vec::new();
        for name in ["layers", "layer"] {
            let Some(l) = self.doc.find(id, name) else { continue };
            for k in self.doc.kids(l).skip(1) {
                let v = self.doc.val(k);
                match v.as_ref() {
                    "*.Cu" => out.extend(0..self.copper.len()),
                    "F&B.Cu" => out.extend([0, self.copper.len() - 1]),
                    v => out.extend(self.cu(v)),
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The first layer a thing is on, canonical.
    pub fn layer_of(&self, id: Id) -> String {
        self.doc.str_of(id, "layer").map_or(String::new(), |l| self.layer(&l).to_string())
    }

    /// The net a thing is on, by name: `(net "GND")`, `(net 5 "GND")`, or `(net 5)` looked up. `""`
    /// is no net.
    pub fn net_of(&self, id: Id) -> String {
        let Some(n) = self.doc.find(id, "net") else { return String::new() };
        let Some(a) = self.doc.arg(n, 0) else { return String::new() };
        if self.doc.kind(a) == Kind::Str {
            return self.doc.val(a).into_owned();
        }
        if let Some(name) = self.doc.arg_str(n, 1) {
            return name.into_owned();
        }
        match self.doc.raw(a).parse::<i64>() {
            Ok(0) => String::new(),
            Ok(k) => self.net_names.get(&k).cloned().or_else(|| self.doc.str_of(id, "net_name").map(|s| s.into_owned())).unwrap_or_default(),
            // a bare name (KiCad 5 writes names without quotes)
            Err(_) => self.doc.val(a).into_owned(),
        }
    }

    /// Does the file number its nets (KiCad 9 and before) or name them (the house format)?
    pub fn nets_by_number(&self) -> bool {
        !self.net_names.is_empty()
    }

    /// The number the file gives a net, where it numbers them.
    pub fn net_number(&self, name: &str) -> Option<i64> {
        self.net_names.iter().find(|(_, n)| n.as_str() == name).map(|(k, _)| *k)
    }

    /// `(name x y)` in nanometres.
    pub fn xy(&self, id: Id, name: &str) -> Option<P> {
        let n = self.doc.find(id, name)?;
        Some((nm(self.doc.arg_num(n, 0)?), nm(self.doc.arg_num(n, 1)?)))
    }

    /// `(name v)` in nanometres.
    pub fn len(&self, id: Id, name: &str) -> Option<i64> {
        self.doc.num_of(id, name).map(nm)
    }

    /// Is the thing locked: `(locked yes)`, a bare `(locked)`, or the word `locked` (KiCad 6 and 7)?
    pub fn locked(&self, id: Id) -> bool {
        self.doc.flag(id, "locked") || self.doc.has_word(id, "locked")
    }

    /// A stroke's width in nanometres: `(stroke (width w) ...)` or the older `(width w)`.
    pub fn stroke(&self, id: Id) -> i64 {
        self.doc.find(id, "stroke").and_then(|s| self.len(s, "width")).or_else(|| self.len(id, "width")).unwrap_or(0)
    }
}

// what the scene is built from

pub fn s(v: &str) -> J {
    J::Str(v.to_string())
}

pub fn n(v: f64) -> J {
    J::Num(v)
}

pub fn obj(kv: Vec<(&str, J)>) -> J {
    J::Obj(kv.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// Nanometres as the scene's millimetres.
pub fn len(v: i64) -> J {
    J::Num(super::num::mm(v))
}

pub fn pt(p: P) -> J {
    J::Arr(vec![len(p.0), len(p.1)])
}

pub fn ptf(p: (f64, f64)) -> J {
    J::Arr(vec![J::Num(p.0), J::Num(p.1)])
}

pub fn poly(pts: &[(f64, f64)]) -> J {
    J::Arr(pts.iter().map(|p| ptf(*p)).collect())
}
