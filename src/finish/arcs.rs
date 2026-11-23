//! The last step: every corner of the routed copper turned into an arc (krt-better's round_corners,
//! done on the plan).
//!
//! One radius for the whole board, because that is what keeps a bundle a bundle: tracks turning together
//! get the same arc, shifted, and their spacing through the bend is their spacing on the straight. A
//! corner whose legs are too short for the radius gets the largest arc its legs allow (half of each leg,
//! so two corners can share one); under MIN_RADIUS it stays sharp.
//!
//! An arc cuts the inside of its corner, so each one is checked with exact geometry against the fixed
//! copper and against the other nets' copper as it is now, arcs already made included. The biggest radius
//! is tried on every corner first, again and again while that still rounds one more (the inner track of
//! a bundle makes room for the next one out); only then the next smaller radius. Corners are left alone
//! at a pad, at a via, where another track of the net joins, and where the width changes.

use crate::geom::{arc_points, hypot, seg_seg_dist2};
use crate::passes::{Pass, Step};
use crate::route::Router;
use std::collections::HashMap;

pub const PASS: Pass = Pass {
    name: "arcs",
    about: "every corner becomes an arc of one radius (--radius), checked with exact geometry against all other copper",
    step: Step::Finish(arcs),
};

fn arcs(r: &mut Router) {
    let (corners, rounded) = r.round_corners(r.cfg.radius_mm);
    r.stats.corners = corners;
    r.stats.rounded = rounded;
}

const MIN_RADIUS: f64 = 0.15;
/// kept beyond the clearance: KiCad draws an arc as chords of its own
const EPS: f64 = 0.004;
/// how far a chord of the checked arc may stand off the arc
const CHORD: f64 = 0.002;
const SHRINK: f64 = 0.6;
const MAX_SWEEPS: usize = 16;

struct Seg {
    a: (f64, f64),
    b: (f64, f64),
    half: f64,
    net: u16,
    lmask: u32,
}

/// Every piece of routed copper, by where it is.
struct SegIndex {
    segs: Vec<Seg>,
    x0: f64,
    y0: f64,
    cell: f64,
    nx: usize,
    ny: usize,
    buckets: Vec<Vec<u32>>,
    /// per bucket: how many corners were rounded there
    made: Vec<u32>,
}

impl SegIndex {
    fn new(minx: f64, miny: f64, maxx: f64, maxy: f64) -> SegIndex {
        let cell = 1.0;
        let nx = ((maxx - minx) / cell).ceil() as usize + 4;
        let ny = ((maxy - miny) / cell).ceil() as usize + 4;
        SegIndex { segs: Vec::new(), x0: minx - 2.0 * cell, y0: miny - 2.0 * cell, cell, nx, ny, buckets: vec![Vec::new(); nx * ny], made: vec![0; nx * ny] }
    }
    fn bx(&self, x: f64) -> usize {
        ((x - self.x0) / self.cell).floor().clamp(0.0, (self.nx - 1) as f64) as usize
    }
    fn by(&self, y: f64) -> usize {
        ((y - self.y0) / self.cell).floor().clamp(0.0, (self.ny - 1) as f64) as usize
    }
    fn add(&mut self, s: Seg) -> u32 {
        let id = self.segs.len() as u32;
        let (x0, x1) = (self.bx(s.a.0.min(s.b.0) - s.half), self.bx(s.a.0.max(s.b.0) + s.half));
        let (y0, y1) = (self.by(s.a.1.min(s.b.1) - s.half), self.by(s.a.1.max(s.b.1) + s.half));
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.buckets[y * self.nx + x].push(id);
            }
        }
        self.segs.push(s);
        id
    }
    /// Is a piece of track a-b of this net at least its clearance from every other net's routed copper?
    fn clear(&self, a: (f64, f64), b: (f64, f64), half: f64, net: u16, lmask: u32, clr: &[f64]) -> bool {
        let mine = clr[net as usize];
        let grow = half + clr.iter().cloned().fold(0.0, f64::max) + EPS + CHORD + 1e-6;
        let (x0, x1) = (self.bx(a.0.min(b.0) - grow), self.bx(a.0.max(b.0) + grow));
        let (y0, y1) = (self.by(a.1.min(b.1) - grow), self.by(a.1.max(b.1) + grow));
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in &self.buckets[y * self.nx + x] {
                    let s = &self.segs[i as usize];
                    if s.net == net || s.lmask & lmask == 0 {
                        continue;
                    }
                    let need = half + s.half + mine.max(clr[s.net as usize]) + EPS + CHORD;
                    if seg_seg_dist2(a, b, s.a, s.b) < need * need {
                        return false;
                    }
                }
            }
        }
        true
    }
    /// How much has been rounded round a point since the count was last read.
    fn made_near(&self, p: (f64, f64)) -> u32 {
        let (cx, cy) = (self.bx(p.0), self.by(p.1));
        let mut n = 0;
        for y in cy.saturating_sub(1)..=(cy + 1).min(self.ny - 1) {
            for x in cx.saturating_sub(1)..=(cx + 1).min(self.nx - 1) {
                n += self.made[y * self.nx + x];
            }
        }
        n
    }
}

