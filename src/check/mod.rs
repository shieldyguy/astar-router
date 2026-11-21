//! astar check: is this plan right for this scene? The independent check (plan/T13-proof.md, T13.7).
//!
//! Two questions, answered from the scene and the plan alone, with exact geometry:
//!
//! - **Joined.** Is every pad of every net joined by the plan's copper, the pads and the planes? Two
//!   pieces of one net are joined when their copper overlaps on a layer they share (KiCad's own rule:
//!   `ItemsTouchOnLayer`). A plane (the scene's `stack.planes`, the plan's `planes`) is taken as one
//!   piece of copper that reaches every through pad and via of its net, and its surface pads and
//!   tracks on its layer, inside its outline when it has one. What a refilled zone really reaches is
//!   KiCad's to say: this check does not fill zones.
//! - **Clear.** Is every piece of the plan's copper far enough from every other net's copper (the
//!   plan's, the pads, tracks and vias already on the board), from every hole, from the board edge,
//!   and out of every keep-out, by the scene's rules (`rules.rs`)? A zone's last fill is not held
//!   against the plan: KiCad fills it again round the plan's copper.
//!
//! It shares no code with the router: only `geom` (shapes, `seg_dist2`, `arc_points`), the scene
//! reader and the JSON reader. `astar check SCENE PLAN` prints one finding a line and exits 0 clean,
//! 1 with findings, 2 when it could not read its input.

pub mod dist;
pub mod plan;
mod rules;

use crate::field::{DynObj, Kind, ALL_LAYERS, NO_JOB};
use crate::geom::arc_points;
use crate::scene::{PathPiece, Scene};
use dist::{gap, in_loop, Pt, Solid};
use plan::{Piece, Plan};
use rules::Rules;
use std::collections::HashMap;

/// A shortfall below this is not a finding: KiCad's DRC lets half a micron pass too.
pub const EPS: f64 = 0.0005;
/// Two pieces of a net are joined when they overlap by a nanometre (KiCad's unit of length) or more.
pub const TOUCH: f64 = 1.0e-6;
/// An arc is checked as chords that stay within this of it.
const CHORD: f64 = 0.0001;
/// The scene reader gives the outline's arcs as chords within 0.005 mm: the edge is known that well.
const EDGE_EPS: f64 = 0.005;
const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Debug)]
enum What {
    Pad,
    Track,
    Via,
    /// an island of a zone as it was last filled (scene 2)
    Island,
    /// a plated hole: a via's, a pad's
    Hole,
    /// a hole without copper
    Npth,
    Edge,
    Keepout,
}

impl What {
    fn copper(self) -> bool {
        matches!(self, What::Pad | What::Track | What::Via | What::Island)
    }
}

struct Prim {
    what: What,
    /// laid by the plan (the rest is the board's own)
    plan: bool,
    net: u16,
    lmask: u32,
    solid: Solid,
    /// which piece of copper it belongs to, for joining; NONE for what is not copper
    node: u32,
    /// what to call it (an index into the labels)
    item: u32,
    /// a pad's own clearance; for a keep-out: 1 it forbids tracks, 2 vias, 3 both
    own: f64,
    /// where a pad or a via is
    at: Pt,
}

#[derive(Clone, Debug)]
pub struct Finding {
    /// open, open-unsaid, short, clearance, hole, hole-to-hole, edge, outside, keepout, plan
    pub kind: &'static str,
    pub net: String,
    /// the other net, when there is one
    pub other: String,
    pub layer: String,
    pub at: Option<Pt>,
    pub detail: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} {}", self.kind, self.net)?;
        if !self.other.is_empty() {
            write!(f, " / {}", self.other)?;
        }
        if !self.layer.is_empty() {
            write!(f, " on {}", self.layer)?;
        }
        if let Some((x, y)) = self.at {
            write!(f, " at ({:.4}, {:.4})", x, y)?;
        }
        write!(f, ": {}", self.detail)
    }
}

pub struct Report {
    pub findings: Vec<Finding>,
    /// the nets with two or more pads, and how many of them are joined
    pub nets: usize,
    pub joined: usize,
    /// what the check could not look at
    pub notes: Vec<String>,
}

impl Report {
    pub fn clean(&self) -> bool {
        self.findings.is_empty()
    }
    /// "3 clearance, 1 open-unsaid"
    pub fn tally(&self) -> String {
        let mut by: Vec<(&str, usize)> = Vec::new();
        for f in &self.findings {
            match by.iter_mut().find(|(k, _)| *k == f.kind) {
                Some(e) => e.1 += 1,
                None => by.push((f.kind, 1)),
            }
        }
        by.iter().map(|(k, n)| format!("{} {}", n, k)).collect::<Vec<_>>().join(", ")
    }
}

