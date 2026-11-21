//! The clearance field: for every grid node on every copper layer, how far the nearest copper is.
//!
//! Two fields share one cell. The static one holds what never moves (pads, the board edge, holes,
//! keep-outs); the dynamic one holds the copper the router has laid and may lift again. Each stores the
//! distance to the nearest object, that object's net, and the distance to the nearest object of any
//! *other* net. So "may net N put a track of half-width h here?" is two compares, for any N and any h,
//! with no per-net bookkeeping: if the nearest object is N's own, look at the second distance.
//!
//! Distances are in ticks of 0.01 mm, rounded down, `FAR` = far (a `Dist` holds 655 mm of them: a wide
//! rail's half-width and a mains net's clearance fit, as a signal's do).

use crate::geom::{seg_dist2, Shape};

/// A distance in ticks.
pub type Dist = u16;
pub const FAR: Dist = Dist::MAX;
pub const TICK: f64 = 0.01;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Cell {
    /// static: distance to the nearest fixed object
    pub sd1: Dist,
    /// static: distance to the nearest fixed object of a net other than `sn1`
    pub sd2: Dist,
    /// dynamic: the same pair for routed copper
    pub dd1: Dist,
    pub dd2: Dist,
    pub sn1: u16,
    pub dn1: u16,
    /// the one net that does not pay this node's toll (0: every net pays)
    pub toll_net: u16,
    /// for the node at a fence's far end: the heading (0..7) in which the fence leaves its pad; NO_DIR
    /// elsewhere. A track of the fence's net starts here in that heading and may not arrive against it.
    pub fdir: u8,
    /// what a step onto this node costs more, as a price class of the cost terms (`search::Costs`);
    /// 0: nothing
    pub toll: u8,
}

const _: () = assert!(std::mem::size_of::<Cell>() == 16);

pub const NO_DIR: u8 = 255;
const EMPTY: Cell = Cell { sd1: FAR, sd2: FAR, dd1: FAR, dd2: FAR, sn1: 0, dn1: 0, toll_net: 0, fdir: NO_DIR, toll: 0 };

/// What an object in the field is (docs/hooks.md, "Copper kind").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// the board's edge, a hole, a keep-out: not copper, but in the way like copper
    Edge,
    Hole,
    Keepout,
    Pad,
    /// the exact stub out of a fine-pitch pad
    Fence,
    Track,
    Via,
    Zone,
    /// copper the board came with
    Existing,
}

/// `job` of an object that no job laid.
pub const NO_JOB: u32 = u32::MAX;

/// A piece of routed copper: a track segment (a capsule on one layer) or a via (a == b, every layer).
#[derive(Clone)]
pub struct DynObj {
    pub kind: Kind,
    pub net: u16,
    /// the job that laid it, or NO_JOB
    pub job: u32,
    /// locked copper is never lifted
    pub locked: bool,
    /// The copper layer, or ALL_LAYERS for a via.
    pub layer: u8,
    pub ax: f64,
    pub ay: f64,
    pub bx: f64,
    pub by: f64,
    pub r: f64,
    pub alive: bool,
}

pub const ALL_LAYERS: u8 = 0xff;
const BSHIFT: usize = 4;

pub struct Field {
    pub w: usize,
    pub h: usize,
    pub nl: usize,
    pub pitch: f64,
    pub x0: f64,
    pub y0: f64,
    pub cells: Vec<Cell>,
    /// How far from an object its distance is recorded, mm. Past it a node reads FAR.
    pub rmax: f64,
    pub objs: Vec<DynObj>,
    bw: usize,
    bh: usize,
    buckets: Vec<Vec<u32>>,
    dirty_mark: Vec<bool>,
    /// How near the middle of a routed via another via's middle may stand, mm, by the board's hole to
    /// hole rule (0: the board has no such rule). The router sets it once, before any via is laid.
    pub hole_reach: f64,
    /// Per node (y * w + x): how many live routed vias stand nearer than `hole_reach`.
    pub holes: Vec<u16>,
}

#[inline]
fn quant(d: f64) -> Dist {
    let q = (d / TICK).floor();
    if q >= 65534.0 {
        65534
    } else if q <= 0.0 {
        0
    } else {
        q as Dist
    }
}

