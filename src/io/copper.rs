//! What is on the board besides the footprints: the outline it draws, the copper already there
//! (tracks, arcs, vias, zones with their fill), the rule areas, the groups.

use super::board::{len, n, obj, poly, pt, s, Board};
use super::foot::Feet;
use super::num::{area, keyhole, Frame};
use super::shapes::{drawn, pts_of};
use super::sexpr::Id;
use crate::json::J;

#[derive(Default)]
pub struct Rest {
    pub outline: Vec<J>,
    pub tracks: Vec<J>,
    pub vias: Vec<J>,
    pub zones: Vec<J>,
    pub keepouts: Vec<J>,
    /// what the board draws on the Margin layer
    pub margins: Vec<super::shapes::Drawn>,
    pub groups: Vec<J>,
    /// every zone with a net: (net, layer, the area of its outline in mm2), in file order
    pub pours: Vec<(String, String, f64)>,
}

impl Board {
    /// An opening drawn in the solder mask. Copper of two nets under one opening is a bridge to KiCad, so
    /// no track or via goes under it, but for the one net whose pads it bares: its tracks must still
    /// reach them. An opening that bares pads of two nets is the board's own affair.
    fn mask_opening(&self, dr: &super::shapes::Drawn, feet: &super::foot::Feet, out: &mut Rest) {
        let back = dr.layer == "B.Mask";
        let cu = if back { &self.copper[self.copper.len() - 1] } else { &self.copper[0] };
        for a in dr.areas() {
            let (x0, y0, x1, y1) = a.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1)));
            let mut nets: Vec<&str> = Vec::new();
            for (i, b) in feet.pad_box.iter().enumerate() {
                let on = feet.pads[i].get("tht").b() || (feet.pads[i].get("side").s() == Some("B")) == back;
                if on && b.0 < x1 && b.2 > x0 && b.1 < y1 && b.3 > y0 && !nets.contains(&feet.pad_net[i].as_str()) {
                    nets.push(feet.pad_net[i].as_str());
                }
            }
            // and the copper the board draws with a net (a test point drawn as a rectangle)
            let drawn_nets: Vec<String> = out
                .zones
                .iter()
                .filter(|z| z.get("locked").b() && z.get("layer").s() == Some(cu.as_str()) && !z.get("net").s().unwrap_or("").is_empty())
                .filter(|z| {
                    let p: Vec<Vec<f64>> = z.get("outline").arr().iter().map(|q| q.nums()).collect();
                    let b = p.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, q| (b.0.min(q[0]), b.1.min(q[1]), b.2.max(q[0]), b.3.max(q[1])));
                    b.0 < x1 && b.2 > x0 && b.1 < y1 && b.3 > y0
                })
                .map(|z| z.get("net").s().unwrap_or("").to_string())
                .collect();
            for n in &drawn_nets {
                if !nets.contains(&n.as_str()) {
                    nets.push(n.as_str());
                }
            }
            if nets.len() > 1 {
                continue;
            }
            // (the stroke round a closed opening is open mask too: it reaches half a stroke past its points)
            for part in std::iter::once(a.clone()).chain(dr.rim()) {
                let mut kv = vec![("name", s("an opening drawn in the solder mask")), ("layers", J::Arr(vec![s(cu)])), ("poly", poly(&part)), ("forbid", J::Arr(vec![s("tracks"), s("vias")]))];
                if let Some(n) = nets.first().filter(|n| !n.is_empty()) {
                    kv.push(("net", s(n)));
                }
                out.keepouts.push(obj(kv));
            }
        }
    }

    /// One `(zone ...)`: a rule area becomes keep-outs, a copper zone one zone a layer and outline.
    fn zone(&self, z: Id, frame: &Frame, out: &mut Rest, quiet: &mut (usize, usize)) {
        let d = &self.doc;
        let layers = self.cu_layers(z);
        // the first polygon is the zone's outline, the others are holes in it (as KiCad reads them)
        let mut rings = d.all(z, "polygon").map(|p| pts_of(self, p, frame)).filter(|p| p.len() >= 3);
        let Some(outline) = rings.next() else { return };
        let holes: Vec<Vec<(f64, f64)>> = rings.collect();
        let name = d.str_of(z, "name").unwrap_or_default().into_owned();
        if let Some(k) = d.find(z, "keepout") {
            let no = |what: &str| d.str_of(k, what).as_deref() == Some("not_allowed");
            let forbid: Vec<J> = [("tracks", "tracks"), ("vias", "vias"), ("copperpour", "zones"), ("pads", "pads"), ("footprints", "footprints")].iter().filter(|(k, _)| no(k)).map(|(_, v)| s(v)).collect();
            // a router lays tracks and vias, and drops vias into planes: an area that keeps out none of
            // tracks, vias and pours is not its business
            if !no("tracks") && !no("vias") && !no("copperpour") {
                quiet.0 += 1;
                return;
            }
            if layers.is_empty() {
                return;
            }
            let mut kv = vec![("layers", J::Arr(layers.iter().map(|&l| s(&self.copper[l])).collect())), ("poly", poly(&keyhole(&outline, &holes))), ("forbid", J::Arr(forbid))];
            if !name.is_empty() {
                kv.insert(0, ("name", s(&name)));
            }
            out.keepouts.push(obj(kv));
            return;
        }
        let net = self.net_of(z);
        let cp = d.find(z, "connect_pads");
        let pads = match cp.and_then(|c| d.arg(c, 0)).filter(|a| d.kind(*a) == super::sexpr::Kind::Atom).map(|a| d.raw(a)) {
            Some("yes") => "solid",
            Some("thru_hole_only") => "thermal_tht",
            Some("no") => "none",
            _ => "thermal",
        };
        for &l in &layers {
            let layer = &self.copper[l];
            let fill: Vec<J> = d
                .all(z, "filled_polygon")
                .filter(|f| self.cu(&self.layer_of(*f)).map_or(layers.len() == 1, |fl| fl == l))
                .map(|f| pts_of(self, f, frame))
                .filter(|p| p.len() >= 3)
                .map(|p| obj(vec![("outline", poly(&p))]))
                .collect();
            {
                let mut kv = vec![("net", s(&net)), ("layer", s(layer)), ("outline", poly(&outline))];
                if !holes.is_empty() {
                    kv.push(("holes", J::Arr(holes.iter().map(|h| poly(h)).collect())));
                }
                if !fill.is_empty() {
                    kv.push(("fill", J::Arr(fill.clone())));
                }
                // (which of two zones is filled first: the priority, then the id; `rest` turns the two
                // into one number)
                kv.push(("priority", n(d.num_of(z, "priority").unwrap_or(0.0))));
                kv.push(("~id", s(&d.str_of(z, "uuid").or_else(|| d.str_of(z, "tstamp")).unwrap_or_default())));
                if let Some(c) = cp.and_then(|c| self.len(c, "clearance")) {
                    kv.push(("clearance", len(c)));
                }
                if let Some(m) = self.len(z, "min_thickness") {
                    kv.push(("min_width", len(m)));
                }
                kv.push(("pads", s(pads)));
                if let Some(f) = d.find(z, "fill") {
                    if let Some(g) = self.len(f, "thermal_gap") {
                        kv.push(("thermal_gap", len(g)));
                    }
                    if let Some(w) = self.len(f, "thermal_bridge_width") {
                        kv.push(("spoke_width", len(w)));
                    }
                }
                if self.locked(z) {
                    kv.push(("locked", J::Bool(true)));
                }
                out.zones.push(obj(kv));
                if !net.is_empty() && d.find(z, "attr").is_none() {
                    out.pours.push((net.clone(), layer.clone(), area(&outline) - holes.iter().map(|h| area(h)).sum::<f64>()));
                }
            }
        }
    }

    /// Everything on the board that is not a footprint (and the footprints' own zones).
    pub fn rest(&mut self, feet: &Feet) -> Rest {
        let mut out = Rest::default();
        let d = &self.doc;
        let (first, last) = (0usize, self.copper.len() - 1);
        let mut quiet = (0usize, 0usize);
        // what is drawn on the mask layers, looked at once all the copper is known
        let mut openings: Vec<super::shapes::Drawn> = Vec::new();
        let (mut texts, mut odd) = (0usize, 0usize);
        for k in d.lists(self.root) {
            match d.head(k) {
                "segment" | "arc" => {
                    let (Some(a), Some(e), Some(w)) = (self.xy(k, "start"), self.xy(k, "end"), self.len(k, "width")) else { continue };
                    let layer = self.layer_of(k);
                    if self.cu(&layer).is_none() || w <= 0 {
                        odd += 1;
                        continue;
                    }
                    let piece = match self.xy(k, "mid") {
                        Some(m) => obj(vec![("arc", J::Arr([a, m, e].iter().flat_map(|p| [len(p.0), len(p.1)]).collect()))]),
                        None => obj(vec![("line", J::Arr(vec![len(a.0), len(a.1), len(e.0), len(e.1)]))]),
                    };
                    let mut kv = vec![("net", s(&self.net_of(k))), ("layer", s(&layer)), ("width", len(w)), ("path", J::Arr(vec![piece]))];
                    if self.locked(k) {
                        kv.push(("locked", J::Bool(true)));
                    }
                    out.tracks.push(obj(kv));
                }
                "via" => {
                    let (Some(at), Some(size)) = (self.xy(k, "at"), self.len(k, "size")) else { continue };
                    if size <= 0 {
                        odd += 1;
                        continue;
                    }
                    let mut kv = vec![("net", s(&self.net_of(k))), ("at", pt(at)), ("size", len(size)), ("drill", len(self.len(k, "drill").unwrap_or(0)))];
                    let span = self.cu_layers(k);
                    if let (Some(&a), Some(&e)) = (span.first(), span.last()) {
                        if (a, e) != (first, last) {
                            kv.push(("layers", J::Arr(vec![s(&self.copper[a]), s(&self.copper[e])])));
                        }
                    }
                    if self.locked(k) {
                        kv.push(("locked", J::Bool(true)));
                    }
                    out.vias.push(obj(kv));
                }
                "zone" => self.zone(k, &Frame::BOARD, &mut out, &mut quiet),
                "gr_text" | "gr_text_box" => {
                    let layer = self.layer_of(k);
                    if self.cu(&layer).is_none() {
                        continue;
                    }
                    // text written in copper: its box is copper nobody may cross
                    match self.text_box(k) {
                        Some(b) => out.zones.push(obj(vec![
                            ("net", s("")),
                            ("layer", s(&layer)),
                            ("outline", poly(&b)),
                            ("fill", J::Arr(vec![obj(vec![("outline", poly(&b))])])),
                            ("min_width", J::Num(0.0)),
                            ("locked", J::Bool(true)),
                        ])),
                        None => texts += 1,
                    }
                }
                "group" => {
                    let parts: Vec<J> = d.find(k, "members").map_or(Vec::new(), |m| d.kids(m).skip(1).filter_map(|u| feet.ref_of.get(d.val(u).as_ref())).map(|r| s(r)).collect());
                    if !parts.is_empty() {
                        out.groups.push(obj(vec![("name", s(&d.arg_str(k, 0).unwrap_or_default())), ("parts", J::Arr(parts))]));
                    }
                }
                h if h.starts_with("gr_") => {
                    let Some(dr) = drawn(self, k, &Frame::BOARD) else { continue };
                    if dr.layer == "Edge.Cuts" {
                        out.outline.extend(dr.path());
                    } else if self.cu(&dr.layer).is_some() {
                        dr.copper(&self.net_of(k), false, &mut out.tracks, &mut out.zones);
                    } else if dr.layer == "F.Mask" || dr.layer == "B.Mask" {
                        openings.push(dr);
                    } else if dr.layer == "Margin" {
                        out.margins.push(dr);
                    }
                }
                _ => {}
            }
        }
        // rule areas a footprint carries: in board coordinates where the footprint is placed by `at`, in
        // the footprint's own where it is placed by the house format's `transform`
        for &fp in &feet.ids {
            let frame = match d.find(fp, "transform") {
                Some(tr) if d.find(fp, "at").is_none() => Frame { at: d.find(tr, "translate").map_or((0, 0), |a| (super::num::nm(d.arg_num(a, 0).unwrap_or(0.0)), super::num::nm(d.arg_num(a, 1).unwrap_or(0.0)))), deg: d.num_of(tr, "rotate").unwrap_or(0.0) },
                _ => Frame::BOARD,
            };
            for z in d.all(fp, "zone") {
                self.zone(z, &frame, &mut out, &mut quiet);
            }
            // and the openings it draws in the solder mask
            let own = match (d.find(fp, "at"), d.find(fp, "transform")) {
                (Some(a), _) => Frame { at: (super::num::nm(d.arg_num(a, 0).unwrap_or(0.0)), super::num::nm(d.arg_num(a, 1).unwrap_or(0.0))), deg: d.arg_num(a, 2).unwrap_or(0.0) },
                _ => Frame { at: frame.at, deg: frame.deg },
            };
            for g in d.lists(fp).filter(|k| d.head(*k).starts_with("fp_") && !matches!(d.head(*k), "fp_text" | "fp_text_box")) {
                if matches!(self.layer_of(g).as_str(), "F.Mask" | "B.Mask") {
                    if let Some(dr) = drawn(self, g, &own) {
                        openings.push(dr);
                    }
                }
            }
            texts += d.all(fp, "fp_text").filter(|t| self.cu(&self.layer_of(*t)).is_some()).count();
        }
        // KiCad fills the zone of higher priority first and, of two with the same, the one whose id is
        // the greater: the scene's priority says both, as the zone's place in that order
        let key = |z: &J| (z.get("priority").f_or(0.0), z.get("~id").s().unwrap_or("").to_string());
        let mut order: Vec<(f64, String)> = out.zones.iter().filter(|z| z.has("~id")).map(key).collect();
        order.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        order.dedup();
        let tied = order.windows(2).any(|w| w[0].0 == w[1].0);
        for z in out.zones.iter_mut() {
            if !z.has("~id") {
                continue;
            }
            let rank = order.iter().position(|k| *k == key(z)).unwrap_or(0);
            if let J::Obj(kv) = z {
                kv.retain(|(k, _)| k != "~id");
                for (k, v) in kv.iter_mut() {
                    if k == "priority" && tied {
                        *v = n(rank as f64);
                    }
                }
                // (a board whose zones all differ in priority keeps its own numbers; 0 is not written)
                kv.retain(|(k, v)| !(k == "priority" && v.f() == Some(0.0)));
            }
        }
        for dr in &openings {
            self.mask_opening(dr, feet, &mut out);
        }
        let mut notes = Vec::new();
        if quiet.0 > 0 {
            notes.push(format!("{} rule areas keep out neither tracks nor vias (only zones, pads or footprints): not in the scene", quiet.0));
        }
        if texts > 0 {
            notes.push(format!("{} texts are written in copper: not read", texts));
        }
        if odd > 0 {
            notes.push(format!("{} tracks or vias have no width or are on a layer the board does not have: not read", odd));
        }
        notes.into_iter().for_each(|x| self.note(x));
        out
    }
}

