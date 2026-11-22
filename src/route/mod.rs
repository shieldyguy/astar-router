//! The router: builds the field from the scene, routes every net, lifts and re-routes what is in the way.

mod access;
mod cfg;
mod copper;
mod drive;
mod duo;
mod ends;
pub(crate) mod exact;
pub(crate) mod fill;
mod job;
mod lay;
mod mend;
mod negotiate;
pub(crate) mod net;
mod old;
mod probe;
mod redraw;
pub(crate) mod solve;
mod wall;

pub use access::Access;
pub use cfg::Cfg;
pub use drive::{route, Routed};
pub use job::{Job, JobKind, Piece, Track};
pub use negotiate::PASS;
pub use net::NET;
pub use exact::{StaticIndex, StaticObj};
pub(crate) use solve::Ask;

use crate::field::{Dist, Field, Kind, TICK};
use crate::geom::Shape;
use crate::passes::Clock;
use crate::report::Stats;
use crate::scene::Scene;
use crate::search::{Costs, Search};
use std::collections::HashMap;
use std::time::Instant;

/// A node of the grid: (layer, y, x).
pub type Node = (u8, u16, u16);

pub struct Router {
    pub sc: Scene,
    pub f: Field,
    pub cfg: Cfg,
    pub(crate) search: Search,
    pub(crate) costs: Costs,
    pub jobs: Vec<Job>,
    /// per node: how often nets have clashed there (it makes the place dearer for every net)
    pub(crate) wear: Vec<u8>,
    /// how dear a clash is now, as a multiple of its first price
    pub(crate) pressure: f64,
    /// the last pass: no clash may be made
    pub(crate) strict: bool,
    /// per net: every via it has, as nodes (x, y)
    pub(crate) net_vias: Vec<Vec<(u16, u16)>>,
    /// per net with a plane: the centre nodes of its through-hole pads (already in the plane)
    pub(crate) net_tht: Vec<Vec<(u16, u16)>>,
    pub(crate) via_ok: Vec<u64>,
    /// per net with a plane that a zone of the board draws: the via sites that stand inside that zone.
    /// A drop outside it would reach the plane's layer and no copper.
    /// the radius round a via's middle that other nets keep their clearance from
    pub(crate) via_r: f64,
    /// the smaller via the board allows (size, drill), laid where the class's own does not fit; and the
    /// same radius, threshold and sites for it (None, 0 and empty when the board allows none smaller)
    pub via_small: Option<(f64, f64)>,
    pub(crate) via_r_s: f64,
    pub(crate) t_via_s: Dist,
    pub(crate) via_ok_s: Vec<u64>,
    pub(crate) plane_sites_s: HashMap<u16, Vec<u64>>,
    pub(crate) plane_sites: HashMap<u16, Vec<u64>>,
    /// where each plane net's plane is, for a via that stands in a pad of its own (`access::PlaneArea`)
    pub(crate) plane_areas: HashMap<u16, access::PlaneArea>,
    /// per node, a bit: a via here would stand too near another hole (the hole-to-hole rule)
    pub(crate) hole_near: Vec<u64>,
    /// what never changes about each net's pours (`fill`), for the nets that have one
    pub(crate) pours: HashMap<u16, fill::PourBase>,
    /// what never changes about the fill of each plane a zone draws (`fill`, `Router::relieve`)
    pub(crate) plane_bases: HashMap<u16, fill::PourBase>,
    /// per pad: the nodes a track may start from: round its centre, or the ends of its fences
    pub(crate) acc: Vec<Vec<Access>>,
    /// the fixed copper as shapes (the exact index)
    pub statics: StaticIndex,
    /// (net, x, y) of a via -> the job that laid it
    pub(crate) via_owner: HashMap<(u16, u16, u16), usize>,
    /// pad -> its plane drop job
    /// per pad with a front and a back (`passes::exits`): the nodes down its middle, as a box (x0, y0, x1, y1)
    pub(crate) exit_box: HashMap<usize, (i32, i32, i32, i32)>,
    /// the board is routed with the exit rule left out, because with it this many nets stayed open (`drive`)
    pub(crate) sides: Option<usize>,
    /// the tracks and vias the board came with, in islands (`old`)
    pub(crate) old: old::Old,
    /// the nets joined off this board (`Cfg::drops`): their pads go down by vias, and no plane of theirs here
    pub(crate) hosted: Vec<u16>,
    pub(crate) drop_of: HashMap<usize, usize>,
    /// per job: the jobs whose track ends on its copper
    pub(crate) deps: Vec<Vec<usize>>,
    /// clashes standing now: (the job laid through, the job whose copper was there)
    pub(crate) pairs: Vec<(usize, usize)>,
    /// the differential pairs routed side by side (`duo`), and what the plan says of those that are not
    pub(crate) duos: Vec<duo::Duo>,
    pub(crate) duo_notes: Vec<(u16, String)>,
    /// copper layers that carry tracks
    pub(crate) routable: Vec<usize>,
    pub(crate) t_via: Dist,
    pub(crate) via_gap2: i64,
    pub(crate) via_steps: u8,
    /// rails this router chose to pour: (net, layer)
    pub poured: Vec<(u16, usize)>,
    /// stop when the search has expanded more nodes than this (set by a caller running two routers)
    pub limit: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    /// where the search's work so far is put for a caller to read
    pub progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    /// the path passes that are on: (clock entry, the pass's function)
    pub(crate) shapers: Vec<(usize, fn(&Router, u16, &[Node]) -> Vec<Node>)>,
    /// seconds per pass
    pub clock: Clock,
    /// the report's entries (`Router::note`)
    pub notes: Vec<crate::report::Note>,
    /// the limit was passed: the copper is unfinished and not to be used
    /// per net: the clearance it keeps and how much more than the board's that is (`Tight`)
    pub(crate) clr_tab: Vec<f64>,
    pub(crate) more_tab: Vec<f64>,
    /// the nets whose copper the net being mended went through, this try, and those the next try keeps
    /// off (`mend`)
    /// the nets kept to layers, and where each may run (`search::Keep`)
    pub(crate) keeps: HashMap<u16, crate::search::Keep>,
    pub(crate) tried: Vec<u16>,
    pub(crate) shun: Vec<u16>,
    pub abandoned: bool,
    /// the rounds and the mending are over: the nets have their ways, and what is drawn now is drawn for good
    pub(crate) settled: bool,
    /// the last connection asked and not found (`solve`): the board itself leaves it no way
    pub(crate) no_way: bool,
    pub stats: Stats,
    pub(crate) t0: Instant,
}

