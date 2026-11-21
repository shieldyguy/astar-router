//! Gaps and regions: per layer, the free space as regions joined by gaps.
//!
//! A gap is the narrowest place between two pieces of fixed copper that face each other with nothing
//! between them: its width comes from the exact shapes, its capacity in tracks from the rules, and its
//! capacity in lanes from the field itself (how many tracks the search can pass through it on this
//! grid). A region is a piece of free space that gaps and copper close off; a pocket is a region too
//! small to count as the open board, and its via capacity is how many via sites it holds at once.

use super::geo::sep;
use crate::field::Kind;
use crate::route::Router;
use std::collections::HashMap;

/// Two pieces of copper further apart than this many default tracks side by side do not make a gap.
pub const GAP_TRACKS: f64 = 4.0;
/// A region at least this large, mm2, is the open board.
pub const OPEN_MM2: f64 = 6.0;
const WALL: u32 = u32::MAX;

pub struct Gap {
    /// the two pieces of copper: indices into the exact index (`Router::statics`)
    pub a: u32,
    pub b: u32,
    pub layer: u8,
    /// where they are nearest
    pub pa: (f64, f64),
    pub pb: (f64, f64),
    /// copper to copper, mm
    pub width: f64,
    /// the same less what the two ask beyond the board's clearance
    pub usable: f64,
    /// how many default tracks fit, by the rules
    pub tracks: u32,
    /// how many the search can pass on this grid
    pub lanes: u32,
    /// the regions on its two sides (0: none found)
    pub sides: [u32; 2],
}

pub struct Region {
    pub layer: u8,
    pub cells: u32,
    /// large enough to be the open board
    pub open: bool,
    /// a pocket's via capacity: how many via sites it holds at once (0 for the open board: not counted)
    pub vias: u32,
    /// one of its nodes
    pub at: (u16, u16),
}

pub struct Gaps {
    pub gaps: Vec<Gap>,
    /// field index of a node on a gap's line -> the gap's number + 1
    pub wall: HashMap<u32, u32>,
    /// per layer (empty for a layer that carries no tracks): per node, its region's number + 1; 0: not free
    pub label: Vec<Vec<u32>>,
    pub regions: Vec<Region>,
    /// the first pad's place in the exact index
    pub pad0: usize,
}

impl Gaps {
    /// The region a node lies in.
    pub fn region(&self, l: usize, y: usize, x: usize, w: usize) -> Option<usize> {
        match self.label[l].get(y * w + x) {
            Some(&v) if v != 0 => Some(v as usize - 1),
            _ => None,
        }
    }
    /// Is the node free and part of the open board on this layer?
    pub fn open_at(&self, l: usize, y: usize, x: usize, w: usize) -> bool {
        self.region(l, y, x, w).map_or(false, |k| self.regions[k].open)
    }
    /// What a piece of fixed copper is called.
    pub fn name(&self, r: &Router, obj: u32) -> String {
        let o = &r.statics.objs[obj as usize];
        match o.kind {
            Kind::Pad => {
                let p = &r.sc.pads[obj as usize - self.pad0];
                format!("{}.{}", p.ref_, p.num)
            }
            Kind::Edge => "edge".into(),
            Kind::Hole => "hole".into(),
            Kind::Keepout => "keepout".into(),
            _ => "copper".into(),
        }
    }
    /// The gap between two named pieces of copper on a layer.
    pub fn between(&self, r: &Router, a: &str, b: &str, layer: usize) -> Option<&Gap> {
        self.gaps.iter().find(|g| g.layer as usize == layer && ((self.name(r, g.a) == a && self.name(r, g.b) == b) || (self.name(r, g.a) == b && self.name(r, g.b) == a)))
    }
}

/// How many tracks of width `w` fit through `usable` mm at clearance `c`.
pub fn tracks_through(usable: f64, w: f64, c: f64) -> u32 {
    (((usable - c + 1e-6) / (w + c)).floor()).max(0.0) as u32
}

