//! Scene 2 read from JSON (docs/scene.md). Strict: a key it does not know, a value of the wrong kind or
//! a name that points nowhere is an error that says where.

use super::read::shape_of;
use super::*;
use crate::geom::arc_points;
use crate::json::{quote, J};
use std::collections::HashMap;

/// An object of the scene and where it is, for the messages.
struct O<'a> {
    j: &'a J,
    at: String,
}

impl<'a> O<'a> {
    fn new(j: &'a J, at: String, keys: &[&str]) -> Result<O<'a>, String> {
        let J::Obj(v) = j else { return Err(format!("{}: not an object", at)) };
        for (k, _) in v {
            if !keys.contains(&k.as_str()) {
                return Err(format!("{}: unknown key {}", at, quote(k)));
            }
        }
        Ok(O { j, at })
    }
    fn bad<T>(&self, k: &str, want: &str) -> Result<T, String> {
        Err(format!("{}: {} must be {}", self.at, quote(k), want))
    }
    fn f_opt(&self, k: &str) -> Result<Option<f64>, String> {
        match self.j.get(k) {
            J::Null => Ok(None),
            J::Num(n) => Ok(Some(*n)),
            _ => self.bad(k, "a number"),
        }
    }
    fn f(&self, k: &str) -> Result<f64, String> {
        match self.f_opt(k)? {
            Some(n) => Ok(n),
            None => Err(format!("{}: no {}", self.at, quote(k))),
        }
    }
    fn s_opt(&self, k: &str) -> Result<Option<&'a str>, String> {
        match self.j.get(k) {
            J::Null => Ok(None),
            J::Str(s) => Ok(Some(s.as_str())),
            _ => self.bad(k, "a string"),
        }
    }
    fn s(&self, k: &str) -> Result<&'a str, String> {
        match self.s_opt(k)? {
            Some(s) => Ok(s),
            None => Err(format!("{}: no {}", self.at, quote(k))),
        }
    }
    /// A flag: absent is false.
    fn b(&self, k: &str) -> Result<bool, String> {
        match self.j.get(k) {
            J::Null => Ok(false),
            J::Bool(b) => Ok(*b),
            _ => self.bad(k, "true or false"),
        }
    }
    /// A list: absent is empty.
    fn arr(&self, k: &str) -> Result<&'a [J], String> {
        match self.j.get(k) {
            J::Null => Ok(&[]),
            J::Arr(v) => Ok(v.as_slice()),
            _ => self.bad(k, "a list"),
        }
    }
    fn strs(&self, k: &str) -> Result<Vec<&'a str>, String> {
        let mut out = Vec::new();
        for x in self.arr(k)? {
            match x.s() {
                Some(s) => out.push(s),
                None => return self.bad(k, "a list of strings"),
            }
        }
        Ok(out)
    }
}

fn nums(j: &J, n: usize, at: &str) -> Result<Vec<f64>, String> {
    let v = j.nums();
    if !matches!(j, J::Arr(_)) || v.len() != n || j.arr().len() != n {
        return Err(format!("{}: must be a list of {} numbers", at, n));
    }
    Ok(v)
}

fn points(list: &[J], at: &str) -> Result<Vec<(f64, f64)>, String> {
    let mut out = Vec::with_capacity(list.len());
    for p in list {
        let v = nums(p, 2, &format!("{}: a point", at))?;
        out.push((v[0], v[1]));
    }
    if out.len() < 3 {
        return Err(format!("{}: a polygon needs at least 3 points", at));
    }
    Ok(out)
}

fn loops(list: &[J], at: &str) -> Result<Vec<Vec<(f64, f64)>>, String> {
    list.iter().map(|l| if let J::Arr(v) = l { points(v, at) } else { Err(format!("{}: must be a list of polygons", at)) }).collect()
}

fn path(list: &[J], at: &str) -> Result<Vec<PathPiece>, String> {
    let mut out = Vec::with_capacity(list.len());
    for q in list {
        let o = O::new(q, format!("{}: a piece", at), &["line", "arc"])?;
        if o.j.has("line") {
            let v = nums(o.j.get("line"), 4, &format!("{}: a line", at))?;
            out.push(PathPiece::Line([v[0], v[1], v[2], v[3]]));
        } else if o.j.has("arc") {
            let v = nums(o.j.get("arc"), 6, &format!("{}: an arc", at))?;
            out.push(PathPiece::Arc([v[0], v[1], v[2], v[3], v[4], v[5]]));
        } else {
            return Err(format!("{}: a piece that is neither line nor arc", at));
        }
    }
    Ok(out)
}

