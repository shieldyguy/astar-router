//! The rules that live beside the board: the project file (`.kicad_pro`: the board's minimums, the net
//! classes and which nets are in them) and the custom rules (`.kicad_dru`). A board alone gets KiCad's
//! own defaults.

use super::sexpr::Doc;
use crate::json::{self, J};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct Class {
    pub name: String,
    pub priority: f64,
    pub clearance: Option<f64>,
    pub track: Option<f64>,
    pub via: Option<f64>,
    pub drill: Option<f64>,
    pub pair_width: Option<f64>,
    pub pair_gap: Option<f64>,
}

/// One custom rule that was read: a clearance or a width that holds on some layers, between two
/// classes, or everywhere.
pub struct Custom {
    pub name: String,
    /// `None`: every layer; else `inner`, `outer` or a layer's name
    pub layer: Option<String>,
    /// the classes the rule names (`A.NetClass == 'X'`), none, one or two
    pub classes: Vec<String>,
    pub clearance: Option<f64>,
    pub track: Option<f64>,
}

pub struct Project {
    /// the board's minimums, mm: KiCad's defaults where the project does not say
    pub min_clearance: f64,
    pub min_track: f64,
    pub min_via: f64,
    pub min_drill: f64,
    pub min_annular: f64,
    pub edge: f64,
    pub hole_clearance: f64,
    pub hole_to_hole: f64,
    /// the fewest spokes a thermal relief may be left with
    pub min_spokes: f64,
    /// classes[0] is Default
    pub classes: Vec<Class>,
    patterns: Vec<(String, String)>,
    assigned: HashMap<String, Vec<String>>,
    /// from the custom rules: the most any rule asks from the edge, a hole, between holes
    pub dru_edge: f64,
    pub dru_hole: f64,
    pub dru_hole_to_hole: f64,
    pub custom: Vec<Custom>,
    /// the custom rules that were not read, by name
    pub unread: Vec<String>,
    pub found_pro: bool,
    pub found_dru: bool,
}

/// `*` and `?` against the whole name, as KiCad matches a net class pattern.
pub fn wild(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni, mut star, mut mark) = (0usize, 0usize, usize::MAX, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = ni;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// `0.2mm`, `8mil`, `0.01in` or a bare number (mm), in mm.
fn length(text: &str) -> Option<f64> {
    let t = text.trim();
    let (num, mul) = if let Some(v) = t.strip_suffix("mm") {
        (v, 1.0)
    } else if let Some(v) = t.strip_suffix("mil") {
        (v, 0.0254)
    } else if let Some(v) = t.strip_suffix("in") {
        (v, 25.4)
    } else if let Some(v) = t.strip_suffix("um") {
        (v, 0.001)
    } else {
        (t, 1.0)
    };
    num.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v * mul)
}

impl Project {
    /// KiCad's defaults for a board with no project.
    pub fn defaults() -> Project {
        Project {
            min_clearance: 0.0,
            min_track: 0.2, // KiCad's default when the project names none
            min_via: 0.5,
            min_drill: 0.3,
            min_annular: 0.1,
            edge: 0.5,
            hole_clearance: 0.25,
            hole_to_hole: 0.25,
            min_spokes: 2.0,
            classes: vec![Class { name: "Default".into(), priority: 2147483647.0, clearance: Some(0.2), track: Some(0.2), via: Some(0.6), drill: Some(0.3), pair_width: Some(0.2), pair_gap: Some(0.25) }],
            patterns: Vec::new(),
            assigned: HashMap::new(),
            dru_edge: 0.0,
            dru_hole: 0.0,
            dru_hole_to_hole: 0.0,
            custom: Vec::new(),
            unread: Vec::new(),
            found_pro: false,
            found_dru: false,
        }
    }

    /// The project and the custom rules beside `board` (a path ending in `.kicad_pcb`), where they are.
    pub fn beside(board: &str) -> Result<Project, String> {
        let stem = board.strip_suffix(".kicad_pcb").unwrap_or(board);
        let mut p = Project::defaults();
        if let Ok(text) = std::fs::read_to_string(format!("{}.kicad_pro", stem)) {
            p.read_pro(&text).map_err(|e| format!("{}.kicad_pro: {}", stem, e))?;
        }
        if let Ok(text) = std::fs::read_to_string(format!("{}.kicad_dru", stem)) {
            p.read_dru(text).map_err(|e| format!("{}.kicad_dru: {}", stem, e))?;
        }
        Ok(p)
    }

