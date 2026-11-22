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

/// What an object that asks less than the board's clearance (`extra` below 0: a keep-out, which asks
/// none) is stamped into the field with. The field holds one distance a node, the object's own grown by
/// the clearance it does not ask, and a track is cleared for a step between two nodes by that distance
/// with an allowance for the sag of the straight piece toward a corner (`Router::thr`). The sag is that
/// of a track as wide as its half-width and the clearance together; past a corner that asks no clearance
/// the real sag, of the half-width alone, is more. The difference at the narrowest track of the board
/// (half-width `half`), where it is most, comes off what is stamped, so the grid is sure only where the
/// piece is clear and the band below it is measured (`Tight`).
pub(crate) fn stamped(extra: f64, pitch: f64, half: f64) -> f64 {
    if extra >= 0.0 {
        return extra;
    }
    let (c, s) = (-extra, pitch * pitch / 2.0);
    extra + ((half * half + s).sqrt() - ((half + c) * (half + c) + s).sqrt() + c).max(0.0)
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

#[cfg(test)]
mod tests {
    use super::stamped;

    /// Two nodes a diagonal step apart, a corner that asks no clearance anywhere beside them: wherever
    /// the grid is sure of both nodes, the track between them is clear of the corner.
    #[test]
    fn where_the_grid_is_sure_beside_a_keep_out_the_track_is_clear_of_it() {
        let (p, c): (f64, f64) = (0.1, 0.2);
        for &low in &[0.05f64, 0.0635, 0.1] {
            for &half in &[low, 0.15, 0.25] {
                let sure = ((half + c) * (half + c) + p * p / 2.0).sqrt();
                let extra = stamped(-c, p, low);
                let (a, b) = ((0.0, 0.0), (p, -p));
                let mut tightest = f64::MAX;
                for i in -80..=120 {
                    for j in -120..=80 {
                        let k = (i as f64 * 0.005, j as f64 * 0.005);
                        let d = |q: (f64, f64)| crate::geom::hypot(k.0 - q.0, k.1 - q.1);
                        if d(a) - extra >= sure && d(b) - extra >= sure {
                            tightest = tightest.min(crate::geom::seg_seg_dist2(a, b, k, k).sqrt());
                        }
                    }
                }
                assert!(tightest >= half, "half {half}, narrowest {low}: {tightest}");
            }
        }
        assert_eq!(stamped(0.1, p, 0.05), 0.1, "what asks more than the board's clearance is stamped as it is");
    }
}
