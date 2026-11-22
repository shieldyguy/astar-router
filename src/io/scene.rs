//! A board file as a scene 2 (docs/scene.md): everything the reader found, put together.

use super::board::{n, obj, s, Board};
use super::copper::cover;
use super::foot::extent;
use super::project::Project;
use super::shapes::bbox;
use crate::json::J;
use std::collections::{HashMap, HashSet};

/// A board that was read: the scene as JSON, the file's tree (the writer lays the plan on it), and
/// what the reader has to say.
pub struct Read {
    pub scene: J,
    pub board: Board,
    pub notes: Vec<String>,
    /// what the board asks that was not read and KiCad may hold the routed board to (`exit::UNSURE`)
    pub doubts: Vec<String>,
}

/// Does a net's name say ground? (`GND`, `/power/GND`, `GNDA`, `AGND`, `VSS`)
fn says_ground(name: &str) -> bool {
    let last = name.rsplit('/').next().unwrap_or(name).to_ascii_uppercase();
    last.starts_with("GND") || last.ends_with("GND") || last == "VSS" || last == "0V"
}

/// The nets that may be the other half of a differential pair with this one as the positive half,
/// each with the pair's name: by KiCad's own rule (a `P` or `+` before any digits and underscores at
/// the name's end pairs with `N` or `-`), and the USB habit of `DP` with `DM`.
fn pair_of(name: &str) -> Vec<(String, String)> {
    let c: Vec<char> = name.chars().collect();
    let mut k = c.len();
    while k > 0 && (c[k - 1].is_ascii_digit() || c[k - 1] == '_') {
        k -= 1;
    }
    if k == 0 || !matches!(c[k - 1], 'P' | '+') {
        return Vec::new();
    }
    let (head, tail): (String, String) = (c[..k - 1].iter().collect(), c[k..].iter().collect());
    let pair = format!("{}{}", head.trim_end_matches('_'), tail);
    let mut out = vec![(pair.clone(), format!("{}{}{}", head, if c[k - 1] == 'P' { 'N' } else { '-' }, tail))];
    if c[k - 1] == 'P' && head.ends_with('D') {
        out.push((pair, format!("{}M{}", head, tail)));
    }
    out
}

fn opt(kv: &mut Vec<(&'static str, J)>, key: &'static str, v: Option<f64>) {
    if let Some(v) = v {
        kv.push((key, n(v)));
    }
}

impl Board {
    /// The stackup, top to bottom, where the board has one: copper, what lies between, the masks.
    fn stackup(&self) -> Vec<J> {
        let d = &self.doc;
        let Some(st) = d.find(self.root, "setup").and_then(|x| d.find(x, "stackup")) else { return Vec::new() };
        let mut out = Vec::new();
        let mut copper = Vec::new();
        for l in d.all(st, "layer") {
            let name = d.arg_str(l, 0).unwrap_or_default().into_owned();
            let ty = d.str_of(l, "type").unwrap_or_default().to_ascii_lowercase();
            let kind = if ty == "copper" {
                "copper"
            } else if ty == "core" || ty == "prepreg" {
                if ty == "core" { "core" } else { "prepreg" }
            } else if ty.contains("solder mask") {
                "mask"
            } else {
                continue;
            };
            let name = if kind == "copper" { self.layer(&name).to_string() } else { name };
            if kind == "copper" {
                copper.push(name.clone());
            }
            // a dielectric made of several sheets: their sum
            let thick: f64 = d.all(l, "thickness").filter_map(|t| d.arg_num(t, 0)).filter(|v| *v >= 0.0 && *v <= 100.0).sum();
            let mut kv = vec![("name", s(&name)), ("type", s(kind)), ("thickness", n(thick.min(100.0)))];
            opt(&mut kv, "epsilon_r", d.num_of(l, "epsilon_r").filter(|e| (1.0..=100.0).contains(e)));
            opt(&mut kv, "loss_tangent", d.num_of(l, "loss_tangent").filter(|e| (0.0..=1.0).contains(e)));
            if let Some(m) = d.str_of(l, "material").filter(|_| kind != "copper") {
                kv.push(("material", s(&m)));
            }
            out.push(obj(kv));
        }
        // a stackup that is not the board's layers (a file edited by hand) is not passed on
        if copper != self.copper {
            return Vec::new();
        }
        out
    }
}

/// The board at `path` read into a scene. The rules come from the `.kicad_pro` and `.kicad_dru`
/// beside it, where they are.
pub fn read(path: &str) -> Result<Read, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read: {}", e))?;
    let file = std::path::Path::new(path).file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned());
    from_text(text, Project::beside(path)?, &file)
}