/// The outline's box.
fn extent(sc: &Scene) -> (f64, f64, f64, f64) {
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(ax, ay, bx, by) in &sc.outline {
        minx = minx.min(ax).min(bx);
        miny = miny.min(ay).min(by);
        maxx = maxx.max(ax).max(bx);
        maxy = maxy.max(ay).max(by);
    }
    (minx, miny, maxx, maxy)
}

/// Where the grid stands: it is laid so that the pads are on it. Pads do not sit on multiples of the
/// pitch (a chip's pin rows are where its body puts them), and a track that leaves a pad from the node
/// beside the pad's middle has to step over to it. So each way the grid is shifted, by less than a
/// pitch, to where the pads' middle lines lie nearest its lines: a long pad counts by the coordinate
/// across its length (the line its track runs out on), any other pad a little by both, and a narrow pad
/// for much more than a broad one. What is still off the grid after that is drawn on its true line by
/// `ends`. (0, 0): the grid on multiples of the pitch.
pub(crate) fn grid_offset(sc: &Scene, pitch: f64) -> (f64, f64) {
    use crate::geom::hypot;
    // per axis: (the coordinate within a pitch, in hundredths of a micron; its weight)
    let mut want: [Vec<(i64, f64)>; 2] = [Vec::new(), Vec::new()];
    let q = (pitch * 1e5).round() as i64;
    for pad in sc.pads.iter().filter(|p| p.net != 0 && sc.nets[p.net as usize].pads.len() >= 2) {
        // the pad's long axis, when it has one, and how wide the pad is across it
        let (long, across) = match &pad.shape {
            Shape::Poly { pts, r } if pts.len() == 4 => {
                let (e0, e1) = ((pts[1].0 - pts[0].0, pts[1].1 - pts[0].1), (pts[2].0 - pts[1].0, pts[2].1 - pts[1].1));
                let (l0, l1) = (hypot(e0.0, e0.1) + 2.0 * r, hypot(e1.0, e1.1) + 2.0 * r);
                if l0 >= 1.3 * l1 {
                    (Some(e0), l1)
                } else if l1 >= 1.3 * l0 {
                    (Some(e1), l0)
                } else {
                    (None, l0.min(l1))
                }
            }
            Shape::Capsule { ax, ay, bx, by, r } if hypot(bx - ax, by - ay) >= 0.6 * r => (Some((bx - ax, by - ay)), 2.0 * r),
            Shape::Capsule { r, .. } | Shape::Circle { r, .. } => (None, 2.0 * r),
            _ => continue,
        };
        // the narrower the pad, the more its line matters: a track half a pitch off the middle of a
        // 0.2 mm pin is in the next pin's clearance, off the middle of a 2 mm pad it is nothing
        let need = (0.5 / across.max(0.1)).powi(2);
        let w = match long {
            Some(d) if d.1.abs() < 1e-6 * d.0.abs() => [0.0, need],
            Some(d) if d.0.abs() < 1e-6 * d.1.abs() => [need, 0.0],
            Some(_) => [0.0, 0.0],
            None => [0.25 * need, 0.25 * need],
        };
        for (a, v) in [pad.x, pad.y].into_iter().enumerate() {
            if w[a] > 0.0 {
                want[a].push((((v * 1e5).round() as i64).rem_euclid(q), w[a]));
            }
        }
    }
    let best = |want: &Vec<(i64, f64)>| -> f64 {
        // the same coordinates together, the heaviest first; the best shift is one of them
        let mut at: std::collections::BTreeMap<i64, f64> = std::collections::BTreeMap::new();
        for &(v, w) in want {
            *at.entry(v).or_insert(0.0) += w;
        }
        let cost = |o: i64| at.iter().map(|(&v, &w)| w * ((v - o + q / 2).rem_euclid(q) - q / 2).abs() as f64).sum::<f64>();
        let mut tries: Vec<(i64, f64)> = at.iter().map(|(&v, &w)| (v, w)).collect();
        tries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.0.cmp(&b.0)));
        let (mut o, mut least) = (0, cost(0));
        for &(v, _) in tries.iter().take(200) {
            let c = cost(v);
            if c < least - 1e-6 {
                (o, least) = (v, c);
            }
        }
        o as f64 / 1e5
    };
    (best(&want[0]), best(&want[1]))
}