struct Corner {
    job: usize,
    track: usize,
    /// the vertex of the track's polyline
    i: usize,
    p: (f64, f64),
    u: (f64, f64),
    v: (f64, f64),
    /// the angle between the legs, and the tangent of half the turn
    phi: f64,
    tan: f64,
    /// the longest piece either leg may give
    tmax: f64,
    /// the radius that failed last, and what had been rounded round here then
    failed: Option<(f64, u32)>,
}

impl Router {
    /// Turn the corners of the routed tracks into arcs of up to `radius` mm. Returns (corners, rounded).
    pub fn round_corners(&mut self, radius: f64) -> (usize, usize) {
        if radius < MIN_RADIUS {
            return (0, 0);
        }
        let mut idx = SegIndex::new(self.f.wx(0), self.f.wy(0), self.f.wx(self.f.w - 1), self.f.wy(self.f.h - 1));
        let all: u32 = (1u32 << self.sc.layers.len()) - 1;
        let clr: Vec<f64> = (0..self.sc.nets.len()).map(|n| self.net_clr(n as u16)).collect();
        // every routed piece into the index; first[j][t]: the id of the track's first piece
        let mut first: Vec<Vec<u32>> = Vec::with_capacity(self.jobs.len());
        // per (net, layer): where tracks end; per net: its vias
        let mut ends: HashMap<(u16, u8), Vec<(f64, f64)>> = HashMap::new();
        let mut vias: HashMap<u16, Vec<(f64, f64)>> = HashMap::new();
        for j in &self.jobs {
            let mut f = Vec::with_capacity(j.tracks.len());
            for t in &j.tracks {
                f.push(idx.segs.len() as u32);
                for s in t.pts.windows(2) {
                    idx.add(Seg { a: s[0], b: s[1], half: t.width / 2.0, net: j.net, lmask: 1 << t.layer });
                }
                let e = ends.entry((j.net, t.layer)).or_default();
                e.push(t.pts[0]);
                e.push(t.pts[t.pts.len() - 1]);
            }
            first.push(f);
            for &(x, y) in &j.vias {
                let p = (self.f.wx(x as usize), self.f.wy(y as usize));
                idx.add(Seg { a: p, b: p, half: if j.small.contains(&(x, y)) { self.via_r_s } else { self.kind(j.net).r }, net: j.net, lmask: all });
                vias.entry(j.net).or_default().push(p);
            }
        }
        let near = |list: Option<&Vec<(f64, f64)>>, p: (f64, f64), t: f64| -> bool { list.map_or(false, |l| l.iter().any(|q| hypot(q.0 - p.0, q.1 - p.1) < t)) };

        let mut corners: Vec<Corner> = Vec::new();
        for (ji, j) in self.jobs.iter().enumerate() {
            for (ti, t) in j.tracks.iter().enumerate() {
                // (a pair's run keeps its corners: an arc each of the same radius would bring its two
                // tracks nearer each other in the turn)
                if j.trunk.as_ref().map_or(false, |k| k.track.pts == t.pts) {
                    continue;
                }
                let lmask = 1u32 << t.layer;
                for i in 1..t.pts.len().saturating_sub(1) {
                    let (a, p, c) = (t.pts[i - 1], t.pts[i], t.pts[i + 1]);
                    let (l1, l2) = (hypot(a.0 - p.0, a.1 - p.1), hypot(c.0 - p.0, c.1 - p.1));
                    if l1 < 1e-6 || l2 < 1e-6 {
                        continue;
                    }
                    let u = ((a.0 - p.0) / l1, (a.1 - p.1) / l1);
                    let v = ((c.0 - p.0) / l2, (c.1 - p.1) / l2);
                    let phi = (u.0 * v.0 + u.1 * v.1).clamp(-1.0, 1.0).acos();
                    let turn = std::f64::consts::PI - phi;
                    // nearly straight: nothing to round; a hairpin: left alone
                    if turn.to_degrees() < 5.0 || phi.to_degrees() < 40.0 {
                        continue;
                    }
                    // a track end, a via or a pad of the net right at the corner: it stays as it is
                    if near(ends.get(&(j.net, t.layer)), p, 1e-4) || near(vias.get(&j.net), p, 1e-4) || self.statics.own_pad_within(p, lmask, j.net, 1e-6) {
                        continue;
                    }
                    // (an arc takes half of each leg at most: two corners on one short leg meet in its
                    // middle, and nothing is left between them)
                    corners.push(Corner { job: ji, track: ti, i, p, u, v, phi, tan: (turn / 2.0).tan(), tmax: l1.min(l2) / 2.0, failed: None });
                }
            }
        }
        for j in self.jobs.iter_mut() {
            for t in j.tracks.iter_mut() {
                t.arcs = vec![None; t.pts.len()];
            }
        }

        let total = corners.len();
        let mut rounded = 0;
        let mut todo: Vec<Corner> = corners;
        let mut level = radius;
        while level >= MIN_RADIUS && !todo.is_empty() {
            for _ in 0..MAX_SWEEPS {
                let mut left: Vec<Corner> = Vec::with_capacity(todo.len());
                let mut progressed = false;
                for mut c in todo {
                    let r = level.min(c.tmax / c.tan);
                    if r < MIN_RADIUS {
                        continue; // its legs are too short for any arc worth having
                    }
                    let seen = idx.made_near(c.p);
                    if c.failed == Some((r, seen)) {
                        left.push(c);
                        continue;
                    }
                    let (net, layer, half) = {
                        let j = &self.jobs[c.job];
                        (j.net, j.tracks[c.track].layer, j.tracks[c.track].width / 2.0)
                    };
                    let lmask = 1u32 << layer;
                    let t = r * c.tan;
                    let s = (c.p.0 + c.u.0 * t, c.p.1 + c.u.1 * t);
                    let e = (c.p.0 + c.v.0 * t, c.p.1 + c.v.1 * t);
                    let bl = hypot(c.u.0 + c.v.0, c.u.1 + c.v.1);
                    let bis = ((c.u.0 + c.v.0) / bl, (c.u.1 + c.v.1) / bl);
                    let dc = r / (c.phi / 2.0).sin();
                    let ctr = (c.p.0 + bis.0 * dc, c.p.1 + bis.1 * dc);
                    let m = (ctr.0 - bis.0 * r, ctr.1 - bis.1 * r);
                    // the cut-off tip may be what carries a branch, a via or a pad of the net
                    let mut ok = !near(ends.get(&(net, layer)), c.p, t + 1e-3) && !near(vias.get(&net), c.p, t + 1e-3) && !self.statics.own_pad_within(c.p, lmask, net, t + 1e-3);
                    let chords = arc_points(s, m, e, CHORD);
                    if ok {
                        for w in chords.windows(2) {
                            if !self.statics.track_clear(w[0], w[1], half, clr[net as usize] + EPS, lmask, net) || !idx.clear(w[0], w[1], half, net, lmask, &clr) {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if !ok {
                        c.failed = Some((r, seen));
                        left.push(c);
                        continue;
                    }
                    // the two legs are cut back to the arc's ends, the arc's chords are copper now
                    let f0 = first[c.job][c.track] as usize;
                    idx.segs[f0 + c.i - 1].b = s;
                    idx.segs[f0 + c.i].a = e;
                    for w in chords.windows(2) {
                        idx.add(Seg { a: w[0], b: w[1], half, net, lmask });
                    }
                    let (bx, by) = (idx.bx(c.p.0), idx.by(c.p.1));
                    idx.made[by * idx.nx + bx] += 1;
                    self.jobs[c.job].tracks[c.track].arcs[c.i] = Some((s, m, e));
                    rounded += 1;
                    progressed = true;
                }
                todo = left;
                if !progressed {
                    break;
                }
            }
            level *= SHRINK;
        }
        (total, rounded)
    }
}