/// A board's text read into a scene, with the rules it is given.
pub fn from_text(text: String, mut pro: Project, file: &str) -> Result<Read, String> {
    let mut b = Board::open(text)?;
    let feet = b.feet();
    let rest = b.rest(&feet);

    // the rules: the Default class, held up by the board's minimums
    let dc = pro.default_class().clone();
    let track = dc.track.unwrap_or(0.2).max(pro.min_track);
    let mut clearance = dc.clearance.unwrap_or(0.2).max(pro.min_clearance);
    if clearance < 0.1 - 1e-9 {
        // a board whose rules ask next to nothing between nets (a Default class left at 0) would have
        // copper laid touching: no fab makes that
        b.note(format!("the board asks {} mm between nets: 0.1 mm is kept, the least a fab makes", clearance));
        clearance = 0.1;
    }
    let drill = dc.drill.unwrap_or(0.3).max(pro.min_drill);
    let via = dc.via.unwrap_or(0.6).max(pro.min_via).max(drill + 2.0 * pro.min_annular);
    let edge = (if pro.edge > 0.0 { pro.edge } else { clearance }).max(pro.dru_edge);
    let hole_clearance = (if pro.hole_clearance > 0.0 { pro.hole_clearance } else { clearance }).max(pro.dru_hole);
    if !pro.found_pro {
        b.note("no .kicad_pro beside the board: the rules are KiCad's defaults (track 0.2, clearance 0.2, via 0.6/0.3, edge 0.5)".into());
    }

    // the outline; a board without one (a block on its own) gets the box round its parts
    let mut outline: Vec<J> = rest.outline.iter().chain(feet.outline.iter()).cloned().collect();
    if outline.is_empty() {
        let Some(e) = extent(&feet) else { return Err("the board has no outline and no pads".into()) };
        let m = edge + 1.0;
        let (x0, y0, x1, y1) = (e.0 - m, e.1 - m, e.2 + m, e.3 + m);
        for (a, c) in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))] {
            outline.push(obj(vec![("line", J::Arr(vec![n(a.0), n(a.1), n(c.0), n(c.1)]))]));
        }
        b.note(format!("the board has no outline on Edge.Cuts: the box round its pads and courtyards, grown by {} mm (the edge rule and 1 mm), is taken as the outline", crate::json::num(m)));
    }
    let ends: Vec<(f64, f64)> = outline.iter().flat_map(|q| if q.has("line") { q.get("line").nums() } else { q.get("arc").nums() }.chunks(2).map(|c| (c[0], c[1])).collect::<Vec<_>>()).collect();
    let ob = bbox(&ends).unwrap_or((0.0, 0.0, 0.0, 0.0));

    // the nets: in the order pads name them, then those only copper names
    let mut count: HashMap<&str, usize> = HashMap::new();
    let mut seen: Vec<&str> = Vec::new();
    for name in feet.pad_net.iter().filter(|x| !x.is_empty()) {
        let c = count.entry(name).or_insert(0);
        if *c == 0 {
            seen.push(name);
        }
        *c += 1;
    }
    let mut copper_only: Vec<String> = Vec::new();
    for c in rest.tracks.iter().chain(&rest.vias).chain(&rest.zones).chain(&feet.tracks).chain(&feet.zones) {
        let name = c.get("net").s().unwrap_or("");
        if !name.is_empty() && !count.contains_key(name) && !copper_only.iter().any(|x| x == name) {
            copper_only.push(name.to_string());
        }
    }

    // planes: a zone on an inner layer that covers half the board or more makes that layer its net's
    // plane. A net has one plane: the larger, then the lower. A pour on an outer layer is no plane: it
    // shares the layer with the parts and their tracks and refills round them, so the layer stays open to
    // routing, and the pour's net is routed like any other and leans on the fill (`route::fill`). Every zone is in `copper`.
    let (bw, bh) = (ob.2 - ob.0, ob.3 - ob.1);
    let mut planes: Vec<(String, String, f64)> = Vec::new();
    let inner = |layer: &str| b.cu(layer).map_or(false, |l| l > 0 && l + 1 < b.copper.len());
    for (net, layer, area) in &rest.pours {
        if !inner(layer) || cover(*area, bw, bh) < 0.5 {
            continue;
        }
        match planes.iter_mut().find(|p| p.0 == *net) {
            Some(p) => {
                if (inner(layer), *area, b.cu(layer)) > (inner(&p.1), p.2, b.cu(&p.1)) {
                    *p = (net.clone(), layer.clone(), *area);
                }
            }
            None => planes.push((net.clone(), layer.clone(), *area)),
        }
    }

    // ground: the plane whose name says ground (the largest), else the net whose name does (the one
    // with the most pads), else the largest plane
    let by_area = |a: &&(String, String, f64), c: &&(String, String, f64)| a.2.partial_cmp(&c.2).unwrap_or(std::cmp::Ordering::Equal);
    let ground: String = planes
        .iter()
        .filter(|p| says_ground(&p.0))
        .max_by(by_area)
        .map(|p| p.0.clone())
        .or_else(|| seen.iter().filter(|x| x.eq_ignore_ascii_case("GND")).chain(seen.iter().filter(|x| says_ground(x))).max_by_key(|x| (x.eq_ignore_ascii_case("GND"), count[**x])).map(|x| x.to_string()))
        .or_else(|| planes.iter().max_by(by_area).map(|p| p.0.clone()))
        .unwrap_or_default();

    // each net's class, and what it has of its own
    let all: Vec<String> = seen.iter().map(|x| x.to_string()).chain(copper_only.iter().cloned()).collect();
    let class_ix: Vec<usize> = all.iter().map(|x| pro.class_of(x)).collect();
    let dflt_track = dc.track.unwrap_or(0.2);
    let net_json = |i: usize| -> (J, bool) {
        let c = &pro.classes[class_ix[i]];
        let name = &all[i];
        let mut kv = vec![("name", s(name))];
        let rail = *name != ground && c.track.map_or(false, |t| t > dflt_track);
        if *name == ground {
            kv.push(("role", s("ground")));
        } else if rail {
            kv.push(("role", s("rail")));
        }
        if class_ix[i] != 0 {
            kv.push(("class", s(&c.name)));
            opt(&mut kv, "width", c.track.map(|t| t.max(pro.min_track)).filter(|t| *t != track));
            opt(&mut kv, "clearance", c.clearance.filter(|v| *v > clearance + 1e-9));
            opt(&mut kv, "via", c.via.filter(|v| *v != via));
            opt(&mut kv, "drill", c.drill.filter(|v| *v != drill));
        }
        (obj(kv), rail)
    };
    // the order the house export lists them in, so that both ways to a scene give the router the
    // same board: the nets to join by name, then the rails as met, then ground; then the rest
    let joined = |i: usize| count.get(all[i].as_str()).map_or(false, |c| *c >= 2) && !all[i].starts_with("unconnected");
    let made: Vec<(J, bool)> = (0..all.len()).map(net_json).collect();
    let mut signals: Vec<usize> = (0..all.len()).filter(|&i| joined(i) && !made[i].1 && all[i] != ground).collect();
    signals.sort_by(|a, c| all[*a].as_bytes().cmp(all[*c].as_bytes()));
    let rails: Vec<usize> = (0..all.len()).filter(|&i| joined(i) && made[i].1).collect();
    let mut order: Vec<usize> = signals.into_iter().chain(rails).collect();
    order.extend((0..all.len()).filter(|&i| all[i] == ground));
    let placed: HashSet<usize> = order.iter().copied().collect();
    order.extend((0..all.len()).filter(|i| !placed.contains(i)));
    let nets: Vec<J> = order.iter().map(|&i| made[i].0.clone()).collect();

    // pairs, by name
    let known: HashSet<&str> = all.iter().map(|x| x.as_str()).collect();
    let mut pairs = Vec::new();
    for name in &all {
        if let Some((pair, other)) = pair_of(name).into_iter().find(|(_, o)| known.contains(o.as_str()) && o != name) {
            {
                let c = &pro.classes[class_ix[all.iter().position(|x| x == name).unwrap()]];
                let mut kv = vec![("name", s(&pair)), ("p", s(name)), ("n", s(&other))];
                opt(&mut kv, "width", c.pair_width);
                opt(&mut kv, "gap", c.pair_gap);
                pairs.push(obj(kv));
            }
        }
    }

    // classes, and the custom rules that could be read
    let classes: Vec<J> = pro
        .classes
        .iter()
        .map(|c| {
            let mut kv = vec![("name", s(&c.name))];
            opt(&mut kv, "track", c.track);
            opt(&mut kv, "clearance", c.clearance);
            opt(&mut kv, "via", c.via);
            opt(&mut kv, "drill", c.drill);
            opt(&mut kv, "pair_width", c.pair_width);
            opt(&mut kv, "pair_gap", c.pair_gap);
            obj(kv)
        })
        .collect();
    let (nl, mut layer_rules, mut class_pairs, mut unread) = (b.copper.len(), Vec::<(usize, Option<f64>, Option<f64>)>::new(), Vec::new(), pro.unread.clone());
    for r in &pro.custom {
        let layers: Option<Vec<usize>> = match r.layer.as_deref() {
            None => Some((0..nl).collect()),
            Some("inner") => Some((1..nl - 1).collect()),
            Some("outer") => Some(vec![0, nl - 1]),
            Some(l) => b.cu(l).map(|l| vec![l]),
        };
        let classes_known = r.classes.iter().all(|c| pro.classes.iter().any(|k| k.name == *c));
        match (layers, r.classes.len()) {
            (Some(ls), 0) => {
                for l in ls {
                    match layer_rules.iter_mut().find(|x| x.0 == l) {
                        Some(x) => *x = (l, r.track.or(x.1), r.clearance.or(x.2)),
                        None => layer_rules.push((l, r.track, r.clearance)),
                    }
                }
            }
            (Some(_), k) if r.layer.is_none() && classes_known && r.clearance.is_some() => {
                let others: Vec<&str> = if k == 2 { vec![r.classes[1].as_str()] } else { pro.classes.iter().map(|c| c.name.as_str()).collect() };
                for o in others {
                    class_pairs.push(obj(vec![("a", s(&r.classes[0])), ("b", s(o)), ("clearance", n(r.clearance.unwrap()))]));
                }
            }
            _ => unread.push(super::project::Unread { name: r.name.clone(), asks: r.clearance.map(|v| ("clearance".to_string(), Some(v))).into_iter().chain(r.track.map(|v| ("track_width".to_string(), Some(v)))).collect() }),
        }
    }
    // a rule that was not read and may ask more than the router keeps anyway is a doubt about the routed
    // board, not a note: KiCad will hold the board to it
    let narrowest = pro.classes.iter().filter_map(|c| c.track).fold(track, f64::min).max(pro.min_track).min(if pro.min_track > 0.0 { pro.min_track } else { f64::MAX });
    let kept = super::project::Kept { clearance, track: narrowest, via: pro.min_via.min(via), drill: pro.min_drill.min(drill), annular: pro.min_annular.min((via - drill) / 2.0) };
    let (more, less): (Vec<_>, Vec<_>) = unread.iter().partition(|u| u.may_ask_more(&kept));
    let list = |v: &[&super::project::Unread]| format!("{}{}", v.iter().take(12).map(|u| u.said()).collect::<Vec<_>>().join("; "), if v.len() > 12 { format!(" and {} more", v.len() - 12) } else { String::new() });
    if !less.is_empty() {
        b.note(format!("{} custom rules of the .kicad_dru are not read; they ask no more than is kept anyway: {}", less.len(), list(&less)));
    }
    if !more.is_empty() {
        b.doubt(format!("{} custom rules of the .kicad_dru are not read, and may ask more than astar keeps: {}", more.len(), list(&more)));
    }
    let odd = pro.odd_patterns(&all);
    if !odd.is_empty() {
        b.note(format!("net class patterns read as wildcards that match no net (regular expressions are not read): {}", odd.join(" ")));
    }

    let mut rules = vec![("track", n(track)), ("clearance", n(clearance)), ("via", n(via)), ("drill", n(drill)), ("edge", n(edge)), ("hole_clearance", n(hole_clearance))];
    let h2h = pro.hole_to_hole.max(pro.dru_hole_to_hole);
    rules.push(("hole_to_hole", n(h2h)));
    if pro.min_spokes != 2.0 {
        rules.push(("min_spokes", n(pro.min_spokes)));
    }
    {
        let mut f = Vec::new();
        // the narrowest track the board allows: a neck at a fine-pitch pad may go down to it. A board that
        // names none has KiCad's own default, 0.2 mm, which is what its DRC will hold the tracks to
        opt(&mut f, "track", Some(pro.min_track).filter(|v| *v > 0.0));
        if pro.found_pro {
            opt(&mut f, "clearance", Some(pro.min_clearance).filter(|v| *v > 0.0));
            opt(&mut f, "via", Some(pro.min_via));
            opt(&mut f, "drill", Some(pro.min_drill));
            opt(&mut f, "annular", Some(pro.min_annular));
            opt(&mut f, "hole_to_hole", Some(pro.hole_to_hole));
            opt(&mut f, "edge", Some(pro.edge).filter(|v| *v > 0.0));
        }
        if !f.is_empty() {
            rules.push(("floors", obj(f)));
        }
    }
    if !layer_rules.is_empty() {
        layer_rules.sort_by_key(|x| x.0);
        let list = layer_rules
            .iter()
            .map(|(l, t, c)| {
                let mut kv = vec![("layer", s(&b.copper[*l]))];
                opt(&mut kv, "track", *t);
                opt(&mut kv, "clearance", *c);
                obj(kv)
            })
            .collect();
        rules.push(("layers", J::Arr(list)));
    }

    let mut stack = vec![("copper", J::Arr(b.copper.iter().map(|l| s(l)).collect())), ("planes", J::Arr(planes.iter().map(|p| obj(vec![("net", s(&p.0)), ("layer", s(&p.1))])).collect()))];
    let stackup = b.stackup();
    if !stackup.is_empty() {
        stack.push(("layers", J::Arr(stackup)));
    }

    let mut keepouts = rest.keepouts;
    keepouts.extend(feet.keepouts);
    // a line on the Margin layer is an edge to KiCad's edge rule, as Edge.Cuts is: no copper within the
    // edge clearance of it, on any layer
    for m in rest.margins.iter().chain(&feet.margins) {
        for area in m.band(edge) {
            keepouts.push(obj(vec![
                ("name", s("the margin line: copper keeps the edge clearance from it")),
                ("layers", J::Arr(b.copper.iter().map(|l| s(l)).collect())),
                ("poly", J::Arr(area.iter().map(|p| J::Arr(vec![n(p.0), n(p.1)])).collect())),
                ("forbid", J::Arr(vec![s("tracks"), s("vias"), s("zones")])),
            ]));
        }
    }
    let mut tracks = rest.tracks;
    tracks.extend(feet.tracks);
    let mut zones = rest.zones;
    zones.extend(feet.zones);
    // a custom rule about pours alone: every pour keeps at least that from other nets
    if pro.dru_zone > 0.0 {
        for z in zones.iter_mut() {
            if let J::Obj(kv) = z {
                match kv.iter_mut().find(|(k, _)| k == "clearance") {
                    Some((_, J::Num(c))) => *c = c.max(pro.dru_zone),
                    _ => kv.push(("clearance".to_string(), n(pro.dru_zone))),
                }
            }
        }
    }
    let notes = b.notes.clone();
    let mut source = vec![("board", s(file)), ("format", n(b.version as f64)), ("read_by", s("astar")), ("notes", J::Arr(notes.iter().map(|x| s(x)).collect()))];
    let doubts = b.doubts.clone();
    if !doubts.is_empty() {
        source.push(("doubts", J::Arr(doubts.iter().map(|x| s(x)).collect())));
    }
    if !feet.castellated.is_empty() {
        source.push(("castellated", J::Arr(feet.castellated)));
    }
    if !feet.ties.is_empty() {
        source.push(("net_ties", J::Arr(feet.ties)));
    }
    let scene = obj(vec![
        ("version", n(2.0)),
        ("units", s("mm")),
        ("source", obj(source)),
        ("stack", obj(stack)),
        ("outline", J::Arr(outline)),
        ("rules", obj(rules)),
        ("classes", J::Arr(classes)),
        ("class_pairs", J::Arr(class_pairs)),
        ("nets", J::Arr(nets)),
        ("pads", J::Arr(feet.pads)),
        ("holes", J::Arr(feet.holes)),
        ("keepouts", J::Arr(keepouts)),
        ("copper", obj(vec![("tracks", J::Arr(tracks)), ("vias", J::Arr(rest.vias)), ("zones", J::Arr(zones))])),
        ("parts", J::Arr(feet.parts)),
        ("pairs", J::Arr(pairs)),
        ("groups", J::Arr(rest.groups)),
    ]);
    Ok(Read { scene, board: b, notes, doubts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(says_ground("GND") && says_ground("/pwr/GNDA") && says_ground("AGND") && says_ground("vss"));
        assert!(!says_ground("SIGNAL") && !says_ground("+5V"));
        assert_eq!(pair_of("USB_P"), vec![("USB".to_string(), "USB_N".to_string())]);
        assert_eq!(pair_of("D+"), vec![("D".to_string(), "D-".to_string())]);
        assert_eq!(pair_of("LVDS_P3"), vec![("LVDS3".to_string(), "LVDS_N3".to_string())]);
        assert_eq!(pair_of("MCU_USB_DP")[1], ("MCU_USB_D".to_string(), "MCU_USB_DM".to_string()));
        assert!(pair_of("USB_N").is_empty() && pair_of("RESET").is_empty());
    }
}