    pub fn read_pro(&mut self, text: &str) -> Result<(), String> {
        let j = json::parse(text)?;
        self.found_pro = true;
        let r = j.get("board").get("design_settings").get("rules");
        let take = |k: &str, to: &mut f64| {
            if let Some(v) = r.get(k).f().filter(|v| v.is_finite() && *v >= 0.0) {
                *to = v;
            }
        };
        take("min_clearance", &mut self.min_clearance);
        take("min_track_width", &mut self.min_track);
        take("min_via_diameter", &mut self.min_via);
        take("min_through_hole_diameter", &mut self.min_drill);
        take("min_via_annular_width", &mut self.min_annular);
        take("min_copper_edge_clearance", &mut self.edge);
        take("min_hole_clearance", &mut self.hole_clearance);
        take("min_hole_to_hole", &mut self.hole_to_hole);
        take("min_resolved_spokes", &mut self.min_spokes);
        let ns = j.get("net_settings");
        let pos = |v: &J| v.f().filter(|v| v.is_finite() && *v > 0.0);
        for c in ns.get("classes").arr() {
            let name = c.get("name").s().unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let class = Class {
                name: name.clone(),
                priority: c.get("priority").f_or(0.0),
                clearance: c.get("clearance").f().filter(|v| v.is_finite() && *v >= 0.0),
                track: pos(c.get("track_width")),
                via: pos(c.get("via_diameter")),
                drill: pos(c.get("via_drill")),
                pair_width: pos(c.get("diff_pair_width")),
                pair_gap: pos(c.get("diff_pair_gap")),
            };
            if name == "Default" {
                // what the Default class does not say stays KiCad's default
                let d = &mut self.classes[0];
                d.clearance = class.clearance.or(d.clearance);
                d.track = class.track.or(d.track);
                d.via = class.via.or(d.via);
                d.drill = class.drill.or(d.drill);
                d.pair_width = class.pair_width.or(d.pair_width);
                d.pair_gap = class.pair_gap.or(d.pair_gap);
            } else if !self.classes.iter().any(|k| k.name == name) {
                self.classes.push(class);
            }
            // KiCad 6 lists a class's nets in the class itself
            if name != "Default" {
                for n in c.get("nets").arr() {
                    if let Some(net) = n.s() {
                        self.assigned.entry(net.to_string()).or_insert_with(Vec::new).push(name.clone());
                    }
                }
            }
        }
        for p in ns.get("netclass_patterns").arr() {
            if let (Some(c), Some(pat)) = (p.get("netclass").s(), p.get("pattern").s()) {
                self.patterns.push((pat.to_string(), c.to_string()));
            }
        }
        if let J::Obj(kv) = ns.get("netclass_assignments") {
            for (net, v) in kv {
                let names: Vec<String> = match v {
                    J::Str(c) => vec![c.clone()],
                    J::Arr(l) => l.iter().filter_map(|c| c.s().map(|c| c.to_string())).collect(),
                    _ => Vec::new(),
                };
                self.assigned.insert(net.clone(), names);
            }
        }
        Ok(())
    }