impl Field {
    /// The grid over a box, mm: where it starts and how many nodes it has each way. Its nodes sit at
    /// `off` plus multiples of the pitch.
    pub fn grid(minx: f64, miny: f64, maxx: f64, maxy: f64, pitch: f64, off: (f64, f64)) -> (f64, f64, usize, usize) {
        let x0 = (((minx - off.0) / pitch).floor() - 2.0) * pitch + off.0;
        let y0 = (((miny - off.1) / pitch).floor() - 2.0) * pitch + off.1;
        let w = ((maxx - x0) / pitch).ceil() as usize + 3;
        let h = ((maxy - y0) / pitch).ceil() as usize + 3;
        (x0, y0, w, h)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(minx: f64, miny: f64, maxx: f64, maxy: f64, pitch: f64, nl: usize, rmax: f64, off: (f64, f64)) -> Field {
        let (x0, y0, w, h) = Field::grid(minx, miny, maxx, maxy, pitch, off);
        let bw = (w >> BSHIFT) + 1;
        let bh = (h >> BSHIFT) + 1;
        Field {
            w,
            h,
            nl,
            pitch,
            x0,
            y0,
            cells: vec![EMPTY; w * h * nl],
            rmax,
            objs: Vec::new(),
            bw,
            bh,
            buckets: vec![Vec::new(); bw * bh * nl],
            dirty_mark: vec![false; bw * bh * nl],
            hole_reach: 0.0,
            holes: vec![0; w * h],
        }
    }

    #[inline]
    pub fn idx(&self, l: usize, y: usize, x: usize) -> usize {
        (l * self.h + y) * self.w + x
    }
    #[inline]
    pub fn wx(&self, x: usize) -> f64 {
        self.x0 + x as f64 * self.pitch
    }
    #[inline]
    pub fn wy(&self, y: usize) -> f64 {
        self.y0 + y as f64 * self.pitch
    }
    /// The node nearest a point, clamped to the grid.
    pub fn node_near(&self, x: f64, y: f64) -> (usize, usize) {
        let gx = ((x - self.x0) / self.pitch).round().clamp(0.0, (self.w - 1) as f64) as usize;
        let gy = ((y - self.y0) / self.pitch).round().clamp(0.0, (self.h - 1) as f64) as usize;
        (gx, gy)
    }

    /// Node range covering a box in mm, clamped; None when it misses the grid.
    pub(crate) fn range(&self, minx: f64, miny: f64, maxx: f64, maxy: f64) -> Option<(usize, usize, usize, usize)> {
        let x0 = ((minx - self.x0) / self.pitch).ceil().max(0.0);
        let y0 = ((miny - self.y0) / self.pitch).ceil().max(0.0);
        let x1 = ((maxx - self.x0) / self.pitch).floor().min((self.w - 1) as f64);
        let y1 = ((maxy - self.y0) / self.pitch).floor().min((self.h - 1) as f64);
        if x1 < x0 || y1 < y0 {
            return None;
        }
        Some((x0 as usize, y0 as usize, x1 as usize, y1 as usize))
    }

    /// Record a fixed object. `extra` (mm) is how much more than the board's clearance it asks for
    /// (the edge rule, a fiducial's own clearance); negative for less (a keep-out wants none).
    pub fn stamp_static(&mut self, shape: &Shape, lmask: u32, net: u16, extra: f64) {
        let reach = self.rmax + extra.max(0.0);
        let (bx0, by0, bx1, by1) = shape.bbox();
        let Some((x0, y0, x1, y1)) = self.range(bx0 - reach, by0 - reach, bx1 + reach, by1 + reach) else { return };
        for y in y0..=y1 {
            let py = self.wy(y);
            for x in x0..=x1 {
                let d = shape.dist(self.wx(x), py) - extra;
                if d >= self.rmax {
                    continue;
                }
                let q = quant(d);
                for l in 0..self.nl {
                    if lmask & (1 << l) == 0 {
                        continue;
                    }
                    let i = (l * self.h + y) * self.w + x;
                    let c = &mut self.cells[i];
                    if net == c.sn1 {
                        if q < c.sd1 {
                            c.sd1 = q;
                        }
                    } else if q < c.sd1 {
                        c.sd2 = c.sd1;
                        c.sd1 = q;
                        c.sn1 = net;
                    } else if q < c.sd2 {
                        c.sd2 = q;
                    }
                }
            }
        }
    }

    fn obj_range(&self, o: &DynObj) -> Option<(usize, usize, usize, usize)> {
        let m = o.r + self.rmax;
        self.range(o.ax.min(o.bx) - m, o.ay.min(o.by) - m, o.ax.max(o.bx) + m, o.ay.max(o.by) + m)
    }

    /// Write one object's distances into the dynamic field, within `clip` (nodes, inclusive) on layer l.
    fn stamp_obj(&mut self, id: usize, l: usize, clip: (usize, usize, usize, usize)) {
        let o = &self.objs[id];
        let Some((mut x0, mut y0, mut x1, mut y1)) = self.obj_range(o) else { return };
        x0 = x0.max(clip.0);
        y0 = y0.max(clip.1);
        x1 = x1.min(clip.2);
        y1 = y1.min(clip.3);
        if x1 < x0 || y1 < y0 {
            return;
        }
        let (ax, ay, bx, by, r, net) = (o.ax, o.ay, o.bx, o.by, o.r, o.net);
        let lim = r + self.rmax;
        let lim2 = lim * lim;
        for y in y0..=y1 {
            let py = self.y0 + y as f64 * self.pitch;
            let row = (l * self.h + y) * self.w;
            for x in x0..=x1 {
                let px = self.x0 + x as f64 * self.pitch;
                let d2 = seg_dist2(px, py, ax, ay, bx, by);
                if d2 >= lim2 {
                    continue;
                }
                let q = quant(d2.sqrt() - r);
                let c = &mut self.cells[row + x];
                if net == c.dn1 {
                    if q < c.dd1 {
                        c.dd1 = q;
                    }
                } else if q < c.dd1 {
                    c.dd2 = c.dd1;
                    c.dd1 = q;
                    c.dn1 = net;
                } else if q < c.dd2 {
                    c.dd2 = q;
                }
            }
        }
    }

    fn layers_of(&self, layer: u8) -> std::ops::Range<usize> {
        if layer == ALL_LAYERS {
            0..self.nl
        } else {
            layer as usize..layer as usize + 1
        }
    }

    /// Lay a piece of copper: it is written into the dynamic field and remembered so it can be lifted.
    /// A routed via's hole into `holes`, or out of it.
    fn count_hole(&mut self, x: f64, y: f64, add: bool) {
        if self.hole_reach <= 0.0 {
            return;
        }
        let r = self.hole_reach;
        let ((x0, y0), (x1, y1)) = (self.node_near(x - r, y - r), self.node_near(x + r, y + r));
        for ny in y0..=y1 {
            for nx in x0..=x1 {
                if (self.wx(nx) - x).hypot(self.wy(ny) - y) < r {
                    let c = &mut self.holes[ny * self.w + nx];
                    *c = if add { c.saturating_add(1) } else { c.saturating_sub(1) };
                }
            }
        }
    }

    /// The live routed vias of other nets whose middle is nearer than `hole_reach` to a node: (job, net).
    pub fn holes_near(&self, y: usize, x: usize, net: u16, out: &mut Vec<(u32, u16)>) {
        let (px, py) = (self.wx(x), self.wy(y));
        for &i in &self.buckets[(y >> BSHIFT) * self.bw + (x >> BSHIFT)] {
            let o = &self.objs[i as usize];
            if o.alive && o.kind == Kind::Via && o.net != net && (o.ax - px).hypot(o.ay - py) < self.hole_reach && !out.contains(&(o.job, o.net)) {
                out.push((o.job, o.net));
            }
        }
    }

    pub fn add_obj(&mut self, o: DynObj) -> u32 {
        let id = self.objs.len();
        if o.kind == Kind::Via {
            self.count_hole(o.ax, o.ay, true);
        }
        let rng = self.obj_range(&o);
        let layers = self.layers_of(o.layer);
        self.objs.push(o);
        if let Some((x0, y0, x1, y1)) = rng {
            for l in layers {
                self.stamp_obj(id, l, (x0, y0, x1, y1));
                for by in (y0 >> BSHIFT)..=(y1 >> BSHIFT) {
                    for bx in (x0 >> BSHIFT)..=(x1 >> BSHIFT) {
                        self.buckets[(l * self.bh + by) * self.bw + bx].push(id as u32);
                    }
                }
            }
        }
        id as u32
    }

    /// Lock or unlock copper: locked copper is never lifted.
    pub fn set_locked(&mut self, ids: &[u32], locked: bool) {
        for &id in ids {
            self.objs[id as usize].locked = locked;
        }
    }

    /// Lift copper: the blocks it reached are wiped and rebuilt from the copper that stays. Locked
    /// copper is refused: it stays, and the number refused is returned.
    pub fn remove_objs(&mut self, ids: &[u32]) -> usize {
        let mut dirty: Vec<usize> = Vec::new();
        let mut refused = 0;
        for &id in ids {
            let id = id as usize;
            if !self.objs[id].alive {
                continue;
            }
            if self.objs[id].locked {
                refused += 1;
                continue;
            }
            self.objs[id].alive = false;
            if self.objs[id].kind == Kind::Via {
                self.count_hole(self.objs[id].ax, self.objs[id].ay, false);
            }
            let Some((x0, y0, x1, y1)) = self.obj_range(&self.objs[id]) else { continue };
            for l in self.layers_of(self.objs[id].layer) {
                for by in (y0 >> BSHIFT)..=(y1 >> BSHIFT) {
                    for bx in (x0 >> BSHIFT)..=(x1 >> BSHIFT) {
                        let b = (l * self.bh + by) * self.bw + bx;
                        if !self.dirty_mark[b] {
                            self.dirty_mark[b] = true;
                            dirty.push(b);
                        }
                    }
                }
            }
        }
        for b in dirty {
            self.dirty_mark[b] = false;
            let l = b / (self.bw * self.bh);
            let by = (b / self.bw) % self.bh;
            let bx = b % self.bw;
            let x0 = bx << BSHIFT;
            let y0 = by << BSHIFT;
            let x1 = (x0 + (1 << BSHIFT) - 1).min(self.w - 1);
            let y1 = (y0 + (1 << BSHIFT) - 1).min(self.h - 1);
            if x0 >= self.w || y0 >= self.h {
                continue;
            }
            for y in y0..=y1 {
                let row = (l * self.h + y) * self.w;
                for c in &mut self.cells[row + x0..=row + x1] {
                    c.dd1 = FAR;
                    c.dd2 = FAR;
                    c.dn1 = 0;
                }
            }
            let mut list = std::mem::take(&mut self.buckets[b]);
            list.retain(|&i| self.objs[i as usize].alive);
            for &i in &list {
                self.stamp_obj(i as usize, l, (x0, y0, x1, y1));
            }
            self.buckets[b] = list;
        }
        refused
    }

    /// Every piece of live routed copper on layer l that may lie within the field's reach of the straight
    /// piece a-b (a piece that reaches further than a block is given more than once).
    pub fn each_near(&self, l: usize, a: (f64, f64), b: (f64, f64), mut f: impl FnMut(&DynObj)) {
        let ((x0, y0), (x1, y1)) = (self.node_near(a.0.min(b.0), a.1.min(b.1)), self.node_near(a.0.max(b.0), a.1.max(b.1)));
        for by in (y0 >> BSHIFT)..=(y1 >> BSHIFT) {
            for bx in (x0 >> BSHIFT)..=(x1 >> BSHIFT) {
                for &i in &self.buckets[(l * self.bh + by) * self.bw + bx] {
                    let o = &self.objs[i as usize];
                    if o.alive {
                        f(o);
                    }
                }
            }
        }
    }

    /// The live routed copper of other nets within `reach` mm of a node on layer l: (job, net) pairs.
    pub fn foreign_near(&self, l: usize, y: usize, x: usize, net: u16, reach: f64, out: &mut Vec<(u32, u16)>) {
        let b = (l * self.bh + (y >> BSHIFT)) * self.bw + (x >> BSHIFT);
        let (px, py) = (self.wx(x), self.wy(y));
        for &i in &self.buckets[b] {
            let o = &self.objs[i as usize];
            if !o.alive || o.net == net {
                continue;
            }
            let d = seg_dist2(px, py, o.ax, o.ay, o.bx, o.by).sqrt() - o.r;
            if d < reach && !out.contains(&(o.job, o.net)) {
                out.push((o.job, o.net));
            }
        }
    }
}

/// Static clearance left for net `net` at a cell, in ticks.
#[inline(always)]
pub fn sdist(c: &Cell, net: u16) -> Dist {
    if c.sn1 == net {
        c.sd2
    } else {
        c.sd1
    }
}

/// Dynamic clearance left for net `net` at a cell, in ticks.
#[inline(always)]
pub fn ddist(c: &Cell, net: u16) -> Dist {
    if c.dn1 == net {
        c.dd2
    } else {
        c.dd1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(net: u16, job: u32, y: f64, locked: bool) -> DynObj {
        DynObj { kind: Kind::Track, net, job, locked, layer: 0, ax: 1.0, ay: y, bx: 4.0, by: y, r: 0.1, alive: true }
    }

    #[test]
    fn lifting_takes_copper_out_of_the_field_and_refuses_locked_copper() {
        let mut f = Field::new(0.0, 0.0, 5.0, 5.0, 0.1, 2, 0.5, (0.0, 0.0));
        let free = f.add_obj(track(1, 0, 1.0, false));
        let held = f.add_obj(track(2, 1, 3.0, true));
        let (on_free, on_held) = (f.idx(0, f.node_near(2.0, 1.0).1, f.node_near(2.0, 1.0).0), f.idx(0, f.node_near(2.0, 3.0).1, f.node_near(2.0, 3.0).0));
        assert_eq!((f.cells[on_free].dd1, f.cells[on_free].dn1), (0, 1));
        assert_eq!((f.cells[on_held].dd1, f.cells[on_held].dn1), (0, 2));
        // both asked for: the free one goes, the locked one is refused and still reads as copper
        assert_eq!(f.remove_objs(&[free, held]), 1);
        assert!(!f.objs[free as usize].alive && f.objs[held as usize].alive);
        assert_eq!(f.cells[on_free].dd1, FAR);
        assert_eq!((f.cells[on_held].dd1, f.cells[on_held].dn1), (0, 2));
        // unlocked, it lifts like any other
        f.set_locked(&[held], false);
        assert_eq!(f.remove_objs(&[held]), 0);
        assert_eq!(f.cells[on_held].dd1, FAR);
    }
}
