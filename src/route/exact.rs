//! The exact index: the fixed copper as shapes, for the questions the grid cannot answer.

use crate::field::{Field, Kind};
use crate::geom::{Shape, SLACK};

/// The fixed copper as shapes, for exact questions the grid cannot answer (is this off-grid piece of
/// track clear?).
pub struct StaticObj {
    pub kind: Kind,
    pub shape: Shape,
    pub lmask: u32,
    pub net: u16,
    pub extra: f64,
    /// the job that laid it, or NO_JOB (the board's own: pads, the edge, holes, keep-outs)
    pub job: u32,
    /// fixed copper is locked: nothing lifts it
    pub locked: bool,
}

impl StaticObj {
    /// An object of the board itself: no job laid it, nothing lifts it.
    pub fn fixed(kind: Kind, shape: Shape, lmask: u32, net: u16, extra: f64) -> StaticObj {
        StaticObj { kind, shape, lmask, net, extra, job: crate::field::NO_JOB, locked: true }
    }
}

pub struct StaticIndex {
    pub objs: Vec<StaticObj>,
    pub(crate) x0: f64,
    pub(crate) y0: f64,
    pub(crate) cell: f64,
    pub(crate) nx: usize,
    pub(crate) ny: usize,
    pub(crate) reach: f64,
    pub(crate) buckets: Vec<Vec<u32>>,
}

impl StaticIndex {
    pub(crate) fn new(minx: f64, miny: f64, maxx: f64, maxy: f64, reach: f64) -> StaticIndex {
        let cell = 2.0;
        let nx = ((maxx - minx) / cell).ceil() as usize + 2;
        let ny = ((maxy - miny) / cell).ceil() as usize + 2;
        StaticIndex { objs: Vec::new(), x0: minx - cell, y0: miny - cell, cell, nx, ny, reach, buckets: vec![Vec::new(); nx * ny] }
    }
    pub(crate) fn bucket(&self, x: f64, y: f64) -> (usize, usize) {
        let bx = ((x - self.x0) / self.cell).floor().clamp(0.0, (self.nx - 1) as f64) as usize;
        let by = ((y - self.y0) / self.cell).floor().clamp(0.0, (self.ny - 1) as f64) as usize;
        (bx, by)
    }
    pub(crate) fn add(&mut self, o: StaticObj) {
        let (x0, y0, x1, y1) = o.shape.bbox();
        let m = self.reach + o.extra.max(0.0);
        let (bx0, by0) = self.bucket(x0 - m, y0 - m);
        let (bx1, by1) = self.bucket(x1 + m, y1 + m);
        let id = self.objs.len() as u32;
        for by in by0..=by1 {
            for bx in bx0..=bx1 {
                self.buckets[by * self.nx + bx].push(id);
            }
        }
        self.objs.push(o);
    }
    /// Is a pad of this net on these layers nearer than `t` to a point?
    pub(crate) fn own_pad_within(&self, p: (f64, f64), lmask: u32, net: u16, t: f64) -> bool {
        let (cx, cy) = self.bucket(p.0, p.1);
        for by in cy.saturating_sub(1)..=(cy + 1).min(self.ny - 1) {
            for bx in cx.saturating_sub(1)..=(cx + 1).min(self.nx - 1) {
                for &i in &self.buckets[by * self.nx + bx] {
                    let o = &self.objs[i as usize];
                    if o.kind == Kind::Pad && o.net == net && o.lmask & lmask != 0 && o.shape.dist(p.0, p.1) < t {
                        return true;
                    }
                }
            }
        }
        false
    }
    /// Is a straight piece of track a-b of half-width `half` at least `clr` from every other net's fixed
    /// copper? Measured exactly, piece against shape, and to within `SLACK`, as everywhere.
    pub(crate) fn track_clear(&self, a: (f64, f64), b: (f64, f64), half: f64, clr: f64, lmask: u32, net: u16) -> bool {
        let (bx0, by0) = self.bucket(a.0.min(b.0), a.1.min(b.1));
        let (bx1, by1) = self.bucket(a.0.max(b.0), a.1.max(b.1));
        let need = half + clr - SLACK;
        for by in by0..=by1 {
            for bx in bx0..=bx1 {
                for &i in &self.buckets[by * self.nx + bx] {
                    let o = &self.objs[i as usize];
                    if o.lmask & lmask == 0 || (o.net == net && net != 0) {
                        continue;
                    }
                    if o.shape.dist_seg(a, b) - o.extra < need {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// What measures a piece of track of one net exactly, where the grid cannot tell (`Router::t_low`): the
/// fixed copper as shapes, the routed copper as it lies, and every net's clearance.
pub struct Tight<'a> {
    pub statics: &'a StaticIndex,
    /// per net: the clearance it keeps (the board's, or its own when that is more), and how much more
    /// than the board's that is
    pub clr: &'a [f64],
    pub more: &'a [f64],
}

impl Tight<'_> {
    /// Is the piece a-b of half-width `half` clear of every other net's fixed copper on layer l?
    pub fn fixed(&self, a: (f64, f64), b: (f64, f64), half: f64, net: u16, l: usize) -> bool {
        self.statics.track_clear(a, b, half, self.clr[net as usize], 1 << l, net)
    }

    /// How far short of its clearance from a piece of routed copper the piece a-b is, when it is.
    fn short(&self, o: &crate::field::DynObj, a: (f64, f64), b: (f64, f64), half: f64, net: u16) -> bool {
        let need = half + (o.r - self.more[o.net as usize]) + self.clr[net as usize].max(self.clr[o.net as usize]) - SLACK;
        crate::geom::seg_seg_dist2(a, b, (o.ax, o.ay), (o.bx, o.by)) < need * need
    }

    /// Is it clear of every other net's routed copper on layer l?
    pub fn routed(&self, f: &Field, a: (f64, f64), b: (f64, f64), half: f64, net: u16, l: usize) -> bool {
        let mut clear = true;
        f.each_near(l, a, b, |o| clear &= o.net == net || !self.short(o, a, b, half, net));
        clear
    }

    /// The jobs whose routed copper it is too near, with their nets, added to `out`.
    pub fn routed_hits(&self, f: &Field, a: (f64, f64), b: (f64, f64), half: f64, net: u16, l: usize, out: &mut Vec<(u32, u16)>) {
        f.each_near(l, a, b, |o| {
            if o.net != net && self.short(o, a, b, half, net) && !out.contains(&(o.job, o.net)) {
                out.push((o.job, o.net));
            }
        });
    }
}
