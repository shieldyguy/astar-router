//! Footprints: their pads (every shape, at any angle, the drill offset from the copper), the holes, the
//! parts with their courtyards, and what a footprint draws on Edge.Cuts and in copper.

use super::board::{len, n, obj, poly, pt, s, Board};
use super::num::{hull, inside, mm, nm, rot, rotf, Frame, P};
use super::shapes::{bbox, box_poly, drawn, Drawn, Geo};
use super::sexpr::Id;
use crate::json::J;
use std::collections::HashMap;

/// The part of a polygon that is nearer to p than to q.
fn nearer(pts: &[(f64, f64)], p: (f64, f64), q: (f64, f64)) -> Vec<(f64, f64)> {
    let (mx, my, dx, dy) = ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0, q.0 - p.0, q.1 - p.1);
    let side = |a: (f64, f64)| (a.0 - mx) * dx + (a.1 - my) * dy;
    let mut out = Vec::new();
    for k in 0..pts.len() {
        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa <= 0.0 {
            out.push(a);
        }
        if (sa < 0.0 && sb > 0.0) || (sa > 0.0 && sb < 0.0) {
            let t = sa / (sa - sb);
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    out
}

/// What the footprints give the scene.
#[derive(Default)]
pub struct Feet {
    pub pads: Vec<J>,
    /// each pad's net, and its box (mm), in the order of `pads`
    pub pad_net: Vec<String>,
    pub pad_box: Vec<(f64, f64, f64, f64)>,
    pub holes: Vec<J>,
    pub parts: Vec<J>,
    /// a footprint's uuid -> the name its part goes by
    pub ref_of: HashMap<String, String>,
    /// Edge.Cuts drawn in footprints
    pub outline: Vec<J>,
    pub tracks: Vec<J>,
    pub zones: Vec<J>,
    /// surface pads on an inner layer only: copper the router must keep off, on that layer
    pub keepouts: Vec<J>,
    /// what footprints draw on the Margin layer
    pub margins: Vec<super::shapes::Drawn>,
    /// "REF.NUM" of every castellated pad; the pads a footprint ties together
    pub castellated: Vec<J>,
    pub ties: Vec<J>,
    /// the footprints' own lists, for what else reads them (their zones)
    pub ids: Vec<Id>,
}

/// A pad's copper: the scene's shape and the box round it, mm.
struct Copper {
    shape: J,
    bx: (f64, f64, f64, f64),
}

fn grow(b: (f64, f64, f64, f64), r: f64) -> (f64, f64, f64, f64) {
    (b.0 - r, b.1 - r, b.2 + r, b.3 + r)
}

/// Corners given in the pad's own frame (nm, about its centre), turned and put on the board, mm.
fn corners(c: P, deg: f64, local: &[(f64, f64)]) -> Vec<(f64, f64)> {
    local
        .iter()
        .map(|&q| {
            let r = rotf(q, deg);
            ((c.0 as f64 + r.0) / 1e6, (c.1 as f64 + r.1) / 1e6)
        })
        .collect()
}

fn poly_shape(pts: Vec<(f64, f64)>, r: f64) -> Copper {
    let bx = grow(bbox(&pts).unwrap_or((0.0, 0.0, 0.0, 0.0)), r);
    Copper { shape: obj(vec![("poly", poly(&pts)), ("r", n(r))]), bx }
}

fn circle_shape(c: P, r: i64) -> Copper {
    let (x, y, r) = (mm(c.0), mm(c.1), mm(r));
    Copper { shape: obj(vec![("circle", J::Arr(vec![n(x), n(y), n(r)]))]), bx: (x - r, y - r, x + r, y + r) }
}

/// A stadium: the pad's long axis less its width, grown by half its width.
fn oval_shape(c: P, w: i64, h: i64, deg: f64) -> Copper {
    if w == h {
        return circle_shape(c, w / 2);
    }
    let half = (w - h).abs() as f64 / 2.0;
    let d = rotf(if w > h { (half, 0.0) } else { (0.0, half) }, deg);
    let (a, e) = (((c.0 as f64 - d.0) / 1e6, (c.1 as f64 - d.1) / 1e6), ((c.0 as f64 + d.0) / 1e6, (c.1 as f64 + d.1) / 1e6));
    let r = mm(w.min(h)) / 2.0;
    Copper { shape: obj(vec![("oval", J::Arr(vec![n(a.0), n(a.1), n(e.0), n(e.1), n(r)]))]), bx: grow((a.0.min(e.0), a.1.min(e.1), a.0.max(e.0), a.1.max(e.1)), r) }
}

fn rect_local(w: f64, h: f64) -> Vec<(f64, f64)> {
    vec![(-w / 2.0, -h / 2.0), (w / 2.0, -h / 2.0), (w / 2.0, h / 2.0), (-w / 2.0, h / 2.0)]
}

impl Board {
    /// The copper of one pad on the board. `custom_hulls` counts the custom pads given as a hull.
    fn pad_copper(&self, pad: Id, shape: &str, c: P, w: i64, h: i64, deg: f64, custom_hulls: &mut usize) -> Copper {
        let d = &self.doc;
        let (wf, hf) = (w as f64, h as f64);
        match shape {
            "circle" => circle_shape(c, w / 2),
            "oval" => oval_shape(c, w, h, deg),
            "roundrect" if d.find(pad, "chamfer").is_none() => {
                let r = (w.min(h) as f64 * d.num_of(pad, "roundrect_rratio").unwrap_or(0.25)).round();
                if r * 2.0 >= w.min(h) as f64 {
                    return oval_shape(c, w, h, deg);
                }
                poly_shape(corners(c, deg, &rect_local(wf - 2.0 * r, hf - 2.0 * r)), r / 1e6)
            }
            "roundrect" => {
                // chamfered: the named corners cut, the others left sharp (KiCad rounds them by the
                // rratio: the true copper is inside this by less than a third of that radius)
                let cut = (w.min(h) as f64 * d.num_of(pad, "chamfer_ratio").unwrap_or(0.0)).round();
                let ch = d.find(pad, "chamfer").unwrap();
                let (x, y) = (wf / 2.0, hf / 2.0);
                let mut p = Vec::new();
                let mut corner = |name: &str, sharp: (f64, f64), a: (f64, f64), e: (f64, f64)| {
                    if d.has_word(ch, name) && cut > 0.0 {
                        p.push(a);
                        p.push(e);
                    } else {
                        p.push(sharp);
                    }
                };
                corner("top_left", (-x, -y), (-x, -y + cut), (-x + cut, -y));
                corner("top_right", (x, -y), (x - cut, -y), (x, -y + cut));
                corner("bottom_right", (x, y), (x, y - cut), (x - cut, y));
                corner("bottom_left", (-x, y), (-x + cut, y), (-x, y - cut));
                poly_shape(corners(c, deg, &p), 0.0)
            }
            "trapezoid" => {
                let dl = d.find(pad, "rect_delta").map_or((0.0, 0.0), |r| (nm(d.arg_num(r, 0).unwrap_or(0.0)) as f64 / 2.0, nm(d.arg_num(r, 1).unwrap_or(0.0)) as f64 / 2.0));
                let (x, y) = (wf / 2.0, hf / 2.0);
                poly_shape(corners(c, deg, &[(-x - dl.1, y + dl.0), (x + dl.1, y - dl.0), (x - dl.1, -y + dl.0), (-x + dl.1, -y - dl.0)]), 0.0)
            }
            "custom" => {
                let f = Frame { at: c, deg };
                let prims: Vec<Drawn> = d.find(pad, "primitives").map_or(Vec::new(), |p| d.lists(p).filter_map(|k| drawn(self, k, &f)).collect());
                let round = d.find(pad, "options").and_then(|o| d.str_of(o, "anchor")).as_deref() == Some("circle");
                // (a round anchor as the 24 sides that hold its circle)
                let ra = wf / 2.0 / (std::f64::consts::PI / 24.0).cos();
                let anchor = if round { (0..24).map(|k| (k as f64 * std::f64::consts::TAU / 24.0).sin_cos()).map(|(sn, cs)| (ra * cs, ra * sn)).collect() } else { rect_local(wf, hf) };
                let anchor = corners(c, deg, &anchor);
                if let [Drawn { geo: Geo::Poly(p), width, .. }] = prims.as_slice() {
                    let (cx, cy) = (mm(c.0), mm(c.1));
                    if p.len() >= 3 && anchor.iter().all(|a| inside(p, (cx + (a.0 - cx) * 0.98, cy + (a.1 - cy) * 0.98))) {
                        return poly_shape(p.clone(), mm(*width) / 2.0);
                    }
                }
                if prims.is_empty() {
                    return if round { circle_shape(c, w / 2) } else { poly_shape(anchor, 0.0) };
                }
                // several pieces: the scene has one shape a pad, so the hull round them all
                *custom_hulls += 1;
                let mut all = anchor;
                for p in &prims {
                    // a stroke's half width round each of its points, as the octagon that holds the circle
                    let r = mm(p.width) / 2.0 / (std::f64::consts::PI / 8.0).cos();
                    for q in p.points() {
                        if r > 0.0 {
                            all.extend((0..8).map(|k| ((k as f64 + 0.5) * std::f64::consts::FRAC_PI_4).sin_cos()).map(|(sn, cs)| (q.0 + r * cs, q.1 + r * sn)));
                        } else {
                            all.push(q);
                        }
                    }
                }
                poly_shape(hull(all), 0.0)
            }
            // rect, and anything a newer KiCad may add: the pad's rectangle
            _ => poly_shape(corners(c, deg, &rect_local(wf, hf)), 0.0),
        }
    }

    /// Every footprint of the board.
    pub fn feet(&mut self) -> Feet {
        let mut out = Feet::default();
        let d = &self.doc;
        let mut notes: Vec<String> = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        let (mut custom_hulls, mut inner_smd, mut npth_copper, mut slots) = (0usize, 0usize, 0usize, 0usize);
        let mut bridges = 0usize;
        let last = self.copper.len() - 1;
        let board_mask = d.find(self.root, "setup").and_then(|st| self.len(st, "pad_to_mask_clearance")).unwrap_or(0);
        for fp in d.lists(self.root).filter(|k| matches!(d.head(*k), "footprint" | "module")) {
            out.ids.push(fp);
            // where it is: (at x y deg), or the house format's (transform (translate x y) (rotate deg) (scale sx sy))
            let xy = |a: Id| (nm(d.arg_num(a, 0).unwrap_or(0.0)), nm(d.arg_num(a, 1).unwrap_or(0.0)));
            let frame = match (d.find(fp, "at"), d.find(fp, "transform")) {
                (Some(a), _) => Frame { at: xy(a), deg: d.arg_num(a, 2).unwrap_or(0.0) },
                (None, Some(tr)) => {
                    if d.find(tr, "scale").map_or(false, |sc| d.arg_num(sc, 0) != Some(1.0) || d.arg_num(sc, 1) != Some(1.0)) {
                        notes.push("a footprint is scaled: it is read at its own size".into());
                    }
                    Frame { at: d.find(tr, "translate").map_or((0, 0), xy), deg: d.num_of(tr, "rotate").unwrap_or(0.0) }
                }
                _ => Frame::BOARD,
            };
            let pos = frame.at;
            let back = self.cu(&self.layer_of(fp)) == Some(last);
            let uuid = d.str_of(fp, "uuid").or_else(|| d.str_of(fp, "tstamp")).unwrap_or_default().into_owned();
            // the reference: a property (KiCad 8 on) or a text (before)
            let mut name = d.all(fp, "property").find(|p| d.arg_str(*p, 0).as_deref() == Some("Reference")).and_then(|p| d.arg_str(p, 1)).map(|r| r.into_owned());
            if name.is_none() {
                name = d.all(fp, "fp_text").find(|t| d.arg_str(*t, 0).as_deref() == Some("reference")).and_then(|t| d.arg_str(t, 1)).map(|r| r.into_owned());
            }
            // every footprint is a part of its own: a second one of a name is NAME~2, one with no name #uuid
            let mut name = name.unwrap_or_default();
            if name.is_empty() {
                name = format!("#{}", if uuid.is_empty() { out.parts.len().to_string() } else { uuid.clone() });
                notes.push("footprints without a reference are named by their id (#uuid)".into());
            }
            let count = seen.entry(name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                notes.push(format!("the reference {} is used by more than one footprint: the later ones are {}~2 and so on", name, name));
                name = format!("{}~{}", name, count);
            }
            if !uuid.is_empty() {
                out.ref_of.insert(uuid, name.clone());
            }
            let fp_clear = self.len(fp, "clearance").unwrap_or(0);
            let fp_mask = self.len(fp, "solder_mask_margin");
            let (fp_join, fp_gap) = (d.num_of(fp, "zone_connect"), self.len(fp, "thermal_gap"));
            let (mut has_hole, first_pad) = (false, out.pads.len());

            for pad in d.all(fp, "pad") {
                let num = d.arg_str(pad, 0).unwrap_or_default().into_owned();
                let kind = d.arg_str(pad, 1).unwrap_or_default().into_owned();
                let shape = d.arg_str(pad, 2).unwrap_or_default().into_owned();
                let pa = d.find(pad, "at");
                let local: P = pa.map_or((0, 0), |a| (nm(d.arg_num(a, 0).unwrap_or(0.0)), nm(d.arg_num(a, 1).unwrap_or(0.0))));
                let deg = pa.and_then(|a| d.arg_num(a, 2)).unwrap_or(0.0);
                let hole_at = frame.put(local);
                let size = d.find(pad, "size");
                let w = size.and_then(|z| d.arg_num(z, 0)).map_or(0, nm);
                let h = size.and_then(|z| d.arg_num(z, 1)).map_or(w, nm);
                // (drill [oval] d [d2] [(offset x y)])
                let (mut dr, mut off): (Vec<i64>, P) = (Vec::new(), (0, 0));
                if let Some(dl) = d.find(pad, "drill") {
                    for k in d.kids(dl).skip(1) {
                        if d.head(k) == "offset" {
                            off = (nm(d.arg_num(k, 0).unwrap_or(0.0)), nm(d.arg_num(k, 1).unwrap_or(0.0)));
                        } else if let Ok(v) = d.raw(k).parse::<f64>() {
                            dr.push(nm(v));
                        }
                    }
                }
                let (dx, dy) = (dr.first().copied().unwrap_or(0), dr.get(1).or(dr.first()).copied().unwrap_or(0));
                has_hole |= matches!(kind.as_str(), "thru_hole" | "np_thru_hole");
                // a non-plated hole whose pad is larger than the hole on a copper layer has copper round it
                let ringed = kind == "np_thru_hole" && w > dx.max(dy) && h > dx.max(dy) && !self.cu_layers(pad).is_empty();
                if kind == "np_thru_hole" {
                    // a hole that asks its own clearance (a module's mounting hole with a 1.7 mm keep-clear
                    // ring) is given that much larger: the scene's holes have one clearance for all
                    let own = self.len(pad, "clearance").filter(|c| *c > 0).unwrap_or(fp_clear).max(0);
                    let (dx, dy) = if dx > 0 { (dx + 2 * own, dy + 2 * own) } else { (dx, dy) };
                    if dx > 0 {
                        if dx == dy {
                            out.holes.push(J::Arr(vec![len(hole_at.0), len(hole_at.1), len(dx)]));
                        } else {
                            // a slot: circles of its width along it, their rim within a micron of the slot's
                            slots += 1;
                            let (wd, half) = (dx.min(dy), (dx - dy).abs() as f64 / 2.0);
                            let steps = ((2.0 * half / (wd as f64 * 0.12)).ceil() as i64).clamp(1, 2000);
                            for k in 0..=steps {
                                let t = -half + 2.0 * half * k as f64 / steps as f64;
                                let q = rot(if dx > dy { (t.round() as i64, 0) } else { (0, t.round() as i64) }, deg);
                                out.holes.push(J::Arr(vec![len(hole_at.0 + q.0), len(hole_at.1 + q.1), len(wd)]));
                            }
                        }
                    }
                    // the hole is all there is of it, unless it has copper: that is a pad of no net
                    if !ringed {
                        continue;
                    }
                    npth_copper += 1;
                }
                let o = rot(off, deg);
                let c = (hole_at.0 + o.0, hole_at.1 + o.1);
                let cu = self.pad_copper(pad, &shape, c, w, h, deg, &mut custom_hulls);
                let on = self.cu_layers(pad);
                let net = self.net_of(pad);
                let tht = kind == "thru_hole" || (on.contains(&0) && on.contains(&last));
                if !tht && !on.contains(&0) && !on.contains(&last) {
                    // a surface pad on an inner layer: copper there and nothing a router can end on
                    if let (Some(&l), Some(p)) = (on.first(), cu.shape.get("poly").arr().first().map(|_| cu.shape.get("poly").clone())) {
                        inner_smd += 1;
                        out.keepouts.push(obj(vec![("name", s(&format!("pad {}.{} on {}", name, num, self.copper[l]))), ("layers", J::Arr(vec![s(&self.copper[l])])), ("poly", p), ("forbid", J::Arr(vec![s("tracks"), s("vias")]))]));
                    }
                    continue;
                }
                let mut kv = vec![("ref", s(&name)), ("num", s(&num)), ("net", s(&net)), ("at", pt(c)), ("fp", pt(pos)), ("size", J::Arr(vec![len(w), len(h)])), ("tht", J::Bool(tht))];
                if !tht && !on.contains(&0) {
                    kv.push(("side", s("B")));
                }
                let is_circle = cu.shape.has("circle");
                kv.push(("shape", cu.shape));
                let mut clear = self.len(pad, "clearance").filter(|v| *v > 0).unwrap_or(fp_clear);
                // the pad's opening in the solder mask is wider than the pad by its margin, and copper of
                // another net under that opening is a bridge to KiCad: other nets keep that far off
                let bared = d.find(pad, "layers").map_or(false, |l| d.kids(l).skip(1).any(|k| d.val(k).ends_with(".Mask")));
                let margin = self.len(pad, "solder_mask_margin").or(fp_mask).unwrap_or(board_mask);
                if bared && margin > 0 {
                    clear = clear.max(margin + 10_000);
                }
                if clear > 0 {
                    kv.push(("clearance", len(clear)));
                }
                if dx > 0 {
                    kv.push(("drill", len(dx.max(dy))));
                }
                // how a pour joins the pad, where the pad or its footprint says so over the zone
                let join = d.num_of(pad, "zone_connect").or(fp_join);
                if let Some(word) = join.and_then(|v| ["none", "thermal", "solid", "thermal_tht"].get(v as usize).copied()) {
                    kv.push(("pour", s(word)));
                }
                if let Some(g) = self.len(pad, "thermal_gap").or(fp_gap).filter(|g| *g > 0) {
                    kv.push(("thermal_gap", len(g)));
                }
                // KiCad's own default: cornerwise from a round pad, along the sides of any other
                let round = shape == "circle" || (shape == "custom" && d.find(pad, "options").and_then(|o| d.str_of(o, "anchor")).as_deref() == Some("circle"));
                if let Some(a) = d.num_of(pad, "thermal_bridge_angle").or(if round { Some(45.0) } else { None }) {
                    // a round pad's shape does not say how the pad is turned, and its spokes turn with
                    // it: its angle is given on the board, a quarter turn being no turn at all
                    let a = if is_circle { (a + deg).rem_euclid(90.0) } else { a };
                    kv.push(("spoke_angle", J::Num(a)));
                }
                if d.all(pad, "property").any(|p| d.arg_str(p, 0).as_deref() == Some("pad_prop_castellated")) {
                    out.castellated.push(s(&format!("{}.{}", name, num)));
                }
                out.pads.push(obj(kv));
                out.pad_net.push(net);
                out.pad_box.push(cu.bx);
            }

            // what the footprint draws: outline, copper, courtyard
            let (mut own, mut any): (Vec<(f64, f64)>, Vec<(f64, f64)>) = (Vec::new(), Vec::new());
            for g in d.lists(fp).filter(|k| d.head(*k).starts_with("fp_")) {
                let Some(dr) = drawn(self, g, &frame) else { continue };
                if dr.layer == "Edge.Cuts" {
                    out.outline.extend(dr.path());
                } else if dr.layer == "Margin" {
                    out.margins.push(dr);
                } else if self.cu(&dr.layer).is_some() {
                    // copper drawn round one of the footprint's own pads is that pad's net's
                    let pts = dr.points();
                    let touches: Vec<usize> = (first_pad..out.pads.len())
                        .filter(|&i| {
                            let bx = grow(out.pad_box[i], mm(dr.width) / 2.0);
                            let side_ok = out.pads[i].get("tht").b() || (out.pads[i].get("side").s() == Some("B")) == (self.cu(&dr.layer) == Some(last));
                            side_ok && (pts.iter().any(|p| p.0 >= bx.0 && p.0 <= bx.2 && p.1 >= bx.1 && p.1 <= bx.3) || (matches!(dr.geo, Geo::Poly(_)) && inside(&pts, ((bx.0 + bx.2) / 2.0, (bx.1 + bx.3) / 2.0))))
                        })
                        .collect();
                    // copper that joins pads of two nets is the footprint's own bridge (a solder jumper
                    // drawn closed). Given whole to one of them it would shut the other's pad in, so each
                    // pad gets the part of it that is nearer to it than to the others
                    if touches.iter().any(|&i| out.pad_net[i] != out.pad_net[touches[0]]) {
                        bridges += 1;
                        let mid: Vec<(f64, f64)> = touches.iter().map(|&i| ((out.pad_box[i].0 + out.pad_box[i].2) / 2.0, (out.pad_box[i].1 + out.pad_box[i].3) / 2.0)).collect();
                        for area in dr.areas() {
                            for (k, &i) in touches.iter().enumerate() {
                                let mut part = area.clone();
                                for m in (0..touches.len()).filter(|&m| m != k) {
                                    part = nearer(&part, mid[k], mid[m]);
                                }
                                if part.len() >= 3 {
                                    out.zones.push(obj(vec![("net", s(&out.pad_net[i])), ("layer", s(&dr.layer)), ("outline", poly(&part)), ("fill", J::Arr(vec![obj(vec![("outline", poly(&part))])])), ("min_width", J::Num(0.0)), ("locked", J::Bool(true))]));
                                }
                            }
                        }
                        continue;
                    }
                    dr.copper(touches.first().map_or("", |&i| out.pad_net[i].as_str()), true, &mut out.tracks, &mut out.zones);
                } else if dr.layer == "F.CrtYd" || dr.layer == "B.CrtYd" {
                    let r = mm(dr.width) / 2.0;
                    if let Some(bx) = bbox(&dr.points()) {
                        let bx = grow(bx, r);
                        any.extend([(bx.0, bx.1), (bx.2, bx.3)]);
                        if (dr.layer == "B.CrtYd") == back {
                            own.extend([(bx.0, bx.1), (bx.2, bx.3)]);
                        }
                    }
                }
            }
            let mut part = vec![("ref", s(&name))];
            if let Some(bx) = bbox(if own.is_empty() { &any } else { &own }).filter(|b| b.2 > b.0 && b.3 > b.1) {
                part.push(("courtyard", box_poly(bx)));
            }
            if back {
                part.push(("side", s("B")));
            }
            if self.locked(fp) || has_hole {
                part.push(("fixed", J::Bool(true)));
            }
            out.parts.push(obj(part));
            for t in d.all(fp, "net_tie_pad_groups") {
                for g in d.kids(t).skip(1) {
                    let pads: Vec<J> = d.val(g).split(',').map(|p| s(p.trim())).collect();
                    out.ties.push(obj(vec![("ref", s(&name)), ("pads", J::Arr(pads))]));
                }
            }
        }
        if custom_hulls > 0 {
            notes.push(format!("{} custom pads are made of several pieces: each is given as the hull round its pieces", custom_hulls));
        }
        if inner_smd > 0 {
            notes.push(format!("{} surface pads are on an inner layer only: they are kept off as copper there, not joined", inner_smd));
        }
        if bridges > 0 {
            notes.push(format!("{} drawings in copper join pads of two nets (a solder jumper drawn closed): each pad is given the part nearer to it", bridges));
        }
        if npth_copper > 0 {
            notes.push(format!("{} non-plated holes have copper round them: read as the hole and a pad of no net", npth_copper));
        }
        if slots > 0 {
            notes.push(format!("{} non-plated slots are given as rows of round holes", slots));
        }
        notes.into_iter().for_each(|x| self.note(x));
        out
    }
}

/// The box round every pad and courtyard, mm: a board with no outline is given this, grown.
pub fn extent(feet: &Feet) -> Option<(f64, f64, f64, f64)> {
    let mut pts: Vec<(f64, f64)> = feet.pad_box.iter().flat_map(|b| [(b.0, b.1), (b.2, b.3)]).collect();
    for p in &feet.parts {
        for q in p.get("courtyard").arr() {
            let v = q.nums();
            pts.push((v[0], v[1]));
        }
    }
    for h in &feet.holes {
        let v = h.nums();
        pts.extend([(v[0] - v[2] / 2.0, v[1] - v[2] / 2.0), (v[0] + v[2] / 2.0, v[1] + v[2] / 2.0)]);
    }
    bbox(&pts)
}