/// Whether `name` is what `pattern` asks for: the same letters, with `*` standing for any run of them.
pub fn like(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    // (star: where the last `*` stood and how much of the name it has taken so far)
    let (mut i, mut k, mut star): (usize, usize, Option<(usize, usize)>) = (0, 0, None);
    while k < n.len() {
        if i < p.len() && p[i] == '*' {
            star = Some((i, k));
            i += 1;
        } else if i < p.len() && p[i] == n[k] {
            i += 1;
            k += 1;
        } else if let Some((si, sk)) = star {
            star = Some((si, sk + 1));
            i = si + 1;
            k = sk + 1;
        } else {
            return false;
        }
    }
    p[i..].iter().all(|&c| c == '*')
}

/// What the settings ask of the nets: which are routed (`--nets`, `--except`, within what the scene asks
/// for itself), and which are kept to layers (`--route-on`, over the scene's own).
pub fn ask(sc: &mut Scene, cfg: &Cfg) {
    let layers = sc.layers.clone();
    for n in sc.nets.iter_mut().skip(1) {
        if (!cfg.nets.is_empty() && !cfg.nets.iter().any(|p| like(p, &n.name))) || cfg.except.iter().any(|p| like(p, &n.name)) {
            n.in_scope = false;
        }
        for (_, names) in cfg.route_on.iter().filter(|(p, _)| like(p, &n.name)) {
            n.keep = names.iter().filter_map(|l| layers.iter().position(|x| x == l)).fold(0, |m, l| m | (1 << l));
        }
    }
}

/// A node's place on the grid is kept in 16 bits.
pub const MAX_NODES_ACROSS: usize = 65_000;