/// The check a run makes of its own plan before it says how it went: the lines to print, and how many
/// of the findings are faults (everything but a net the plan itself says is open).
pub fn of_run(scene_json: &crate::json::J, sc: &Scene, plan_text: &str) -> (Vec<String>, usize) {
    let plan = match crate::json::parse(plan_text).map_err(|e| e.to_string()).and_then(|j| Plan::from_json(&j).map_err(|e| e.to_string())) {
        Ok(p) => p,
        Err(e) => return (vec![format!("astar: its own check could not read the plan it wrote: {}", e)], 1),
    };
    let r = check(sc, &plan, &scene_planes(scene_json, sc));
    let faults: Vec<&Finding> = r.findings.iter().filter(|f| f.kind != "open").collect();
    if faults.is_empty() {
        return (Vec::new(), 0);
    }
    let mut by: Vec<(&str, usize)> = Vec::new();
    for f in &faults {
        match by.iter_mut().find(|(k, _)| *k == f.kind) {
            Some(e) => e.1 += 1,
            None => by.push((f.kind, 1)),
        }
    }
    let mut lines = vec![format!("astar: ITS OWN CHECK FINDS FAULT WITH THIS PLAN: {}. This is a fault in astar, not in the board.", by.iter().map(|(k, n)| format!("{} {}", n, k)).collect::<Vec<_>>().join(", "))];
    lines.extend(faults.iter().take(8).map(|f| format!("astar:   {}", f)));
    if faults.len() > 8 {
        lines.push(format!("astar:   and {} more (astar check SCENE PLAN lists them)", faults.len() - 8));
    }
    (lines, faults.len())
}

struct Sets(Vec<u32>);

impl Sets {
    fn find(&mut self, mut a: u32) -> u32 {
        while self.0[a as usize] != a {
            let up = self.0[self.0[a as usize] as usize];
            self.0[a as usize] = up;
            a = up;
        }
        a
    }
    fn join(&mut self, a: u32, b: u32) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b) as usize] = a.min(b);
        }
    }
}