pub fn find(r: &Router) -> Gaps {
    let (w, c) = (r.sc.rules.track, r.sc.rules.clearance);
    let gmax = GAP_TRACKS * (w + c) + c;
    let f = &r.f;
    let objs = &r.statics.objs;
    let pad0 = r.sc.outline.len() + r.sc.holes.len() + r.sc.keepouts.len();
    let routable: u32 = r.routable.iter().fold(0, |m, &l| m | (1 << l));
    let t_def = Router::thr(f.pitch, w / 2.0, c, true);

    // the fixed copper by where it is, 1 mm bins
    let (bx0, by0) = (f.x0, f.y0);
    let (nbx, nby) = ((f.w as f64 * f.pitch).ceil() as usize + 2, (f.h as f64 * f.pitch).ceil() as usize + 2);
    let bin = |x: f64, y: f64| -> (usize, usize) { (((x - bx0).floor().max(0.0) as usize).min(nbx - 1), ((y - by0).floor().max(0.0) as usize).min(nby - 1)) };
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); nbx * nby];
    let boxes: Vec<(f64, f64, f64, f64)> = objs.iter().map(|o| o.shape.bbox()).collect();
    for (i, o) in objs.iter().enumerate() {
        if o.kind == Kind::Fence || o.lmask & routable == 0 {
            continue;
        }
        let (a, b) = (bin(boxes[i].0, boxes[i].1), bin(boxes[i].2, boxes[i].3));
        for by in a.1..=b.1 {
            for bx in a.0..=b.0 {
                bins[by * nbx + bx].push(i as u32);
            }
        }
    }
    let near = |x0: f64, y0: f64, x1: f64, y1: f64, out: &mut Vec<u32>| {
        out.clear();
        let (a, b) = (bin(x0, y0), bin(x1, y1));
        for by in a.1..=b.1 {
            for bx in a.0..=b.0 {
                out.extend_from_slice(&bins[by * nbx + bx]);
            }
        }
        out.sort_unstable();
        out.dedup();
    };

    let mut gaps: Vec<Gap> = Vec::new();
    let (mut around, mut blockers) = (Vec::new(), Vec::new());
    for (i, o) in objs.iter().enumerate() {
        if o.kind == Kind::Fence || o.lmask & routable == 0 {
            continue;
        }
        let bi = boxes[i];
        let reach = gmax + o.extra.max(0.0) + 1.0;
        near(bi.0 - reach, bi.1 - reach, bi.2 + reach, bi.3 + reach, &mut around);
        for &j in &around {
            let (j, q) = (j as usize, &objs[j as usize]);
            if j <= i || (o.kind == Kind::Edge && q.kind == Kind::Edge) || (o.kind == Kind::Keepout && q.kind == Kind::Keepout) {
                continue;
            }
            let lm = o.lmask & q.lmask & routable;
            if lm == 0 {
                continue;
            }
            let extras = o.extra + q.extra;
            let bj = boxes[j];
            let far = (bj.0 - bi.2).max(bi.0 - bj.2).max(bj.1 - bi.3).max(bi.1 - bj.3);
            if far - extras >= gmax {
                continue;
            }
            let (d, pa, pb) = sep(&o.shape, &q.shape);
            if d <= 1e-9 || d - extras >= gmax {
                continue;
            }
            let m = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
            let rho = d / 2.0;
            near(m.0 - rho, m.1 - rho, m.0 + rho, m.1 + rho, &mut blockers);
            for l in 0..f.nl {
                if lm & (1 << l) == 0 {
                    continue;
                }
                // nothing else may reach into the circle on the two nearest points: then they face each other
                if blockers.iter().any(|&k| k as usize != i && k as usize != j && objs[k as usize].lmask & (1 << l) != 0 && objs[k as usize].shape.dist(m.0, m.1) < rho + 1e-6) {
                    continue;
                }
                let usable = d - extras;
                gaps.push(Gap { a: i as u32, b: j as u32, layer: l as u8, pa, pb, width: d, usable, tracks: tracks_through(usable, w, c), lanes: 0, sides: [0, 0] });
            }
        }
    }

    // the lanes of each gap, read off the field: the nodes on the line across it where a default track
    // may stand, taken as far apart as two tracks must be
    let legal = |l: usize, y: i64, x: i64| -> bool { x >= 0 && y >= 0 && (x as usize) < f.w && (y as usize) < f.h && f.cells[f.idx(l, y as usize, x as usize)].sd1 >= t_def };
    for g in &mut gaps {
        let l = g.layer as usize;
        let (vx, vy) = ((g.pb.0 - g.pa.0) / g.width, (g.pb.1 - g.pa.1) / g.width);
        let along_x = vx.abs() >= vy.abs();
        let m = ((g.pa.0 + g.pb.0) / 2.0, (g.pa.1 + g.pb.1) / 2.0);
        // (u: along the line across the gap, v: the rows to either side of its middle)
        let (u0, u1, vm, lead) = if along_x { (g.pa.0.min(g.pb.0) - f.x0, g.pa.0.max(g.pb.0) - f.x0, m.1 - f.y0, vx.abs()) } else { (g.pa.1.min(g.pb.1) - f.y0, g.pa.1.max(g.pb.1) - f.y0, m.0 - f.x0, vy.abs()) };
        let step = (((w + c) / (f.pitch * lead)) - 1e-9).ceil().max(1.0) as i64;
        let (lo, hi) = ((u0 / f.pitch).floor() as i64 - 1, (u1 / f.pitch).ceil() as i64 + 1);
        let fv = vm / f.pitch;
        let rows: Vec<i64> = if (fv - fv.round()).abs() < 1e-6 { vec![fv.round() as i64] } else { vec![fv.floor() as i64, fv.ceil() as i64] };
        let mut lanes = u32::MAX;
        for &row in &rows {
            let (mut n, mut last) = (0u32, i64::MIN / 2);
            for u in lo..=hi {
                let ok = if along_x { legal(l, row, u) } else { legal(l, u, row) };
                if ok && u - last >= step {
                    n += 1;
                    last = u;
                }
            }
            lanes = lanes.min(n);
        }
        g.lanes = lanes;
    }

    // walls: the nodes on each gap's line; regions: the free nodes they and the copper close off
    let mut wall: HashMap<u32, u32> = HashMap::new();
    for (gi, g) in gaps.iter().enumerate() {
        let l = g.layer as usize;
        let (x0, y0) = f.node_near(g.pa.0.min(g.pb.0) - f.pitch, g.pa.1.min(g.pb.1) - f.pitch);
        let (x1, y1) = f.node_near(g.pa.0.max(g.pb.0) + f.pitch, g.pa.1.max(g.pb.1) + f.pitch);
        for y in y0..=y1 {
            for x in x0..=x1 {
                if crate::geom::seg_dist2(f.wx(x), f.wy(y), g.pa.0, g.pa.1, g.pb.0, g.pb.1).sqrt() <= 0.75 * f.pitch {
                    wall.entry(f.idx(l, y, x) as u32).or_insert(gi as u32 + 1);
                }
            }
        }
    }
    let mut label: Vec<Vec<u32>> = vec![Vec::new(); f.nl];
    let mut regions: Vec<Region> = Vec::new();
    let open_cells = (OPEN_MM2 / (f.pitch * f.pitch)) as u32;
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for &l in &r.routable {
        // free nodes 0, the rest WALL; then each run of free nodes in a row is taken whole
        let base = l * f.w * f.h;
        let mut lab: Vec<u32> = f.cells[base..base + f.w * f.h].iter().map(|c| if c.sd1 >= t_def { 0 } else { WALL }).collect();
        for &ci in wall.keys() {
            if ci as usize >= base && (ci as usize) < base + f.w * f.h {
                lab[ci as usize - base] = WALL;
            }
        }
        for start in 0..f.w * f.h {
            if lab[start] != 0 {
                continue;
            }
            let id = regions.len() as u32 + 1;
            let mut n = 0u32;
            stack.push(((start % f.w) as u32, (start / f.w) as u32));
            while let Some((x, y)) = stack.pop() {
                let row = y as usize * f.w;
                if lab[row + x as usize] != 0 {
                    continue;
                }
                let (mut x0, mut x1) = (x as usize, x as usize);
                while x0 > 0 && lab[row + x0 - 1] == 0 {
                    x0 -= 1;
                }
                while x1 + 1 < f.w && lab[row + x1 + 1] == 0 {
                    x1 += 1;
                }
                lab[row + x0..=row + x1].fill(id);
                n += (x1 - x0 + 1) as u32;
                for ny in [y as i64 - 1, y as i64 + 1] {
                    if ny < 0 || ny as usize >= f.h {
                        continue;
                    }
                    let nrow = ny as usize * f.w;
                    let mut in_run = false;
                    for xx in x0..=x1 {
                        let free = lab[nrow + xx] == 0;
                        if free && !in_run {
                            stack.push((xx as u32, ny as u32));
                        }
                        in_run = free;
                    }
                }
            }
            regions.push(Region { layer: l as u8, cells: n, open: n >= open_cells, vias: 0, at: ((start % f.w) as u16, (start / f.w) as u16) });
        }
        for v in lab.iter_mut() {
            if *v == WALL {
                *v = 0;
            }
        }
        label[l] = lab;
    }
    // each gap's two sides
    for g in &mut gaps {
        let l = g.layer as usize;
        let (nx, ny) = (-(g.pb.1 - g.pa.1) / g.width, (g.pb.0 - g.pa.0) / g.width);
        let m = ((g.pa.0 + g.pb.0) / 2.0, (g.pa.1 + g.pb.1) / 2.0);
        for (k, s) in [1.0, -1.0].iter().enumerate() {
            for step in 2..=8 {
                let d = step as f64 * 0.75 * f.pitch * s;
                let (x, y) = f.node_near(m.0 + nx * d, m.1 + ny * d);
                let v = label[l][y * f.w + x];
                if v != 0 {
                    g.sides[k] = v;
                    break;
                }
            }
        }
    }
    // a pocket's via capacity: the via sites it holds at once, each clear of the others
    let need = (r.sc.rules.via + c) / f.pitch;
    let need2 = (need * need - 1e-9).ceil() as i64;
    let mut sites: HashMap<u32, Vec<(i64, i64)>> = HashMap::new();
    for &l in &r.routable {
        for i in 0..f.w * f.h {
            let v = label[l][i];
            if v == 0 || regions[v as usize - 1].open {
                continue;
            }
            if !(0..f.nl).all(|ll| f.cells[ll * f.w * f.h + i].sd1 >= r.t_via) {
                continue;
            }
            let (x, y) = ((i % f.w) as i64, (i / f.w) as i64);
            let held = sites.entry(v).or_default();
            if held.iter().all(|&(hx, hy)| (hx - x) * (hx - x) + (hy - y) * (hy - y) >= need2) {
                held.push((x, y));
                regions[v as usize - 1].vias += 1;
            }
        }
    }
    Gaps { gaps, wall, label, regions, pad0 }
}