/// Can this scene be routed with these settings? The scene's own check, the settings', and what the two
/// make together: the grid. The first thing wrong is the error.
pub fn check(sc: &Scene, cfg: &Cfg) -> Result<(), String> {
    sc.check()?;
    cfg.check()?;
    for name in &cfg.drops {
        if !sc.nets.iter().skip(1).any(|n| n.name == *name) {
            return Err(format!("--drops {}: the board has no net of that name", name));
        }
    }
    for (flag, names) in [("--nets", &cfg.nets), ("--except", &cfg.except)] {
        for name in names {
            if !sc.nets.iter().skip(1).any(|n| like(name, &n.name)) {
                return Err(format!("{} {}: the board has no net of that name", flag, name));
            }
        }
    }
    for (name, layers) in &cfg.route_on {
        if !sc.nets.iter().skip(1).any(|n| like(name, &n.name)) {
            return Err(format!("--route-on {}: the board has no net of that name", name));
        }
        for l in layers {
            if !sc.layers.contains(l) {
                return Err(format!("--route-on {}={}: the board has no copper layer {} (it has {})", name, layers.join("+"), l, sc.layers.join(", ")));
            }
        }
    }
    let nl = sc.layers.len();
    let mut given: Vec<(usize, usize)> = Vec::new();
    for (name, layers) in &cfg.planes {
        let said = format!("--plane {}={}", name, layers.join("+"));
        let Some(n) = sc.nets.iter().skip(1).position(|x| x.name == *name).map(|i| i + 1) else {
            return Err(format!("{}: the board has no net of that name", said));
        };
        for l in layers {
            let Some(li) = sc.layers.iter().position(|x| x == l) else {
                return Err(format!("{}: the board has no copper layer {} (it has {})", said, l, sc.layers.join(", ")));
            };
            if li == 0 || li + 1 >= nl {
                continue;
            }
            // an inner layer is a plane: one a net, one net a layer
            if let Some(o) = sc.nets.iter().position(|x| x.plane == Some(li)).filter(|o| *o != n) {
                return Err(format!("{}: {} is the plane of {} already", said, l, sc.nets[o].name));
            }
            if sc.nets[n].plane.map_or(false, |p| p != li) || given.iter().any(|g| (g.0 == n) != (g.1 == li)) {
                return Err(format!("{}: a net has one plane, and a layer is one net's (an outer layer can be given besides, as a pour)", said));
            }
            given.push((n, li));
        }
    }
    // a net kept to layers needs one of them to run on
    let mut asked = sc.clone();
    ask(&mut asked, cfg);
    let planes: u32 = asked.nets.iter().filter_map(|n| n.plane).fold(0, |m, l| m | (1 << l));
    for n in asked.nets.iter().skip(1).filter(|n| n.keep != 0 && n.plane.is_none() && n.keep & !planes == 0) {
        return Err(format!("{} is kept to layers that are all another net's plane: it has nowhere to run", n.name));
    }
    let (minx, miny, maxx, maxy) = extent(sc);
    let (_, _, w, h) = Field::grid(minx, miny, maxx, maxy, cfg.pitch, (0.0, 0.0));
    let cells = w.saturating_mul(h).saturating_mul(sc.layers.len());
    if w > MAX_NODES_ACROSS || h > MAX_NODES_ACROSS || cells > cfg.max_cells {
        return Err(format!(
            "the board is {:.1} by {:.1} mm: at a pitch of {} mm that is a grid of {} by {} nodes on {} layers; the router takes {} nodes across and {} million in all (a coarser --pitch makes it smaller)",
            maxx - minx,
            maxy - miny,
            cfg.pitch,
            w,
            h,
            sc.layers.len(),
            MAX_NODES_ACROSS,
            cfg.max_cells / 1_000_000
        ));
    }
    if (0..sc.layers.len()).all(|l| sc.nets.iter().any(|n| n.plane == Some(l))) {
        return Err("every copper layer is a net's plane: there is no layer left for tracks".into());
    }
    Ok(())
}

impl Router {
    /// The threshold, in ticks, a node must read for a track of this half-width to be legal between it
    /// and a neighbour node: the clearance plus the sag of the straight piece between two nodes.
    pub(crate) fn thr(pitch: f64, half: f64, clr: f64, sag: bool) -> Dist {
        let t = half + clr;
        let tt = if sag { (t * t + pitch * pitch / 2.0).sqrt() } else { t };
        // (short by `SLACK` at most, as everywhere: what lets a track on the grid pass a fine-pitch pin's
        // neighbour at all)
        (((tt - crate::geom::SLACK) / TICK) - 1e-9).ceil().min(65534.0) as Dist
    }

    pub(crate) fn net_clr(&self, net: u16) -> f64 {
        self.sc.rules.clearance.max(self.sc.nets[net as usize].clearance)
    }
    /// The narrowest a net's track is drawn: its own width or the board's, whichever is less, and down to
    /// the fab's floor where the scene names one (a neck at a fine-pitch pad; every neck is reported).
    pub(crate) fn w_min(&self, net: u16) -> f64 {
        let w = self.sc.nets[net as usize].width.min(self.sc.rules.track);
        match self.sc.rules.floors.track {
            Some(f) if f > 0.0 && f < w => f,
            _ => w,
        }
    }
    /// The via threshold of a net: the board's, or wider when the net asks more clearance than the board.
    pub(crate) fn t_via_of(&self, net: u16) -> Dist {
        if self.net_clr(net) > self.sc.rules.clearance {
            Self::thr(self.cfg.pitch, self.via_r, self.net_clr(net), false)
        } else {
            self.t_via
        }
    }
    /// How much further than the board's clearance other nets keep from this net's copper.
    /// Is the via net lays at node (x, y) the small one: the full one where it stands clear of routed
    /// copper, else the small one where that does; when neither does, the largest the fixed copper allows
    /// (the search prices a via the same way).
    pub(crate) fn via_is_small(&self, net: u16, x: usize, y: usize) -> bool {
        let i = y * self.f.w + x;
        let bit = |m: &[u64]| m.get(i >> 6).map_or(false, |w| w & (1 << (i & 63)) != 0);
        let sm = self.t_via_s != 0 && self.t_via_of(net) == self.t_via && bit(&self.via_ok_s);
        if !sm {
            return false;
        }
        let big = bit(&self.via_ok);
        let free = |t: Dist| (0..self.f.nl).all(|l| crate::field::ddist(&self.f.cells[self.f.idx(l, y, x)], net) >= t);
        if big && free(self.t_via) {
            false
        } else if free(self.t_via_s) {
            true
        } else {
            !big
        }
    }

