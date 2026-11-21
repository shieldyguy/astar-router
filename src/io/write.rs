//! The writer: a plan laid on the board it was made for. Tracks, arcs, vias and plane zones go in as
//! the file's last items, in the file's own dialect (nets by name or by number, `uuid` or `tstamp`,
//! one item a line or KiCad 8's indented form); copper the plan lifts goes by its id; everything else
//! is left byte for byte. New ids come from the plan and a seed, so the same run writes the same file.

use super::board::Board;
use super::num::{arc_pts, centre3};
use super::scene::Read;
use super::sexpr::{quote, Edits};
use crate::json::J;
use std::fmt::Write as _;

/// A number as KiCad writes it: six decimals at most, no trailing zeros.
fn num(v: f64) -> String {
    let t = format!("{:.6}", v);
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" || t.is_empty() {
        "0".into()
    } else {
        t.into()
    }
}

/// Ids for new items: the same plan and seed give the same ids.
struct Ids {
    state: u64,
}

impl Ids {
    fn new(plan: &str, seed: u64) -> Ids {
        // FNV-1a over the plan's copper, then the seed
        let mut h: u64 = 0xcbf29ce484222325;
        for b in plan.bytes().chain(seed.to_le_bytes()) {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        Ids { state: h }
    }
    fn word(&mut self) -> u64 {
        // splitmix64
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    /// A version 4 uuid.
    fn next(&mut self) -> String {
        let (a, b) = (self.word(), self.word());
        format!("{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}", a >> 32, (a >> 16) & 0xffff, a & 0xfff, 0x8000 | (b >> 48) & 0x3fff, b & 0xffff_ffff_ffff)
    }
}

/// How this file writes an item.
struct Dialect<'a> {
    b: &'a Board,
    /// KiCad 8's form: one value a line, tabs
    tall: bool,
    /// the name of an item's id: `uuid`, `tstamp`, or none (KiCad 5)
    id: &'static str,
}

impl<'a> Dialect<'a> {
    fn of(b: &'a Board) -> Dialect<'a> {
        let t = b.doc.text();
        let id = if t.contains("(uuid ") {
            "uuid"
        } else if t.contains("(tstamp ") {
            "tstamp"
        } else {
            ""
        };
        Dialect { b, tall: t.contains("\n\t("), id }
    }

    /// An item from its name and its values, in the file's form.
    fn item(&self, name: &str, parts: &[String]) -> String {
        if self.tall {
            let mut o = format!("\t({}", name);
            for p in parts {
                o.push_str("\n\t\t");
                o.push_str(&p.replace('\n', "\n\t\t"));
            }
            o.push_str("\n\t)\n");
            o
        } else {
            format!("  ({} {})\n", name, parts.iter().map(|p| p.split('\n').map(|l| l.trim()).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(" "))
        }
    }

    fn net(&self, name: &str) -> Result<String, String> {
        if !self.b.nets_by_number() {
            return Ok(format!("(net {})", quote(name)));
        }
        match self.b.net_number(name) {
            Some(k) => Ok(format!("(net {})", k)),
            None => Err(format!("the plan lays copper on net {}, which the board does not have", quote(name))),
        }
    }

    fn id(&self, ids: &mut Ids) -> Option<String> {
        let u = ids.next();
        match self.id {
            "" => None,
            "tstamp" => Some(format!("(tstamp {})", u)),
            _ => Some(format!("(uuid {})", quote(&u))),
        }
    }
}

/// The box round a scene's outline, mm.
fn outline_box(scene: &J) -> Option<(f64, f64, f64, f64)> {
    let mut pts = Vec::new();
    for q in scene.get("outline").arr() {
        if q.has("line") {
            pts.extend(q.get("line").nums().chunks(2).map(|c| (c[0], c[1])));
        } else {
            let v = q.get("arc").nums();
            if v.len() == 6 {
                pts.extend(arc_pts((v[0], v[1]), (v[2], v[3]), (v[4], v[5]), 0.01));
            }
        }
    }
    super::shapes::bbox(&pts)
}

/// What the writer laid, for the run's own lines.
#[derive(Default, Debug, PartialEq)]
pub struct Laid {
    pub tracks: usize,
    pub arcs: usize,
    pub vias: usize,
    pub zones: usize,
    pub lifted: usize,
    pub notes: Vec<String>,
}

/// The board's text with the plan laid on it. `plan` is the plan's JSON (`Routed::plan`); `seed`
/// changes the new items' ids and nothing else.
pub fn lay(rd: &Read, plan: &J, seed: u64) -> Result<(String, Laid), String> {
    let b = &rd.board;
    let d = &b.doc;
    let dia = Dialect::of(b);
    let mut laid = Laid::default();
    let copper_text = crate::json::write(&J::Arr(vec![plan.get("nets").clone(), plan.get("planes").clone()]));
    let mut ids = Ids::new(&copper_text, seed);
    let rules = rd.scene.get("rules");
    let (via, drill) = (rules.get("via").f_or(0.6), rules.get("drill").f_or(0.3));
    let (top, bottom) = (quote(&b.copper[0]), quote(&b.copper[b.copper.len() - 1]));
    if !plan.get("moves").arr().is_empty() {
        return Err("the plan moves parts: the writer lays copper and leaves parts where they are".into());
    }
    let mut out = String::new();
    let layer_of = |l: &J| -> Result<String, String> {
        let name = l.s().unwrap_or("");
        b.cu(name).map(|i| quote(&b.copper[i])).ok_or_else(|| format!("the plan names the layer {}, which the board does not have", quote(name)))
    };
    for net in plan.get("nets").arr() {
        let name = net.get("name").s().unwrap_or("");
        let net_ref = dia.net(name)?;
        for t in net.get("tracks").arr() {
            let layer = format!("(layer {})", layer_of(t.get("layer"))?);
            let width = format!("(width {})", num(t.get("width").f().filter(|w| *w > 0.0).ok_or("a track of the plan has no width")?));
            for q in t.get("path").arr() {
                let (name, mut parts) = if q.has("line") {
                    let v = q.get("line").nums();
                    if v.len() != 4 {
                        return Err("a line of the plan is not four numbers".into());
                    }
                    if (num(v[0]), num(v[1])) == (num(v[2]), num(v[3])) {
                        continue;
                    }
                    laid.tracks += 1;
                    ("segment", vec![format!("(start {} {})", num(v[0]), num(v[1])), format!("(end {} {})", num(v[2]), num(v[3]))])
                } else {
                    let v = q.get("arc").nums();
                    if v.len() != 6 {
                        return Err("an arc of the plan is not six numbers".into());
                    }
                    if (num(v[0]), num(v[1])) == (num(v[4]), num(v[5])) {
                        continue;
                    }
                    // an arc whose three points lie on a line is a line
                    if centre3((v[0], v[1]), (v[2], v[3]), (v[4], v[5])).is_none() {
                        laid.tracks += 1;
                        ("segment", vec![format!("(start {} {})", num(v[0]), num(v[1])), format!("(end {} {})", num(v[4]), num(v[5]))])
                    } else {
                        laid.arcs += 1;
                        ("arc", vec![format!("(start {} {})", num(v[0]), num(v[1])), format!("(mid {} {})", num(v[2]), num(v[3])), format!("(end {} {})", num(v[4]), num(v[5]))])
                    }
                };
                parts.extend([width.clone(), layer.clone(), net_ref.clone()]);
                parts.extend(dia.id(&mut ids));
                out.push_str(&dia.item(name, &parts));
            }
        }
        for v in net.get("vias").arr() {
            let at = v.get("at").nums();
            if at.len() != 2 {
                return Err("a via of the plan has no place".into());
            }
            // no protection token: a new via takes the board's own setup, as one placed in the editor does
            let mut parts = vec![format!("(at {} {})", num(at[0]), num(at[1])), format!("(size {})", num(v.get("size").f_or(via))), format!("(drill {})", num(v.get("drill").f_or(drill))), format!("(layers {} {})", top, bottom), net_ref.clone()];
            parts.extend(dia.id(&mut ids));
            out.push_str(&dia.item("via", &parts));
            laid.vias += 1;
        }
    }

    let mut edits = Edits::new();
    let bx = outline_box(&rd.scene);
    for q in plan.get("planes").arr() {
        // (a plane that is the net's host's is not this board's to draw)
        if q.get("host").b() {
            continue;
        }
        let (net, layer) = (q.get("net").s().unwrap_or(""), q.get("layer").s().unwrap_or(""));
        let li = b.cu(layer).ok_or_else(|| format!("the plan pours on the layer {}, which the board does not have", quote(layer)))?;
        // the board's own zone of this net on this layer alone gives way to the plan's
        for z in d.all(b.root, "zone") {
            if d.find(z, "keepout").is_none() && b.net_of(z) == net && b.cu_layers(z) == vec![li] {
                edits.remove(d, z);
                laid.lifted += 1;
            }
        }
        let whole = q.get("outline").arr().is_empty();
        let loops: Vec<Vec<(f64, f64)>> = if whole {
            let Some((x0, y0, x1, y1)) = bx else { continue };
            vec![vec![(x0 - 1.0, y0 - 1.0), (x1 + 1.0, y0 - 1.0), (x1 + 1.0, y1 + 1.0), (x0 - 1.0, y1 + 1.0)]]
        } else {
            q.get("outline").arr().iter().map(|l| l.arr().iter().map(|p| p.nums()).filter(|p| p.len() == 2).map(|p| (p[0], p[1])).collect()).collect()
        };
        if !q.get("holes").arr().is_empty() {
            laid.notes.push(format!("the plane of {} on {} has holes: they are not written, the zone is its outline", net, layer));
        }
        let prio = if whole { 0 } else { q.get("priority").f_or(1.0) as i64 };
        let pads = match q.get("pads").s().unwrap_or("solid") {
            "thermal" => "",
            "thermal_tht" => " thru_hole_only",
            _ => " yes",
        };
        for lp in loops.iter().filter(|l| l.len() >= 3) {
            let mut parts = Vec::new();
            if b.nets_by_number() {
                parts.push(dia.net(net)?);
                parts.push(format!("(net_name {})", quote(net)));
            } else {
                parts.push(dia.net(net)?);
            }
            parts.push(format!("(layer {})", quote(&b.copper[li])));
            parts.extend(dia.id(&mut ids));
            parts.push("(hatch edge 0.5)".into());
            if prio != 0 {
                parts.push(format!("(priority {})", prio));
            }
            parts.push(format!("(connect_pads{}\n\t(clearance 0.2)\n)", pads));
            parts.push(format!("(min_thickness {})", num(q.get("min_width").f_or(0.2))));
            parts.push("(fill yes\n\t(thermal_gap 0.3)\n\t(thermal_bridge_width 0.3)\n\t(island_removal_mode 0)\n)".into());
            let mut pts = String::from("(polygon\n\t(pts\n\t\t");
            for (k, p) in lp.iter().enumerate() {
                let _ = write!(pts, "{}(xy {} {})", if k > 0 { " " } else { "" }, num(p.0), num(p.1));
            }
            pts.push_str("\n\t)\n)");
            parts.push(pts);
            out.push_str(&dia.item("zone", &parts));
            laid.zones += 1;
        }
    }

    // copper the plan lifts, by its id
    let lift: Vec<&str> = plan.get("lift").arr().iter().filter_map(|x| x.s()).collect();
    if !lift.is_empty() {
        for k in d.lists(b.root).filter(|k| matches!(d.head(*k), "segment" | "arc" | "via" | "zone")) {
            let id = d.str_of(k, "uuid").or_else(|| d.str_of(k, "tstamp"));
            if id.map_or(false, |id| lift.contains(&id.as_ref())) {
                edits.remove(d, k);
                laid.lifted += 1;
            }
        }
    }
    if !out.is_empty() {
        edits.append(d, b.root, &out);
    }
    Ok((edits.apply(d)?, laid))
}

/// The files that go with a board (`.kicad_pro`, `.kicad_dru`, `.kicad_prl`) copied beside the
/// written one, unchanged. Returns the endings that were copied.
pub fn copy_siblings(from: &str, to: &str) -> Result<Vec<&'static str>, String> {
    let (a, b) = (from.strip_suffix(".kicad_pcb").unwrap_or(from), to.strip_suffix(".kicad_pcb").unwrap_or(to));
    let mut done = Vec::new();
    if a == b {
        return Ok(done);
    }
    for ext in [".kicad_pro", ".kicad_dru", ".kicad_prl"] {
        let src = format!("{}{}", a, ext);
        if std::path::Path::new(&src).is_file() {
            std::fs::copy(&src, format!("{}{}", b, ext)).map_err(|e| format!("cannot copy {}: {}", src, e))?;
            done.push(ext);
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_ids() {
        assert_eq!((num(1.0), num(-0.0000001), num(30.5499), num(0.1 + 0.2)), ("1".to_string(), "0".to_string(), "30.5499".to_string(), "0.3".to_string()));
        let (mut a, mut b, mut c) = (Ids::new("plan", 0), Ids::new("plan", 0), Ids::new("plan", 1));
        let (ua, ub, uc) = (a.next(), b.next(), c.next());
        assert_eq!(ua, ub);
        assert_ne!(ua, uc);
        assert_ne!(ua, a.next());
        assert_eq!(ua.len(), 36);
        assert_eq!(&ua[14..15], "4");
    }
}