fn impedance(j: &J, at: &str) -> Result<Option<Impedance>, String> {
    if matches!(j, J::Null) {
        return Ok(None);
    }
    let o = O::new(j, format!("{}: impedance", at), &["ohms", "differential", "tolerance", "coplanar_gap"])?;
    Ok(Some(Impedance { ohms: o.f("ohms")?, differential: o.b("differential")?, tolerance: o.f_opt("tolerance")?, coplanar_gap: o.f_opt("coplanar_gap")? }))
}

struct Names<'a> {
    layers: HashMap<&'a str, usize>,
    nets: HashMap<&'a str, u16>,
}

impl<'a> Names<'a> {
    fn layer(&self, name: &str, at: &str) -> Result<usize, String> {
        self.layers.get(name).copied().ok_or_else(|| format!("{}: layer {} is not in stack.copper", at, quote(name)))
    }
    /// A net by name; "" is no net.
    fn net(&self, name: &str, at: &str) -> Result<u16, String> {
        if name.is_empty() {
            return Ok(0);
        }
        self.nets.get(name).copied().ok_or_else(|| format!("{}: net {} is not in nets", at, quote(name)))
    }
    fn layer_rules(&self, list: &[J], at: &str) -> Result<Vec<LayerRule>, String> {
        let mut out = Vec::new();
        for (i, r) in list.iter().enumerate() {
            let at = format!("{}.layers[{}]", at, i);
            let o = O::new(r, at.clone(), &["layer", "track", "clearance"])?;
            out.push(LayerRule { layer: self.layer(o.s("layer")?, &at)?, track: o.f_opt("track")?, clearance: o.f_opt("clearance")? });
        }
        Ok(out)
    }
}

const TOP: &[&str] = &["version", "units", "bench", "source", "stack", "outline", "rules", "classes", "class_pairs", "nets", "pads", "holes", "keepouts", "copper", "parts", "pairs", "match", "groups", "scope"];