/// Buckets of things by where they are.
struct Grid {
    x0: f64,
    y0: f64,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

impl Grid {
    fn new(bb: (f64, f64, f64, f64)) -> Grid {
        // about 400 cells across the longer side, none smaller than a millimetre
        let cell = ((bb.2 - bb.0).max(bb.3 - bb.1) / 400.0).max(1.0);
        let nx = ((bb.2 - bb.0) / cell).ceil() as usize + 1;
        let ny = ((bb.3 - bb.1) / cell).ceil() as usize + 1;
        Grid { x0: bb.0, y0: bb.1, cell, nx, ny, cells: vec![Vec::new(); nx * ny] }
    }
    fn ix(&self, x: f64) -> usize {
        ((x - self.x0) / self.cell).floor().clamp(0.0, (self.nx - 1) as f64) as usize
    }
    fn iy(&self, y: f64) -> usize {
        ((y - self.y0) / self.cell).floor().clamp(0.0, (self.ny - 1) as f64) as usize
    }
    /// The cells within `grow` of the piece, each once, into `out`.
    fn cells_of(&self, s: &Solid, grow: f64, out: &mut Vec<u32>) {
        out.clear();
        let boxed = |x0: f64, y0: f64, x1: f64, y1: f64, out: &mut Vec<u32>| {
            for y in self.iy(y0)..=self.iy(y1) {
                for x in self.ix(x0)..=self.ix(x1) {
                    out.push((y * self.nx + x) as u32);
                }
            }
        };
        match s {
            Solid::Cap { a, b, r } => {
                // along the segment, not its whole box: a long diagonal is in the cells it crosses
                let m = r + grow;
                let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                let n = (len / (self.cell * 0.5)).ceil().max(1.0) as usize;
                for k in 0..=n {
                    let t = k as f64 / n as f64;
                    let (x, y) = (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
                    boxed(x - m, y - m, x + m, y + m, out);
                }
            }
            Solid::Area { .. } => {
                let bb = s.bbox();
                boxed(bb.0 - grow, bb.1 - grow, bb.2 + grow, bb.3 + grow, out);
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

struct Build<'a> {
    sc: &'a Scene,
    prims: Vec<Prim>,
    labels: Vec<String>,
    nodes: u32,
}

impl<'a> Build<'a> {
    fn node(&mut self) -> u32 {
        self.nodes += 1;
        self.nodes - 1
    }
    fn label(&mut self, s: String) -> u32 {
        self.labels.push(s);
        self.labels.len() as u32 - 1
    }
    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, what: What, plan: bool, net: u16, lmask: u32, solid: Solid, node: u32, item: u32, own: f64, at: Pt) {
        self.prims.push(Prim { what, plan, net, lmask, solid, node, item, own, at });
    }
    /// A piece of track: one segment, or the chords of an arc, all one piece of copper.
    fn track(&mut self, plan: bool, net: u16, layer: usize, width: f64, line: Option<[f64; 4]>, arc: Option<[f64; 6]>, item: u32) {
        let node = self.node();
        let pts: Vec<Pt> = match (line, arc) {
            (Some(v), _) => vec![(v[0], v[1]), (v[2], v[3])],
            (_, Some(v)) => arc_points((v[0], v[1]), (v[2], v[3]), (v[4], v[5]), CHORD),
            _ => return,
        };
        for w in pts.windows(2) {
            self.add(What::Track, plan, net, 1 << layer, Solid::Cap { a: w[0], b: w[1], r: width / 2.0 }, node, item, 0.0, w[0]);
        }
    }
    /// A via: its copper on the layers of `lmask`, and its hole.
    fn via(&mut self, plan: bool, net: u16, lmask: u32, at: Pt, size: f64, drill: f64) {
        let name = &self.sc.nets[net as usize].name;
        let (copper, hole) = (self.label(format!("a via of {}", name)), self.label(format!("the hole of a via of {}", name)));
        let node = self.node();
        self.add(What::Via, plan, net, lmask, Solid::circle(at.0, at.1, size / 2.0), node, copper, 0.0, at);
        self.add(What::Hole, plan, net, lmask, Solid::circle(at.0, at.1, drill / 2.0), NONE, hole, 0.0, at);
    }
}

/// A router over the scene with the plan's planes as its own, for its model of the pours' fill; None for
/// a board without a pour to model (or one the router will not take).
fn fill_model(sc: &Scene, plan: &Plan) -> Option<crate::route::Router> {
    if !sc.copper.zones.iter().any(|z| z.net != 0 && z.outline.len() >= 3 && !(z.locked && z.min_width == Some(0.0))) {
        return None;
    }
    let mut sc = sc.clone();
    for q in plan.planes.iter().filter(|q| !q.host && q.outline.is_empty()) {
        if let (Some(n), Some(l)) = (sc.nets.iter().position(|n| n.name == q.net), sc.layers.iter().position(|l| *l == q.layer)) {
            if n != 0 && sc.nets[n].plane.is_none() {
                sc.nets[n].plane = Some(l);
            }
        }
    }
    let mut r = crate::route::Router::new(sc, crate::route::Cfg::default(), Vec::new()).ok()?;
    for ni in 1..r.sc.nets.len() {
        if r.sc.nets[ni].pads.len() >= 2 && r.sc.nets[ni].plane.is_none() {
            if let Some(base) = r.pour_base(ni as u16) {
                r.pours.insert(ni as u16, base);
            }
        }
    }
    Some(r)
}

/// The planes the scene declares, as (net, layer): every one of them, where `Scene` keeps one a net.
pub fn scene_planes(scene_json: &crate::json::J, sc: &Scene) -> Vec<(u16, usize)> {
    let mut out = Vec::new();
    for q in scene_json.get("stack").get("planes").arr() {
        let net = q.get("net").s().and_then(|n| sc.nets.iter().position(|x| x.name == n));
        let layer = q.get("layer").s().and_then(|l| sc.layers.iter().position(|x| x == l));
        if let (Some(n), Some(l)) = (net, layer) {
            if n != 0 && !out.contains(&(n as u16, l)) {
                out.push((n as u16, l));
            }
        }
    }
    out
}

struct PlaneNode {
    net: u16,
    layer: usize,
    /// its outer loop and the loops cut out of it; None: the whole board
    area: Option<(Vec<Pt>, Vec<Vec<Pt>>)>,
    node: u32,
}

/// The whole check. `planes`: the scene's declared planes (`scene_planes`).
pub fn check(sc: &Scene, plan: &Plan, planes: &[(u16, usize)]) -> Report {
    let rules = Rules::new(sc);
    let nl = sc.layers.len();
    let all: u32 = if nl >= 32 { u32::MAX } else { (1u32 << nl) - 1 };
    let mut b = Build { sc, prims: Vec::new(), labels: Vec::new(), nodes: 0 };
    let mut findings: Vec<Finding> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let bad_plan = |net: &str, detail: String| Finding { kind: "plan", net: net.to_string(), other: String::new(), layer: String::new(), at: None, detail };

    // the board's own: pads and their holes, bare holes, the edge, keep-outs, copper already there
    for (i, p) in sc.pads.iter().enumerate() {
        let lmask = if p.tht { all } else if p.back { 1 << (nl - 1) } else { 1 };
        let node = b.node();
        debug_assert_eq!(node as usize, i);
        let item = b.label(format!("pad {}.{}", p.ref_, p.num));
        b.add(What::Pad, false, p.net, lmask, Solid::of_shape(&p.shape), node, item, p.clearance, (p.x, p.y));
        if let Some(d) = p.drill.filter(|d| *d > 0.0) {
            let item = b.label(format!("the hole of pad {}.{}", p.ref_, p.num));
            b.add(What::Hole, false, p.net, all, Solid::circle(p.x, p.y, d / 2.0), NONE, item, 0.0, (p.x, p.y));
        }
    }
    let hole = b.label("a hole".into());
    for &(x, y, d) in &sc.holes {
        b.add(What::Npth, false, 0, all, Solid::circle(x, y, d / 2.0), NONE, hole, 0.0, (x, y));
    }
    let edge = b.label("the board edge".into());
    for &(ax, ay, bx, by) in &sc.outline {
        b.add(What::Edge, false, 0, all, Solid::Cap { a: (ax, ay), b: (bx, by), r: 0.0 }, NONE, edge, 0.0, (ax, ay));
    }
    for (i, k) in sc.keepouts.iter().enumerate() {
        let forbids = k.forbid.tracks as u8 + 2 * k.forbid.vias as u8;
        if forbids == 0 || k.poly.len() < 3 {
            continue;
        }
        let lmask = k.layers.iter().fold(0u32, |m, &l| m | (1 << l));
        let item = b.label(if k.name.is_empty() { format!("keep-out {}", i + 1) } else { format!("the keep-out \"{}\"", k.name) });
        // (its net: the one net it does not keep out)
        b.add(What::Keepout, false, k.net, lmask, Solid::Area { outer: k.poly.clone(), holes: Vec::new(), r: 0.0 }, NONE, item, forbids as f64, k.poly[0]);
    }
    for t in &sc.copper.tracks {
        let item = b.label(format!("a track already on the board ({})", sc.nets[t.net as usize].name));
        for piece in &t.path {
            match *piece {
                PathPiece::Line(v) => b.track(false, t.net, t.layer, t.width, Some(v), None, item),
                PathPiece::Arc(v) => b.track(false, t.net, t.layer, t.width, None, Some(v), item),
            }
        }
    }
    for v in &sc.copper.vias {
        let lmask = v.span.map_or(all, |(a, z)| (a.min(z)..=a.max(z)).fold(0u32, |m, l| m | (1 << l)));
        b.via(false, v.net, lmask, (v.x, v.y), v.size, v.drill);
    }
    // A pour's last fill is stale the moment the plan's copper lies, and a board that was never filled
    // has none. So the pours are filled again, round the copper as it now stands, by the router's own
    // model of KiCad's fill (`route::fill`): what that model covers is taken from it and not from the
    // scene. (KiCad's own fill is the last word; this is astar's best knowledge of it.)
    let mut filler = fill_model(sc, plan);
    for z in &sc.copper.zones {
        if filler.as_ref().map_or(false, |r| r.pours.contains_key(&z.net) && r.routable.contains(&z.layer) && !(z.locked && z.min_width == Some(0.0))) {
            continue;
        }
        let item = b.label(format!("a zone of {}", sc.nets[z.net as usize].name));
        for isl in z.fill.iter().filter(|i| i.outline.len() >= 3) {
            let node = b.node();
            b.add(What::Island, false, z.net, 1 << z.layer, Solid::Area { outer: isl.outline.clone(), holes: isl.holes.clone(), r: 0.0 }, node, item, 0.0, isl.outline[0]);
        }
    }

    // the plan's copper
    let net_of: HashMap<&str, u16> = sc.nets.iter().enumerate().skip(1).map(|(i, n)| (n.name.as_str(), i as u16)).collect();
    let layer_of: HashMap<&str, usize> = sc.layers.iter().enumerate().map(|(i, l)| (l.as_str(), i)).collect();
    for n in &plan.nets {
        let Some(&net) = net_of.get(n.name.as_str()) else {
            if !n.tracks.is_empty() || !n.vias.is_empty() {
                findings.push(bad_plan(&n.name, "the plan lays copper on a net the scene does not have".into()));
            }
            continue;
        };
        for t in &n.tracks {
            let Some(&layer) = layer_of.get(t.layer.as_str()) else {
                findings.push(bad_plan(&n.name, format!("a track on {}, which is not a layer of the scene", t.layer)));
                continue;
            };
            let item = b.label(format!("a track of {}", n.name));
            for piece in &t.path {
                match *piece {
                    Piece::Line(v) => b.track(true, net, layer, t.width, Some(v), None, item),
                    Piece::Arc(v) => b.track(true, net, layer, t.width, None, Some(v), item),
                }
            }
        }
        let (size, drill) = rules.via_of(net);
        for v in &n.vias {
            b.via(true, net, all, (v.x, v.y), v.size.unwrap_or(size), v.drill.unwrap_or(drill));
        }
    }

    // the planes: the plan's, and the scene's where the plan does not pour that net on that layer anew
    let mut plane_nodes: Vec<PlaneNode> = Vec::new();
    for q in &plan.planes {
        let (Some(&net), Some(&layer)) = (net_of.get(q.net.as_str()), layer_of.get(q.layer.as_str())) else {
            findings.push(bad_plan(&q.net, format!("a plane of a net or on a layer ({}) the scene does not have", q.layer)));
            continue;
        };
        if q.outline.is_empty() {
            let node = b.node();
            plane_nodes.push(PlaneNode { net, layer, area: None, node });
        }
        for (k, outer) in q.outline.iter().enumerate() {
            let node = b.node();
            plane_nodes.push(PlaneNode { net, layer, area: Some((outer.clone(), q.holes.get(k).cloned().unwrap_or_default())), node });
        }
    }
    for &(net, layer) in planes {
        if !plane_nodes.iter().any(|q| q.net == net && q.layer == layer) {
            let node = b.node();
            plane_nodes.push(PlaneNode { net, layer, area: None, node });
        }
    }

    // the pours as they fill now: a node for every group of pieces the model joins
    let mut fills: Vec<(u16, crate::route::fill::Pour, HashMap<u32, u32>)> = Vec::new();
    if let Some(r) = filler.as_mut() {
        for n in &plan.nets {
            let Some(&net) = net_of.get(n.name.as_str()) else { continue };
            let more = r.more_clr(net);
            for t in &n.tracks {
                let Some(&layer) = layer_of.get(t.layer.as_str()) else { continue };
                for piece in &t.path {
                    let pts: Vec<Pt> = match *piece {
                        Piece::Line(v) => vec![(v[0], v[1]), (v[2], v[3])],
                        Piece::Arc(v) => arc_points((v[0], v[1]), (v[2], v[3]), (v[4], v[5]), 0.005),
                    };
                    for w in pts.windows(2) {
                        r.f.add_obj(DynObj { kind: Kind::Track, net, job: NO_JOB, locked: false, layer: layer as u8, ax: w[0].0, ay: w[0].1, bx: w[1].0, by: w[1].1, r: t.width / 2.0 + more, alive: true });
                    }
                }
            }
            let (size, drill) = rules.via_of(net);
            for v in &n.vias {
                let vr = (v.size.unwrap_or(size) / 2.0).max(v.drill.unwrap_or(drill) / 2.0 + sc.rules.hole_clearance - sc.rules.clearance);
                r.f.add_obj(DynObj { kind: Kind::Via, net, job: NO_JOB, locked: false, layer: ALL_LAYERS, ax: v.x, ay: v.y, bx: v.x, by: v.y, r: vr + more, alive: true });
            }
        }
        let mut nets: Vec<u16> = r.pours.keys().copied().collect();
        nets.sort();
        for net in nets {
            let Some(pm) = r.pour(net) else { continue };
            let mut group_node: HashMap<u32, u32> = HashMap::new();
            for c in 1..pm.cells.len() {
                let next = b.nodes;
                if *group_node.entry(pm.comp_group[c]).or_insert(next) == next {
                    b.node();
                }
            }
            fills.push((net, pm, group_node));
        }
    }

    let prims = &b.prims;
    let labels = &b.labels;
    let mut sets = Sets((0..b.nodes).collect());

    // everything by where it is
    let mut bb = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in prims {
        let q = p.solid.bbox();
        bb = (bb.0.min(q.0), bb.1.min(q.1), bb.2.max(q.2), bb.3.max(q.3));
    }
    let mut grid = Grid::new(bb);
    let mut cells: Vec<u32> = Vec::new();
    for (i, p) in prims.iter().enumerate() {
        grid.cells_of(&p.solid, 0.0, &mut cells);
        for &c in &cells {
            grid.cells[c as usize].push(i as u32);
        }
    }

    // every two things near each other, once
    let reach = rules.reach() + EDGE_EPS;
    let layer_name = |m: u32| sc.layers[m.trailing_zeros() as usize].clone();
    let mut worst: HashMap<(u32, u32, &'static str), (f64, Finding)> = HashMap::new();
    let mut seen = vec![u32::MAX; prims.len()];
    for (i, a) in prims.iter().enumerate() {
        let abox = a.solid.bbox();
        grid.cells_of(&a.solid, reach, &mut cells);
        for &c in &cells {
            for &j in &grid.cells[c as usize] {
                let ju = j as usize;
                if ju <= i || seen[ju] == i as u32 {
                    continue;
                }
                seen[ju] = i as u32;
                let o = &prims[ju];
                let common = a.lmask & o.lmask;
                if common == 0 {
                    continue;
                }
                let obox = o.solid.bbox();
                let apart = (obox.0 - abox.2).max(abox.0 - obox.2).max(obox.1 - abox.3).max(abox.1 - obox.3);
                if apart > reach {
                    continue;
                }
                // what must lie between the two, how far short of it is still let pass, and what a shortfall is called
                let (need, slack, kind): (f64, f64, &'static str) = match (a.what.copper(), o.what.copper()) {
                    (true, true) => {
                        if a.net == o.net {
                            // one net: are the two joined?
                            if a.net != 0 && a.node != o.node && apart <= 0.0 && sets.find(a.node) != sets.find(o.node) && gap(&a.solid, &o.solid).0 <= -TOUCH {
                                sets.join(a.node, o.node);
                            }
                            continue;
                        }
                        // a zone is filled again once the plan's copper lies: its last fill gives way
                        if (!a.plan && !o.plan) || a.what == What::Island || o.what == What::Island {
                            continue;
                        }
                        (rules.between(a.net, o.net, common).max(a.own).max(o.own), EPS, "clearance")
                    }
                    (false, false) => {
                        // two holes, one of them a via's of the plan
                        let holes = matches!(a.what, What::Hole | What::Npth) && matches!(o.what, What::Hole | What::Npth);
                        let same_spot = a.net == o.net && (a.at.0 - o.at.0).abs() < TOUCH && (a.at.1 - o.at.1).abs() < TOUCH;
                        match sc.rules.hole_to_hole {
                            Some(h) if holes && (a.plan || o.plan) && !same_spot => (h, EPS, "hole-to-hole"),
                            _ => continue,
                        }
                    }
                    _ => {
                        let (c, h) = if a.what.copper() { (a, o) } else { (o, a) };
                        if c.what == What::Island {
                            continue;
                        }
                        match h.what {
                            What::Npth if c.plan => (sc.rules.hole_clearance, EPS, "hole"),
                            What::Hole if c.net != h.net && (c.plan || h.plan) => (sc.rules.hole_clearance, EPS, "hole"),
                            What::Edge if c.plan => (sc.rules.edge, EDGE_EPS, "edge"),
                            What::Keepout if c.plan && (h.net == 0 || h.net != c.net) && ((c.what == What::Track && h.own as u8 & 1 != 0) || (c.what == What::Via && h.own as u8 & 2 != 0)) => (0.0, EPS, "keepout"),
                            _ => continue,
                        }
                    }
                };
                let (g, at) = gap(&a.solid, &o.solid);
                if g >= need - slack {
                    continue;
                }
                // the plan's piece is named first
                let (p, q) = if a.plan || !o.plan { (a, o) } else { (o, a) };
                let (lp, lq) = (&labels[p.item as usize], &labels[q.item as usize]);
                let kind = if kind == "clearance" && g <= 0.0 { "short" } else { kind };
                let detail = match kind {
                    "short" => format!("{} touches {}", lp, lq),
                    "keepout" => format!("{} lies in {}", lp, lq),
                    _ => format!("{} is {:.4} mm from {}; the rule asks {:.4}", lp, g.max(0.0), lq, need),
                };
                let f = Finding { kind, net: sc.nets[p.net as usize].name.clone(), other: sc.nets[q.net as usize].name.clone(), layer: layer_name(common), at: Some(at), detail };
                let key = (p.item.min(q.item), p.item.max(q.item), kind);
                match worst.get(&key) {
                    Some((old, _)) if *old <= g => {}
                    _ => {
                        worst.insert(key, (g, f));
                    }
                }
            }
        }
    }
    findings.extend(worst.into_values().map(|(_, f)| f));

    // what each pour joins: the pads the model says it joins (solidly, or by enough spokes), and the
    // net's own tracks and vias that lie in its fill
    if let Some(r) = &filler {
        let f = &r.f;
        for (net, pm, group_node) in &fills {
            for (i, &p) in sc.nets[*net as usize].pads.iter().enumerate() {
                if let Some(&g) = group_node.get(&pm.pad_group[i]) {
                    sets.join(p as u32, g);
                }
            }
            for p in prims.iter().filter(|p| p.net == *net && p.node != NONE && matches!(p.what, What::Track | What::Via)) {
                let pts: Vec<Pt> = match &p.solid {
                    Solid::Cap { a, b, .. } if p.what == What::Track => {
                        let n = (((b.0 - a.0).hypot(b.1 - a.1) / (f.pitch / 2.0)).ceil() as usize).max(1);
                        (0..=n).map(|k| (a.0 + (b.0 - a.0) * k as f64 / n as f64, a.1 + (b.1 - a.1) * k as f64 / n as f64)).collect()
                    }
                    _ => vec![p.at],
                };
                for l in (0..nl).filter(|l| p.lmask & (1 << l) != 0) {
                    for &(x, y) in &pts {
                        let (nx, ny) = f.node_near(x, y);
                        let c = pm.comp[f.idx(l, ny, nx)] as usize;
                        if c != 0 {
                            sets.join(p.node, group_node[&pm.comp_group[c]]);
                        }
                    }
                }
            }
        }
    }

    // what each plane reaches
    for q in &plane_nodes {
        let within = |p: Pt| q.area.as_ref().map_or(true, |(outer, holes)| in_loop(outer, p) && !holes.iter().any(|h| in_loop(h, p)));
        for p in prims.iter().filter(|p| p.net == q.net && p.node != NONE && p.lmask & (1 << q.layer) != 0) {
            let inside = match (&p.solid, p.what) {
                (Solid::Cap { a, b, .. }, What::Track) => within(*a) || within(*b),
                (_, What::Pad) | (_, What::Via) => within(p.at),
                _ => false,
            };
            if inside {
                sets.join(p.node, q.node);
            }
        }
    }

    // is every net one piece?
    let said: std::collections::HashSet<&str> = plan.left_out.iter().map(|s| s.as_str()).collect();
    let (mut nets, mut joined) = (0, 0);
    for (ni, n) in sc.nets.iter().enumerate().skip(1) {
        if n.pads.len() < 2 || !n.in_scope {
            continue;
        }
        nets += 1;
        let mut groups: Vec<(u32, Vec<usize>)> = Vec::new();
        for &pi in &n.pads {
            let root = sets.find(pi as u32);
            match groups.iter_mut().find(|g| g.0 == root) {
                Some(g) => g.1.push(pi),
                None => groups.push((root, vec![pi])),
            }
        }
        if groups.len() == 1 {
            joined += 1;
            continue;
        }
        groups.sort_by_key(|g| std::cmp::Reverse(g.1.len()));
        let show = |g: &Vec<usize>| {
            let mut s: Vec<String> = g.iter().take(4).map(|&pi| format!("{}.{}", sc.pads[pi].ref_, sc.pads[pi].num)).collect();
            if g.len() > 4 {
                s.push(format!("and {} more", g.len() - 4));
            }
            format!("[{}]", s.join(" "))
        };
        let mut parts: Vec<String> = groups.iter().take(4).map(|g| show(&g.1)).collect();
        if groups.len() > 4 {
            parts.push(format!("and {} more", groups.len() - 4));
        }
        let lone = sc.pads[groups[groups.len() - 1].1[0]].clone();
        let admitted = said.contains(n.name.as_str());
        findings.push(Finding {
            kind: if admitted { "open" } else { "open-unsaid" },
            net: sc.nets[ni].name.clone(),
            other: String::new(),
            layer: String::new(),
            at: Some((lone.x, lone.y)),
            detail: format!("its {} pads are in {} pieces: {}; {}", n.pads.len(), groups.len(), parts.join(" "), if admitted { "the plan says so" } else { "the plan calls it joined" }),
        });
    }

    // copper outside the board: told by the outline when it is closed
    let key = |x: f64, y: f64| ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64);
    let mut ends: HashMap<(i64, i64), u32> = HashMap::new();
    for &(ax, ay, bx, by) in &sc.outline {
        *ends.entry(key(ax, ay)).or_insert(0) += 1;
        *ends.entry(key(bx, by)).or_insert(0) += 1;
    }
    if ends.values().all(|n| n % 2 == 0) {
        let outside = |p: Pt| {
            let mut inside = false;
            for &(ax, ay, bx, by) in &sc.outline {
                if (ay > p.1) != (by > p.1) && p.0 < (bx - ax) * (p.1 - ay) / (by - ay) + ax {
                    inside = !inside;
                }
            }
            !inside
        };
        let mut told = std::collections::HashSet::new();
        for p in prims.iter().filter(|p| p.plan && p.what.copper()) {
            if outside(p.at) && told.insert(p.item) {
                findings.push(Finding { kind: "outside", net: sc.nets[p.net as usize].name.clone(), other: String::new(), layer: layer_name(p.lmask), at: Some(p.at), detail: format!("{} lies outside the board's outline", labels[p.item as usize]) });
            }
        }
    } else {
        notes.push("the outline is not closed: nothing was checked for lying outside the board".into());
    }

    let order = |k: &str| ["plan", "open-unsaid", "open", "short", "clearance", "hole", "hole-to-hole", "edge", "outside", "keepout"].iter().position(|x| *x == k).unwrap_or(99);
    findings.sort_by(|x, y| {
        let at = |f: &Finding| f.at.map_or((0i64, 0i64), |(x, y)| ((x * 1e4).round() as i64, (y * 1e4).round() as i64));
        (order(x.kind), &x.net, &x.other, at(x), &x.detail).cmp(&(order(y.kind), &y.net, &y.other, at(y), &y.detail))
    });
    Report { findings, nets, joined, notes }
}

const USAGE: &str = "usage: astar check SCENE.json PLAN.json [--max N] [-q]
       astar's own exact check of a plan against its scene: is every pad of every net joined by the plan's
       copper, the pads and the planes; is the plan's copper clear of every other net's copper, of every hole,
       of the board edge and of every keep-out, by the scene's rules. One finding a line:
         KIND NET [/ OTHER NET] [on LAYER] [at (X, Y)]: what is wrong
       KIND: open-unsaid (a net in pieces that the plan calls joined), open (in pieces, and the plan's left_out
       says so), short, clearance, hole, hole-to-hole, edge, outside, keepout, plan (the plan does not fit the scene)
       --max N  print at most N findings (the count is always whole)     -q  print no findings, only the count
       exit: 0 clean, 1 findings, 2 the scene or the plan could not be read";

/// `astar check`: the exit code.
pub fn command(args: &[String]) -> i32 {
    let mut files: Vec<&str> = Vec::new();
    let mut max = usize::MAX;
    let mut quiet = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--max" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse::<usize>().ok()) {
                    Some(n) => max = n,
                    None => {
                        eprintln!("{}", USAGE);
                        return crate::exit::BAD_INPUT;
                    }
                }
            }
            "-q" | "--quiet" => quiet = true,
            a if a.starts_with('-') => {
                eprintln!("{}", USAGE);
                return crate::exit::BAD_INPUT;
            }
            a => files.push(a),
        }
        i += 1;
    }
    if files.len() != 2 {
        eprintln!("{}", USAGE);
        return crate::exit::BAD_INPUT;
    }
    let refuse = |why: String| {
        eprintln!("astar check: {}", why);
        crate::exit::BAD_INPUT
    };
    let sc = match Scene::load(files[0]) {
        Ok(s) => s,
        Err(e) => return refuse(e.to_string()),
    };
    // the scene's JSON once more, for every plane it declares (the scene keeps one a net)
    let planes = match std::fs::read_to_string(files[0]).map_err(|e| e.to_string()).and_then(|t| crate::json::parse(&t)) {
        Ok(j) => scene_planes(&j, &sc),
        Err(e) => return refuse(format!("{}: {}", files[0], e)),
    };
    let plan = match Plan::load(files[1]) {
        Ok(p) => p,
        Err(e) => return refuse(e),
    };
    let rep = check(&sc, &plan, &planes);
    if !quiet {
        for f in rep.findings.iter().take(max) {
            println!("{}", f);
        }
        if rep.findings.len() > max {
            println!("... and {} more", rep.findings.len() - max);
        }
    }
    for n in &rep.notes {
        eprintln!("astar check: note: {}", n);
    }
    if rep.clean() {
        eprintln!("astar check: clean: {} of {} nets joined, the plan's copper is clear", rep.joined, rep.nets);
        return 0;
    }
    eprintln!("astar check: {} findings ({}); {} of {} nets joined", rep.findings.len(), rep.tally(), rep.joined, rep.nets);
    1
}