    pub(crate) fn more_clr(&self, net: u16) -> f64 {
        (self.net_clr(net) - self.sc.rules.clearance).max(0.0)
    }
    /// Below this a node is surely too near other copper for the net's narrowest track. From here up to
    /// `t_min` the grid cannot tell: `t_min` keeps an allowance for a step between two nodes that sags
    /// toward a corner, and a tick of rounding; beside a straight edge (a pad's side, a track that runs
    /// alongside) there is no sag, and a track that fits exactly is in that band. There the step is
    /// measured, piece against shape (`Tight`).
    pub(crate) fn t_low(&self, net: u16) -> Dist {
        (((self.w_min(net) / 2.0 + self.net_clr(net) - crate::geom::SLACK) / TICK).floor().max(0.0) as Dist).min(self.t_min(net))
    }
    pub(crate) fn tight(&self) -> exact::Tight<'_> {
        exact::Tight { statics: &self.statics, clr: &self.clr_tab, more: &self.more_tab }
    }
    /// May a track of the net at its narrowest go from node a to its neighbour b (x, y) on layer l: clear
    /// of fixed copper and of other nets' routed copper? By the grid where the grid is sure, else measured.
    pub(crate) fn step_free(&self, net: u16, l: usize, a: (usize, usize), b: (usize, usize)) -> bool {
        let (t_low, t_min) = (self.t_low(net), self.t_min(net));
        let (ca, cb) = (&self.f.cells[self.f.idx(l, a.1, a.0)], &self.f.cells[self.f.idx(l, b.1, b.0)]);
        let (sb, db) = (crate::field::sdist(cb, net), crate::field::ddist(cb, net));
        if sb < t_low || db < t_low {
            return false;
        }
        let (pa, pb) = ((self.f.wx(a.0), self.f.wy(a.1)), (self.f.wx(b.0), self.f.wy(b.1)));
        let half = self.w_min(net) / 2.0;
        (sb.min(crate::field::sdist(ca, net)) >= t_min || self.tight().fixed(pa, pb, half, net, l)) && (db.min(crate::field::ddist(ca, net)) >= t_min || self.tight().routed(&self.f, pa, pb, half, net, l))
    }
    pub(crate) fn t_min(&self, net: u16) -> Dist {
        let w = self.w_min(net);
        Self::thr(self.cfg.pitch, w / 2.0, self.net_clr(net), true)
    }
    pub(crate) fn t_nom(&self, net: u16) -> Dist {
        Self::thr(self.cfg.pitch, self.sc.nets[net as usize].width / 2.0, self.net_clr(net), true)
    }

    /// The field, the exact index and the pads' access nodes of a board; no fence, no job, no copper yet.
    /// `poured`: the planes the board passes gave to nets.
    pub fn new(sc: Scene, cfg: Cfg, poured: Vec<(u16, usize)>) -> Result<Router, String> {
        check(&sc, &cfg)?;
        let t0 = Instant::now();
        let mut sc = sc;
        let copper_pads = copper::copper_pads(&mut sc);
        let pitch = cfg.pitch;
        let nl = sc.layers.len();
        // a net joined off this board (`Cfg::drops`): the pour that joins it is its host's, on the bottom.
        // Here it is a net with a plane there, so each of its pads gets a via (`passes::drops`), but the
        // layer stays open to every other net's tracks
        let mut hosted: Vec<u16> = Vec::new();
        for name in &cfg.drops {
            if let Some(i) = sc.nets.iter().position(|n| n.name == *name) {
                if i != 0 && sc.nets[i].plane.is_none() {
                    sc.nets[i].plane = Some(nl - 1);
                    hosted.push(i as u16);
                }
            }
        }
        // A clearance that holds on one layer only, or between two classes only, is kept everywhere: the
        // field knows one clearance a net. The board's is the most any layer asks, a net's the most its
        // class asks on any layer or against any other class. (More room than the rule asks in places,
        // never less.)
        sc.rules.clearance = sc.rules.layer_rules.iter().filter_map(|r| r.clearance).fold(sc.rules.clearance, f64::max);
        for i in 1..sc.nets.len() {
            let c = sc.nets[i].netclass;
            let mut most = sc.nets[i].clearance;
            for v in sc.classes.get(c).iter().flat_map(|k| k.layer_rules.iter().filter_map(|r| r.clearance)) {
                most = most.max(v);
            }
            for p in sc.class_pairs.iter().filter(|p| p.a == c || p.b == c) {
                most = most.max(p.clearance);
            }
            sc.nets[i].clearance = most;
        }
        // the pairs that run side by side, each with a net of its own for its centre line (`duo`)
        let (duos, duo_notes) = duo::plan(&mut sc, cfg.skip.iter().any(|s| s == "pairs"));
        let clr = sc.rules.clearance;
        let (minx, miny, maxx, maxy) = extent(&sc);
        // how far an object's distance must be known: the widest track's half-width plus its clearance, and
        // a via's plus the largest clearance a net asks
        // what other nets keep their clearance from round a via: its copper, or more where the board asks
        // more room round a hole than the via's ring and the clearance give
        let via_r = (sc.rules.via / 2.0).max(sc.rules.drill / 2.0 + sc.rules.hole_clearance - clr);
        let mut reach: f64 = via_r + clr;
        for n in &sc.nets {
            if n.pads.len() >= 2 {
                reach = reach.max(n.width / 2.0 + clr.max(n.clearance)).max(via_r + clr.max(n.clearance));
            }
        }
        for d in &duos {
            reach = reach.max(d.width / 2.0 + sc.nets[d.centre as usize].clearance);
        }
        let rmax = reach + pitch + 0.03;
        let mut f = Field::new(minx, miny, maxx, maxy, pitch, nl, rmax, grid_offset(&sc, pitch));
        // the narrowest track any net of the board is drawn at, by half (`exact::stamped`)
        let low_half = sc.nets.iter().map(|n| n.width).fold(sc.rules.track, f64::min).min(sc.rules.floors.track.filter(|f| *f > 0.0).unwrap_or(f64::MAX)) / 2.0;
        let all: u32 = (1u32 << nl) - 1;

        // the fixed copper, as shapes: the board edge, holes, keep-outs, pads
        let mut statics = StaticIndex::new(minx, miny, maxx, maxy, rmax);
        for &(ax, ay, bx, by) in &sc.outline {
            statics.add(StaticObj::fixed(Kind::Edge, Shape::Capsule { ax, ay, bx, by, r: 0.0 }, all, 0, sc.rules.edge - clr));
        }
        let n_edge = statics.objs.len();
        for &(x, y, d) in &sc.holes {
            statics.add(StaticObj::fixed(Kind::Hole, Shape::Circle { x, y, r: d / 2.0 }, all, 0, sc.rules.hole_clearance - clr));
        }
        for k in &sc.keepouts {
            // only an area that forbids tracks is in a track's way; one that forbids vias alone closes
            // via sites, one that forbids pours alone bounds the planes (`find_via_sites`)
            if !k.forbid.tracks {
                continue;
            }
            let mut m = 0u32;
            for &l in &k.layers {
                m |= 1 << l;
            }
            // (an area that lets one net through is that net's own to the field.) A track keeps no
            // clearance from it, but its copper stays wholly outside: KiCad allows a keep-out no slack,
            // so the track's edge stands a micron off, which is the slack everything else is measured to
            // and as much again
            statics.add(StaticObj::fixed(Kind::Keepout, Shape::Poly { pts: k.poly.clone(), r: 0.0 }, m, k.net, -clr + 0.001));
        }
        // copper the board came with and nobody may move: what the board and its footprints draw in
        // copper (an antenna, a tab, a logo, text), and tracks and vias already there. It is in the way
        // of every other net like a pad is, and part of its own net's tree (`old`).
        let more = |net: u16| (sc.nets[net as usize].clearance - clr).max(0.0);
        for t in &sc.copper.tracks {
            for piece in &t.path {
                let pts: Vec<(f64, f64)> = match piece {
                    crate::scene::PathPiece::Line(v) => vec![(v[0], v[1]), (v[2], v[3])],
                    crate::scene::PathPiece::Arc(v) => crate::geom::arc_points((v[0], v[1]), (v[2], v[3]), (v[4], v[5]), 0.005),
                };
                for w in pts.windows(2) {
                    statics.add(StaticObj::fixed(Kind::Existing, Shape::Capsule { ax: w[0].0, ay: w[0].1, bx: w[1].0, by: w[1].1, r: t.width / 2.0 }, 1 << t.layer, t.net, more(t.net)));
                }
            }
        }
        for v in &sc.copper.vias {
            statics.add(StaticObj::fixed(Kind::Existing, Shape::Circle { x: v.x, y: v.y, r: (v.size / 2.0).max(v.drill / 2.0 + sc.rules.hole_clearance - clr) }, all, v.net, more(v.net)));
        }
        for z in &sc.copper.zones {
            // a drawn shape, not a pour: a pour refills round new tracks, a drawing does not
            if z.locked && z.min_width == Some(0.0) && z.outline.len() >= 3 {
                statics.add(StaticObj::fixed(Kind::Existing, Shape::Poly { pts: z.outline.clone(), r: 0.0 }, 1 << z.layer, z.net, more(z.net)));
            }
        }
        for p in &sc.pads {
            let m = if p.tht { all } else if p.back { 1 << (nl - 1) } else { 1 };
            let own = p.clearance.max(sc.nets[p.net as usize].clearance);
            statics.add(StaticObj::fixed(Kind::Pad, p.shape.clone(), m, p.net, (own - clr).max(0.0)));
            // a pad's hole asks the board's room round a hole of every other net, like any hole: said on
            // its own where the pad's copper and clearance do not keep them that far already
            if let Some(d) = p.drill.filter(|d| *d > 0.0) {
                if d / 2.0 + sc.rules.hole_clearance > p.shape.depth(p.x, p.y) + own.max(clr) + 1e-6 {
                    statics.add(StaticObj::fixed(Kind::Hole, Shape::Circle { x: p.x, y: p.y, r: d / 2.0 }, all, p.net, sc.rules.hole_clearance - clr));
                }
            }
        }

        // the board edge into the field, then everything outside it closed off
        for o in &statics.objs[..n_edge] {
            f.stamp_static(&o.shape, o.lmask, o.net, exact::stamped(o.extra, cfg.pitch, low_half));
        }
        let t_edge = Self::thr(pitch, sc.rules.track.min(0.1) / 2.0, clr, true);
        let mut seen = vec![false; f.w * f.h];
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for p in &sc.pads {
            let (x, y) = f.node_near(p.x, p.y);
            if !seen[y * f.w + x] && f.cells[f.idx(0, y, x)].sd1 >= t_edge {
                seen[y * f.w + x] = true;
                stack.push((x, y));
            }
        }
        while let Some((x, y)) = stack.pop() {
            for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= f.w as i32 || ny >= f.h as i32 {
                    continue;
                }
                let (nx, ny) = (nx as usize, ny as usize);
                if !seen[ny * f.w + nx] && f.cells[f.idx(0, ny, nx)].sd1 >= t_edge {
                    seen[ny * f.w + nx] = true;
                    stack.push((nx, ny));
                }
            }
        }
        for l in 0..nl {
            for y in 0..f.h {
                for x in 0..f.w {
                    // (the outermost nodes are closed whatever the outline says: the search relies on it)
                    if !seen[y * f.w + x] || x == 0 || y == 0 || x == f.w - 1 || y == f.h - 1 {
                        let i = f.idx(l, y, x);
                        f.cells[i].sd1 = 0;
                        f.cells[i].sn1 = 0;
                    }
                }
            }
        }
        drop(seen);
        for o in &statics.objs[n_edge..] {
            f.stamp_static(&o.shape, o.lmask, o.net, exact::stamped(o.extra, cfg.pitch, low_half));
        }

        let t_via = Self::thr(pitch, via_r, clr, false);
        // the smaller via: the board's least, never under what a fab drills (0.3 mm on a 0.15 mm hole)
        // and never without its ring
        let fl = &sc.rules.floors;
        let drill_s = fl.drill.unwrap_or(sc.rules.drill).min(sc.rules.drill).max(0.15);
        let size_s = fl.via.unwrap_or(sc.rules.via).max(drill_s + 2.0 * fl.annular.unwrap_or(0.1)).max(0.3);
        let via_small = if size_s < sc.rules.via - 1e-9 && drill_s <= sc.rules.drill + 1e-9 { Some((size_s, drill_s)) } else { None };
        let via_r_s = via_small.map_or(0.0, |(s, d)| (s / 2.0).max(d / 2.0 + sc.rules.hole_clearance - clr));
        let t_via_s = if via_small.is_some() { Self::thr(pitch, via_r_s, clr, false) } else { 0 };
        let plane_layers: Vec<usize> = sc.nets.iter().enumerate().filter(|(i, _)| !hosted.contains(&(*i as u16))).filter_map(|(_, n)| n.plane).collect();
        let routable: Vec<usize> = (0..nl).filter(|l| !plane_layers.contains(l)).collect();

        let orth = (1000.0 * pitch).round().max(1.0);
        let diag = (1414.2 * pitch).round().max(1.0);
        let mut step = vec![[0u32; 2]; nl];
        for &l in &routable {
            let m = if l == 0 || l == nl - 1 { 1.0 } else { cfg.inner_mul };
            step[l] = [(orth * m).round() as u32, (diag * m).round() as u32];
        }
        let t45 = (cfg.turn_mm * 1000.0).round() as u32;
        let mut costs = Costs::new(step, [0, t45, t45 * 4, t45 * 40], cfg.neck_mul.max(1), orth as u64, diag as u64, f.w * f.h);
        costs.neck_in = (cfg.neck_mm * 1000.0) as u32;
        costs.exit_price = (cfg.exit_mm * 1000.0) as u32;
        let bshift = ((orth / 2.0).log2().floor().max(0.0)) as u32;
        let search = Search::new(&f, bshift);
        let nn = sc.nets.len();
        // two vias' holes keep the board's hole to hole distance, whatever their nets: by the widest hole
        // a via may have here (`Field::holes`, kept as vias are laid and lifted)
        let hole_reach = sc.rules.hole_to_hole.map_or(0.0, |h2h| sc.rules.drill.max(via_small.map_or(0.0, |v| v.1)) + h2h + 0.005);
        let mut f = f;
        f.hole_reach = hole_reach;
        // (two of one net: a via's width and a little more apart, and no nearer than their holes may be)
        let gap = (sc.rules.via + 0.1).max(hole_reach) / pitch;
        let mut r = Router {
            sc,
            f,
            cfg,
            search,
            costs,
            jobs: Vec::new(),
            wear: Vec::new(),
            pressure: 1.0,
            strict: false,
            net_vias: vec![Vec::new(); nn],
            net_tht: vec![Vec::new(); nn],
            via_ok: Vec::new(),
            via_r,
            via_small,
            via_r_s,
            t_via_s,
            via_ok_s: Vec::new(),
            plane_sites_s: HashMap::new(),
            plane_sites: HashMap::new(),
            plane_areas: HashMap::new(),
            hole_near: Vec::new(),
            pours: HashMap::new(),
            plane_bases: HashMap::new(),
            acc: Vec::new(),
            statics,
            via_owner: HashMap::new(),
            drop_of: HashMap::new(),
            exit_box: HashMap::new(),
            sides: None,
            old: old::Old::default(),
            hosted,
            deps: Vec::new(),
            pairs: Vec::new(),
            duos,
            duo_notes,
            routable,
            t_via,
            via_gap2: (gap * gap).ceil() as i64,
            via_steps: (gap.ceil() as u8).saturating_add(2),
            poured,
            limit: None,
            progress: None,
            shapers: Vec::new(),
            clock: Clock::new(),
            notes: Vec::new(),
            clr_tab: Vec::new(),
            more_tab: Vec::new(),
            keeps: HashMap::new(),
            tried: Vec::new(),
            shun: Vec::new(),
            abandoned: false,
            settled: false,
            no_way: false,
            stats: Stats { build_s: 0.0, route_s: 0.0, routings: 0, rips: 0, rounds: 0, fences: 0, corners: 0, rounded: 0, search_s: 0.0, lay_s: 0.0, lift_s: 0.0 },
            t0,
        };
        if copper_pads > 0 {
            r.note("copper", 0, None, format!("{} shapes the board draws in copper with a net are joined to their nets like pads", copper_pads));
        }
        r.clr_tab = (0..r.sc.nets.len()).map(|n| r.net_clr(n as u16)).collect();
        r.more_tab = (0..r.sc.nets.len()).map(|n| r.more_clr(n as u16)).collect();
        r.find_pad_nodes();
        r.find_old();
        // the nets kept to layers: each with its pads, which it may leave on their own layer
        let all: u32 = (1u32 << nl) - 1;
        for ni in (1..r.sc.nets.len()).filter(|&i| r.sc.nets[i].keep != 0 && r.sc.nets[i].plane.is_none()) {
            let pads = r.sc.nets[ni].pads.iter().map(|&p| (if r.sc.pads[p].tht { all } else if r.sc.pads[p].back { 1 << (nl - 1) } else { 1 }, r.sc.pads[p].shape.clone())).collect();
            r.keeps.insert(ni as u16, crate::search::Keep { layers: r.sc.nets[ni].keep, pads, reach: r.cfg.stub_mm });
        }
        r.wear = vec![0u8; r.f.cells.len()];
        Ok(r)
    }
}