    pub fn read_dru(&mut self, text: String) -> Result<(), String> {
        let d = Doc::parse(text)?;
        self.found_dru = true;
        for &rule in d.tops() {
            if d.head(rule) != "rule" {
                continue;
            }
            let name = d.arg_str(rule, 0).unwrap_or_default().into_owned();
            let cond = d.str_of(rule, "condition").map(|c| c.into_owned());
            let layer = d.str_of(rule, "layer").map(|l| l.into_owned());
            // the classes a condition names, when it names nothing else
            let mut classes = Vec::new();
            let mut plain = true;
            if let Some(c) = &cond {
                for term in c.split("&&") {
                    let t: String = term.chars().filter(|ch| !ch.is_whitespace()).collect();
                    match t.strip_prefix("A.NetClass=='").or_else(|| t.strip_prefix("B.NetClass=='")).and_then(|r| r.strip_suffix('\'')) {
                        Some(cl) if !cl.contains('\'') => classes.push(cl.to_string()),
                        _ => plain = false,
                    }
                }
            }
            let mut read = false;
            for c in d.all(rule, "constraint") {
                let kind = d.arg_str(c, 0).unwrap_or_default().into_owned();
                let val = |k: &str| d.str_of(c, k).and_then(|v| length(&v));
                match kind.as_str() {
                    // the most any rule asks, whatever it asks it of: the router keeps that from everything
                    "edge_clearance" => self.dru_edge = self.dru_edge.max(val("min").unwrap_or(0.0)),
                    "hole_clearance" => self.dru_hole = self.dru_hole.max(val("min").unwrap_or(0.0)),
                    "hole_to_hole" => self.dru_hole_to_hole = self.dru_hole_to_hole.max(val("min").unwrap_or(0.0)),
                    "clearance" | "track_width" if plain && classes.len() <= 2 => {
                        let (clearance, track) = if kind == "clearance" { (val("min"), None) } else { (None, val("opt").or(val("min"))) };
                        // a width for a class is the class's business, not a layer's
                        if (clearance.is_some() || track.is_some()) && (track.is_none() || classes.is_empty()) {
                            self.custom.push(Custom { name: name.clone(), layer: layer.clone(), classes: classes.clone(), clearance, track });
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                }
                read = true;
            }
            if !read {
                self.unread.push(name);
            }
        }
        Ok(())
    }

    /// The class a net is in. A net in several classes (KiCad 9 on) is in a class made of them: each
    /// value from the class of highest priority that gives it. Returns the class's index, adding the
    /// made-up class to `classes` when it is new.
    pub fn class_of(&mut self, net: &str) -> usize {
        let mut names: Vec<&str> = self.assigned.get(net).map_or(Vec::new(), |v| v.iter().map(|s| s.as_str()).collect());
        for (pat, class) in &self.patterns {
            if wild(pat, net) && !names.contains(&class.as_str()) {
                names.push(class);
            }
        }
        let mut ix: Vec<usize> = names.iter().filter_map(|n| self.classes.iter().position(|c| c.name == *n)).filter(|i| *i != 0).collect();
        ix.dedup();
        match ix.len() {
            0 => 0,
            1 => ix[0],
            _ => {
                ix.sort_by(|a, b| self.classes[*a].priority.partial_cmp(&self.classes[*b].priority).unwrap_or(std::cmp::Ordering::Equal).then(self.classes[*a].name.cmp(&self.classes[*b].name)));
                let name = ix.iter().map(|i| self.classes[*i].name.as_str()).collect::<Vec<_>>().join(",");
                if let Some(i) = self.classes.iter().position(|c| c.name == name) {
                    return i;
                }
                let mut m = Class { name, priority: self.classes[ix[0]].priority, ..Class::default() };
                for &i in &ix {
                    let c = &self.classes[i];
                    m.clearance = m.clearance.or(c.clearance);
                    m.track = m.track.or(c.track);
                    m.via = m.via.or(c.via);
                    m.drill = m.drill.or(c.drill);
                    m.pair_width = m.pair_width.or(c.pair_width);
                    m.pair_gap = m.pair_gap.or(c.pair_gap);
                }
                self.classes.push(m);
                self.classes.len() - 1
            }
        }
    }

    /// The patterns that match none of these nets and look like regular expressions (KiCad tries a
    /// pattern both ways; astar reads wildcards only).
    pub fn odd_patterns(&self, nets: &[String]) -> Vec<String> {
        self.patterns.iter().filter(|(p, _)| p.contains(".*") || p.contains('[') || p.contains('|') || p.contains('\\')).filter(|(p, _)| !nets.iter().any(|n| wild(p, n))).map(|(p, _)| p.clone()).collect()
    }

    /// The Default class's value, else the board's.
    pub fn default_class(&self) -> &Class {
        &self.classes[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wild("+5V", "+5V") && !wild("+5V", "+5VA"));
        assert!(wild("V*", "VBUS") && wild("*clk", "sys_clk") && !wild("*clk", "clk2"));
        assert!(wild("/*USB_*", "/a/USB_DP") && wild("Net-(D2-Pad2)", "Net-(D2-Pad2)") && wild("D?", "D7"));
    }

    #[test]
    fn classes_and_rules() {
        let mut p = Project::defaults();
        p.read_pro(r#"{"board": {"design_settings": {"rules": {"min_clearance": 0.1, "min_copper_edge_clearance": 0.3}}},
            "net_settings": {"classes": [{"name": "Default", "clearance": 0.15, "track_width": 0.2, "priority": 2147483647},
              {"name": "Power", "track_width": 0.5, "priority": 1}, {"name": "HV", "clearance": 0.6, "priority": 0}],
            "netclass_patterns": [{"netclass": "Power", "pattern": "+*"}], "netclass_assignments": {"+48V": ["HV"]}}}"#)
            .unwrap();
        assert_eq!((p.min_clearance, p.edge, p.min_via), (0.1, 0.3, 0.5));
        assert_eq!(p.class_of("SIG"), 0);
        let power = p.class_of("+5V");
        assert_eq!(p.classes[power].name, "Power");
        let both = p.class_of("+48V");
        let c = p.classes[both].clone();
        assert_eq!((c.name.as_str(), c.clearance, c.track), ("HV,Power", Some(0.6), Some(0.5)));
        p.read_dru("(version 1)\n(rule \"inner\" (layer inner) (constraint clearance (min 0.2mm)))\n(rule hv (condition \"A.NetClass == 'HV' && B.NetClass == 'Power'\") (constraint clearance (min 8mil)))\n(rule \"edge\" (condition \"A.Type == 'Track'\") (constraint edge_clearance (min 0.4mm)))\n(rule \"silk\" (constraint silk_clearance (min 0.1mm)))\n".into()).unwrap();
        assert_eq!(p.custom.len(), 2);
        assert_eq!(p.custom[1].classes, vec!["HV", "Power"]);
        assert!((p.custom[1].clearance.unwrap() - 0.2032).abs() < 1e-12);
        assert_eq!((p.dru_edge, p.unread.clone()), (0.4, vec!["silk".to_string()]));
        let _ = both;
    }
}