impl Board {
    /// The box a board-level text takes, in mm: from its words, its font size, its angle and how it
    /// is justified. A little generous: it stands for the strokes, which the file does not hold.
    fn text_box(&self, k: Id) -> Option<Vec<(f64, f64)>> {
        let d = &self.doc;
        let text = d.arg_str(k, 0)?;
        let at = d.find(k, "at")?;
        let (x, y) = (d.arg_num(at, 0)?, d.arg_num(at, 1)?);
        let rot = d.arg_num(at, 2).unwrap_or(0.0);
        let fx = d.find(k, "effects")?;
        let font = d.find(fx, "font")?;
        let size = d.find(font, "size")?;
        let (h, w) = (d.arg_num(size, 0)?, d.arg_num(size, 1)?);
        let thick = d.num_of(font, "thickness").unwrap_or(0.15 * h);
        let lines: Vec<&str> = text.split("\\n").flat_map(|l| l.split('\n')).collect();
        let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64;
        if longest == 0.0 {
            return None;
        }
        let (bw, bh) = (longest * w * 1.05 + thick, lines.len() as f64 * h * 1.62 + thick);
        // where the box sits about its anchor
        let (mut ox, mut oy) = (-bw / 2.0, -bh / 2.0);
        if let Some(j) = d.find(fx, "justify") {
            if d.has_word(j, "left") {
                ox = 0.0;
            } else if d.has_word(j, "right") {
                ox = -bw;
            }
            if d.has_word(j, "top") {
                oy = 0.0;
            } else if d.has_word(j, "bottom") {
                oy = -bh;
            }
            if d.has_word(j, "mirror") {
                ox = -ox - bw;
            }
        }
        // KiCad turns text counter-clockwise for a positive angle, with y down
        let (sn, cs) = (-rot.to_radians()).sin_cos();
        Some([(ox, oy), (ox + bw, oy), (ox + bw, oy + bh), (ox, oy + bh)].iter().map(|&(px, py)| (x + px * cs - py * sn, y + px * sn + py * cs)).collect())
    }
}

/// How much of a box of this size (mm) an area covers, 0 to 1.
pub fn cover(area_mm2: f64, w: f64, h: f64) -> f64 {
    if w * h > 0.0 {
        (area_mm2 / (w * h)).min(1.0)
    } else {
        0.0
    }
}