impl Scene {
    pub(super) fn read2(j: &J) -> Result<Scene, String> {
        let top = O::new(j, "scene".into(), TOP)?;
        if let Some(u) = top.s_opt("units")? {
            if u != "mm" {
                return Err(format!("scene: units {}: only \"mm\" is read", quote(u)));
            }
        }
        for k in ["bench", "source"] {
            if !matches!(j.get(k), J::Null | J::Obj(_)) {
                return top.bad(k, "an object");
            }
        }

        // the stack
        let stack = O::new(j.get("stack"), "stack".into(), &["copper", "planes", "layers"])?;
        let layers: Vec<String> = stack.strs("copper")?.into_iter().map(|s| s.to_string()).collect();
        if layers.len() < 2 {
            return Err("stack.copper needs at least two layers".into());
        }
        let mut names = Names { layers: HashMap::new(), nets: HashMap::new() };
        for (i, l) in layers.iter().enumerate() {
            if names.layers.insert(l.as_str(), i).is_some() {
                return Err(format!("stack.copper: layer {} twice", quote(l)));
            }
        }
        let mut stackup = Vec::new();
        for (i, l) in stack.arr("layers")?.iter().enumerate() {
            let o = O::new(l, format!("stack.layers[{}]", i), &["name", "type", "thickness", "epsilon_r", "loss_tangent", "material"])?;
            let kind = match o.s("type")? {
                "copper" => StackKind::Copper,
                "core" => StackKind::Core,
                "prepreg" => StackKind::Prepreg,
                "mask" => StackKind::Mask,
                t => return Err(format!("{}: type {} is not copper, core, prepreg or mask", o.at, quote(t))),
            };
            stackup.push(StackLayer { name: o.s("name")?.to_string(), kind, thickness: o.f("thickness")?, epsilon_r: o.f_opt("epsilon_r")?, loss_tangent: o.f_opt("loss_tangent")?, material: o.s_opt("material")?.map(|s| s.to_string()) });
        }
        if !stackup.is_empty() {
            let cu: Vec<&str> = stackup.iter().filter(|l| l.kind == StackKind::Copper).map(|l| l.name.as_str()).collect();
            if cu != layers.iter().map(|s| s.as_str()).collect::<Vec<_>>() {
                return Err("stack.layers: its copper layers are not stack.copper, in order".into());
            }
        }

        // the rules
        let r = O::new(j.get("rules"), "rules".into(), &["track", "clearance", "via", "drill", "edge", "hole_clearance", "hole_to_hole", "min_spokes", "floors", "layers"])?;
        let mut floors = Floors::default();
        if r.j.has("floors") {
            let f = O::new(r.j.get("floors"), "rules.floors".into(), &["track", "clearance", "via", "drill", "annular", "hole_to_hole", "edge"])?;
            floors = Floors { track: f.f_opt("track")?, clearance: f.f_opt("clearance")?, via: f.f_opt("via")?, drill: f.f_opt("drill")?, annular: f.f_opt("annular")?, hole_to_hole: f.f_opt("hole_to_hole")?, edge: f.f_opt("edge")? };
        }
        let rules = Rules {
            track: r.f("track")?,
            clearance: r.f("clearance")?,
            via: r.f("via")?,
            drill: r.f("drill")?,
            edge: r.f("edge")?,
            hole_clearance: r.f("hole_clearance")?,
            hole_to_hole: r.f_opt("hole_to_hole")?,
            min_spokes: r.f_opt("min_spokes")?,
            floors,
            layer_rules: names.layer_rules(r.arr("layers")?, "rules")?,
        };

        // the classes: the first is Default, there or not
        let mut classes = vec![NetClass { name: "Default".into(), track: None, clearance: None, via: None, drill: None, pair_width: None, pair_gap: None, impedance: None, layer_rules: vec![] }];
        let mut class_ix: HashMap<&str, usize> = HashMap::new();
        class_ix.insert("Default", 0);
        for (i, c) in top.arr("classes")?.iter().enumerate() {
            let at = format!("classes[{}]", i);
            let o = O::new(c, at.clone(), &["name", "track", "clearance", "via", "drill", "pair_width", "pair_gap", "impedance", "layers"])?;
            let name = o.s("name")?;
            let class = NetClass {
                name: name.to_string(),
                track: o.f_opt("track")?,
                clearance: o.f_opt("clearance")?,
                via: o.f_opt("via")?,
                drill: o.f_opt("drill")?,
                pair_width: o.f_opt("pair_width")?,
                pair_gap: o.f_opt("pair_gap")?,
                impedance: impedance(o.j.get("impedance"), &at)?,
                layer_rules: names.layer_rules(o.arr("layers")?, &at)?,
            };
            if name == "Default" && i == 0 {
                classes[0] = class;
            } else if class_ix.insert(name, classes.len()).is_some() {
                return Err(format!("{}: class {} twice (Default, when given, comes first)", at, quote(name)));
            } else {
                classes.push(class);
            }
        }
        let class_of = |name: &str, at: &str| class_ix.get(name).copied().ok_or_else(|| format!("{}: class {} is not in classes", at, quote(name)));
        let mut class_pairs = Vec::new();
        for (i, c) in top.arr("class_pairs")?.iter().enumerate() {
            let at = format!("class_pairs[{}]", i);
            let o = O::new(c, at.clone(), &["a", "b", "clearance"])?;
            class_pairs.push(ClassPair { a: class_of(o.s("a")?, &at)?, b: class_of(o.s("b")?, &at)?, clearance: o.f("clearance")? });
        }

        // the nets
        let mut nets: Vec<Net> = vec![Net::plain(String::new(), Class::Signal, rules.track, 0.0)];
        let net_list = top.arr("nets")?;
        if net_list.len() > 60000 {
            return Err("more than 60000 nets".into());
        }
        for (i, n) in net_list.iter().enumerate() {
            let at = format!("nets[{}]", i);
            let o = O::new(n, at.clone(), &["name", "role", "class", "width", "clearance", "via", "drill", "impedance", "route_on"])?;
            let name = o.s("name")?;
            if name.is_empty() {
                return Err(format!("{}: a net without a name", at));
            }
            let role = match o.s_opt("role")?.unwrap_or("signal") {
                "signal" => Class::Signal,
                "rail" => Class::Rail,
                "ground" => Class::Ground,
                t => return Err(format!("{}: role {} is not signal, rail or ground", at, quote(t))),
            };
            if names.nets.insert(name, nets.len() as u16).is_some() {
                return Err(format!("{}: net {} twice", at, quote(name)));
            }
            let mut net = Net::plain(name.to_string(), role, o.f_opt("width")?.unwrap_or(rules.track), o.f_opt("clearance")?.unwrap_or(0.0));
            net.netclass = class_of(o.s_opt("class")?.unwrap_or("Default"), &at)?;
            net.via = o.f_opt("via")?;
            net.drill = o.f_opt("drill")?;
            net.impedance = impedance(o.j.get("impedance"), &at)?;
            for l in o.j.get("route_on").arr() {
                let Some(l) = l.s() else { return Err(format!("{}: route_on is a list of layer names", at)) };
                net.keep |= 1 << names.layer(l, &at)?;
            }
            nets.push(net);
        }
        for (i, pl) in stack.arr("planes")?.iter().enumerate() {
            let at = format!("stack.planes[{}]", i);
            let o = O::new(pl, at.clone(), &["net", "layer"])?;
            let n = names.net(o.s("net")?, &at)?;
            if n == 0 {
                return Err(format!("{}: a plane without a net", at));
            }
            nets[n as usize].plane = Some(names.layer(o.s("layer")?, &at)?);
        }

        // the pads
        let mut pads = Vec::new();
        for (i, p) in top.arr("pads")?.iter().enumerate() {
            let o = O::new(p, format!("pads[{}]", i), &["ref", "num", "net", "at", "fp", "size", "tht", "side", "shape", "clearance", "drill", "pour", "thermal_gap", "spoke_angle"])?;
            let at = format!("pad {}.{}", o.s("ref")?, o.s("num")?);
            let net = names.net(o.s_opt("net")?.unwrap_or(""), &at)?;
            let xy = nums(o.j.get("at"), 2, &format!("{}: at", at))?;
            let fp = if o.j.has("fp") { nums(o.j.get("fp"), 2, &format!("{}: fp", at))? } else { xy.clone() };
            if o.j.has("size") {
                nums(o.j.get("size"), 2, &format!("{}: size", at))?;
            }
            let back = match o.s_opt("side")?.unwrap_or("F") {
                "F" => false,
                "B" => true,
                t => return Err(format!("{}: side {} is not F or B", at, quote(t))),
            };
            O::new(o.j.get("shape"), format!("{}: shape", at), &["poly", "r", "circle", "oval"])?;
            let shape = shape_of(o.j.get("shape")).map_err(|e| format!("{}: {}", at, e))?;
            if net != 0 {
                nets[net as usize].pads.push(pads.len());
            }
            let pour = match o.s_opt("pour")? {
                None => None,
                Some("solid") => Some(ZonePads::Solid),
                Some("thermal") => Some(ZonePads::Thermal),
                Some("thermal_tht") => Some(ZonePads::ThermalTht),
                Some("none") => Some(ZonePads::None),
                Some(t) => return Err(format!("{}: pour {} is not solid, thermal, thermal_tht or none", at, quote(t))),
            };
            pads.push(Pad { ref_: o.s("ref")?.to_string(), num: o.s("num")?.to_string(), net, x: xy[0], y: xy[1], fx: fp[0], fy: fp[1], tht: o.b("tht")?, back, shape, clearance: o.f_opt("clearance")?.unwrap_or(0.0), drill: o.f_opt("drill")?, pour, thermal_gap: o.f_opt("thermal_gap")?, spoke_angle: o.f_opt("spoke_angle")? });
        }

        let mut holes = Vec::new();
        for (i, h) in top.arr("holes")?.iter().enumerate() {
            let v = nums(h, 3, &format!("holes[{}]", i))?;
            holes.push((v[0], v[1], v[2]));
        }

        let mut outline = Vec::new();
        for q in path(top.arr("outline")?, "outline")? {
            match q {
                PathPiece::Line(v) => outline.push((v[0], v[1], v[2], v[3])),
                PathPiece::Arc(v) => {
                    let pts = arc_points((v[0], v[1]), (v[2], v[3]), (v[4], v[5]), 0.005);
                    for w in pts.windows(2) {
                        outline.push((w[0].0, w[0].1, w[1].0, w[1].1));
                    }
                }
            }
        }
        if outline.is_empty() {
            return Err("the scene has no outline".into());
        }

        let mut keepouts = Vec::new();
        for (i, k) in top.arr("keepouts")?.iter().enumerate() {
            let at = format!("keepouts[{}]", i);
            let o = O::new(k, at.clone(), &["layers", "poly", "forbid", "name", "net"])?;
            let mut ls = Vec::new();
            for l in o.strs("layers")? {
                ls.push(names.layer(l, &at)?);
            }
            let mut forbid = FORBID_COPPER;
            if o.j.has("forbid") {
                forbid = Forbid { tracks: false, vias: false, zones: false, pads: false, footprints: false };
                for f in o.strs("forbid")? {
                    match f {
                        "tracks" => forbid.tracks = true,
                        "vias" => forbid.vias = true,
                        "zones" => forbid.zones = true,
                        "pads" => forbid.pads = true,
                        "footprints" => forbid.footprints = true,
                        t => return Err(format!("{}: forbid {} is not tracks, vias, zones, pads or footprints", at, quote(t))),
                    }
                }
            }
            keepouts.push(Keepout { layers: if ls.is_empty() { (0..layers.len()).collect() } else { ls }, poly: points(o.arr("poly")?, &at)?, forbid, name: o.s_opt("name")?.unwrap_or("").to_string(), net: names.net(o.s_opt("net")?.unwrap_or(""), &at)? });
        }

        // copper already on the board
        let mut copper = Copper::default();
        if j.has("copper") {
            let c = O::new(j.get("copper"), "copper".into(), &["tracks", "vias", "zones"])?;
            for (i, t) in c.arr("tracks")?.iter().enumerate() {
                let at = format!("copper.tracks[{}]", i);
                let o = O::new(t, at.clone(), &["net", "layer", "width", "path", "locked"])?;
                copper.tracks.push(OldTrack { net: names.net(o.s_opt("net")?.unwrap_or(""), &at)?, layer: names.layer(o.s("layer")?, &at)?, width: o.f("width")?, path: path(o.arr("path")?, &at)?, locked: o.b("locked")?, laid: false });
            }
            for (i, v) in c.arr("vias")?.iter().enumerate() {
                let at = format!("copper.vias[{}]", i);
                let o = O::new(v, at.clone(), &["net", "at", "size", "drill", "layers", "locked"])?;
                let xy = nums(o.j.get("at"), 2, &format!("{}: at", at))?;
                let span = match o.strs("layers")?.as_slice() {
                    [] => None,
                    [a, b] => Some((names.layer(a, &at)?, names.layer(b, &at)?)),
                    _ => return Err(format!("{}: layers must be the first and the last layer", at)),
                };
                copper.vias.push(OldVia { net: names.net(o.s_opt("net")?.unwrap_or(""), &at)?, x: xy[0], y: xy[1], size: o.f("size")?, drill: o.f("drill")?, span, locked: o.b("locked")? });
            }
            for (i, z) in c.arr("zones")?.iter().enumerate() {
                let at = format!("copper.zones[{}]", i);
                let o = O::new(z, at.clone(), &["net", "layer", "outline", "holes", "fill", "priority", "clearance", "min_width", "pads", "thermal_gap", "spoke_width", "locked"])?;
                let mut fill = Vec::new();
                for f in o.arr("fill")? {
                    let fo = O::new(f, format!("{}: fill", at), &["outline", "holes"])?;
                    fill.push(Island { outline: points(fo.arr("outline")?, &fo.at)?, holes: loops(fo.arr("holes")?, &fo.at)? });
                }
                let pads = match o.s_opt("pads")?.unwrap_or("solid") {
                    "solid" => ZonePads::Solid,
                    "thermal" => ZonePads::Thermal,
                    "thermal_tht" => ZonePads::ThermalTht,
                    "none" => ZonePads::None,
                    t => return Err(format!("{}: pads {} is not solid, thermal, thermal_tht or none", at, quote(t))),
                };
                copper.zones.push(Zone {
                    net: names.net(o.s_opt("net")?.unwrap_or(""), &at)?,
                    layer: names.layer(o.s("layer")?, &at)?,
                    outline: points(o.arr("outline")?, &at)?,
                    holes: loops(o.arr("holes")?, &at)?,
                    fill,
                    priority: o.f_opt("priority")?.unwrap_or(0.0).clamp(-1e6, 1e6) as i32,
                    clearance: o.f_opt("clearance")?,
                    min_width: o.f_opt("min_width")?,
                    pads,
                    thermal_gap: o.f_opt("thermal_gap")?,
                    spoke_width: o.f_opt("spoke_width")?,
                    locked: o.b("locked")?,
                });
            }
        }

        // parts, and what goes together
        let mut parts: Vec<Part> = Vec::new();
        let mut part_ix: HashMap<&str, usize> = HashMap::new();
        for (i, p) in top.arr("parts")?.iter().enumerate() {
            let at = format!("parts[{}]", i);
            let o = O::new(p, at.clone(), &["ref", "courtyard", "side", "fixed"])?;
            let name = o.s("ref")?;
            if part_ix.insert(name, parts.len()).is_some() {
                return Err(format!("{}: part {} twice", at, quote(name)));
            }
            let courtyard = if o.arr("courtyard")?.is_empty() { Vec::new() } else { points(o.arr("courtyard")?, &at)? };
            let back = match o.s_opt("side")?.unwrap_or("F") {
                "F" => false,
                "B" => true,
                t => return Err(format!("{}: side {} is not F or B", at, quote(t))),
            };
            parts.push(Part { ref_: name.to_string(), courtyard, back, fixed: o.b("fixed")?, in_scope: true });
        }
        let part_of = |name: &str, at: &str| part_ix.get(name).copied().ok_or_else(|| format!("{}: part {} is not in parts", at, quote(name)));
        let mut pairs = Vec::new();
        for (i, p) in top.arr("pairs")?.iter().enumerate() {
            let at = format!("pairs[{}]", i);
            let o = O::new(p, at.clone(), &["name", "p", "n", "width", "gap", "impedance", "max_skew", "max_uncoupled"])?;
            let (pn, nn) = (names.net(o.s("p")?, &at)?, names.net(o.s("n")?, &at)?);
            if pn == 0 || nn == 0 || pn == nn {
                return Err(format!("{}: a pair needs two nets", at));
            }
            pairs.push(Pair { name: o.s("name")?.to_string(), p: pn, n: nn, width: o.f_opt("width")?, gap: o.f_opt("gap")?, impedance: impedance(o.j.get("impedance"), &at)?, max_skew: o.f_opt("max_skew")?, max_uncoupled: o.f_opt("max_uncoupled")? });
        }
        let mut matches = Vec::new();
        for (i, m) in top.arr("match")?.iter().enumerate() {
            let at = format!("match[{}]", i);
            let o = O::new(m, at.clone(), &["name", "nets", "tolerance", "target", "by"])?;
            let mut ns = Vec::new();
            for n in o.strs("nets")? {
                let id = names.net(n, &at)?;
                if id == 0 || ns.contains(&id) {
                    return Err(format!("{}: net {} is empty or twice", at, quote(n)));
                }
                ns.push(id);
            }
            let by_time = match o.s_opt("by")?.unwrap_or("length") {
                "length" => false,
                "time" => true,
                t => return Err(format!("{}: by {} is not length or time", at, quote(t))),
            };
            matches.push(MatchGroup { name: o.s("name")?.to_string(), nets: ns, tolerance: o.f("tolerance")?, target: o.f_opt("target")?, by_time });
        }
        let mut groups = Vec::new();
        for (i, g) in top.arr("groups")?.iter().enumerate() {
            let at = format!("groups[{}]", i);
            let o = O::new(g, at.clone(), &["name", "parts"])?;
            let mut ps = Vec::new();
            for p in o.strs("parts")? {
                ps.push(part_of(p, &at)?);
            }
            groups.push(Group { name: o.s("name")?.to_string(), parts: ps });
        }

        // what is in scope: everything, unless the scene names the nets or the parts
        if j.has("scope") {
            let o = O::new(j.get("scope"), "scope".into(), &["nets", "parts"])?;
            if o.j.has("nets") {
                nets.iter_mut().skip(1).for_each(|n| n.in_scope = false);
                for n in o.strs("nets")? {
                    let id = names.net(n, "scope.nets")?;
                    nets[id as usize].in_scope = id != 0;
                }
            }
            if o.j.has("parts") {
                parts.iter_mut().for_each(|p| p.in_scope = false);
                for p in o.strs("parts")? {
                    parts[part_of(p, "scope.parts")?].in_scope = true;
                }
            }
        }

        let existing_copper = copper.tracks.len() + copper.vias.len() + copper.zones.len();
        Ok(Scene { version: 2, layers, rules, nets, pads, holes, outline, keepouts, existing_copper, stackup, classes, class_pairs, copper, parts, pairs, matches, groups, said: Vec::new() })
    }
}
