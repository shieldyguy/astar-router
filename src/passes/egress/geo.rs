//! Exact distances between two shapes, with the points where they are nearest.

use crate::geom::{hypot, Shape};

/// A shape as a core (a point, a segment, a closed polygon) grown by a radius.
struct Core {
    pts: Vec<(f64, f64)>,
    r: f64,
    closed: bool,
}

fn core(s: &Shape) -> Core {
    match s {
        Shape::Poly { pts, r } => Core { pts: pts.clone(), r: *r, closed: true },
        Shape::Circle { x, y, r } => Core { pts: vec![(*x, *y)], r: *r, closed: false },
        Shape::Capsule { ax, ay, bx, by, r } => Core { pts: vec![(*ax, *ay), (*bx, *by)], r: *r, closed: false },
    }
}

fn segs(c: &Core) -> Vec<((f64, f64), (f64, f64))> {
    let n = c.pts.len();
    if n == 1 {
        return vec![(c.pts[0], c.pts[0])];
    }
    let mut v: Vec<_> = c.pts.windows(2).map(|w| (w[0], w[1])).collect();
    if c.closed && n > 2 {
        v.push((c.pts[n - 1], c.pts[0]));
    }
    v
}

fn foot(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (a.0 + t * dx, a.1 + t * dy)
}

fn cross(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Do the segments a and b cross each other properly or touch?
fn meet(a: ((f64, f64), (f64, f64)), b: ((f64, f64), (f64, f64))) -> bool {
    let (d1, d2) = (cross(a.0, a.1, b.0), cross(a.0, a.1, b.1));
    let (d3, d4) = (cross(b.0, b.1, a.0), cross(b.0, b.1, a.1));
    (d1 * d2 < 0.0) && (d3 * d4 < 0.0)
}

/// The nearest points of two segments: (distance, on a, on b). Where they are nearest along a whole
/// stretch (parallel segments that face each other) the middle of that stretch is given.
pub fn seg_seg(a: ((f64, f64), (f64, f64)), b: ((f64, f64), (f64, f64))) -> (f64, (f64, f64), (f64, f64)) {
    if meet(a, b) {
        let m = foot(a.0, b.0, b.1);
        return (0.0, m, m);
    }
    let cands = [(a.0, foot(a.0, b.0, b.1)), (a.1, foot(a.1, b.0, b.1)), (foot(b.0, a.0, a.1), b.0), (foot(b.1, a.0, a.1), b.1)];
    let d = |c: &((f64, f64), (f64, f64))| hypot(c.0 .0 - c.1 .0, c.0 .1 - c.1 .1);
    let best = cands.iter().map(d).fold(f64::MAX, f64::min);
    let (mut pa, mut pb, mut n) = ((0.0, 0.0), (0.0, 0.0), 0.0);
    for c in &cands {
        if d(c) <= best + 1e-9 {
            pa = (pa.0 + c.0 .0, pa.1 + c.0 .1);
            pb = (pb.0 + c.1 .0, pb.1 + c.1 .1);
            n += 1.0;
        }
    }
    (best, (pa.0 / n, pa.1 / n), (pb.0 / n, pb.1 / n))
}

/// How far apart the copper of two shapes is, and the points on each where they are nearest:
/// (distance, on a, on b). 0 when they touch or overlap.
pub fn sep(a: &Shape, b: &Shape) -> (f64, (f64, f64), (f64, f64)) {
    let (ca, cb) = (core(a), core(b));
    // one inside the other
    if a.dist(cb.pts[0].0, cb.pts[0].1) <= 0.0 || b.dist(ca.pts[0].0, ca.pts[0].1) <= 0.0 {
        return (0.0, cb.pts[0], cb.pts[0]);
    }
    let (sa, sb) = (segs(&ca), segs(&cb));
    let mut all: Vec<(f64, (f64, f64), (f64, f64))> = Vec::with_capacity(sa.len() * sb.len());
    for &x in &sa {
        for &y in &sb {
            all.push(seg_seg(x, y));
        }
    }
    let best = all.iter().map(|c| c.0).fold(f64::MAX, f64::min);
    let (mut pa, mut pb, mut n) = ((0.0, 0.0), (0.0, 0.0), 0.0);
    for c in &all {
        if c.0 <= best + 1e-9 {
            pa = (pa.0 + c.1 .0, pa.1 + c.1 .1);
            pb = (pb.0 + c.2 .0, pb.1 + c.2 .1);
            n += 1.0;
        }
    }
    let (pa, pb) = ((pa.0 / n, pa.1 / n), (pb.0 / n, pb.1 / n));
    let d = best - ca.r - cb.r;
    if d <= 0.0 || best < 1e-12 {
        return (0.0, pa, pb);
    }
    let (ux, uy) = ((pb.0 - pa.0) / best, (pb.1 - pa.1) / best);
    (d, (pa.0 + ux * ca.r, pa.1 + uy * ca.r), (pb.0 - ux * cb.r, pb.1 - uy * cb.r))
}

/// The least distance between two polylines (each a list of points; one point is a point).
pub fn line_line(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    let seg = |p: &[(f64, f64)]| -> Vec<((f64, f64), (f64, f64))> {
        if p.len() == 1 {
            vec![(p[0], p[0])]
        } else {
            p.windows(2).map(|w| (w[0], w[1])).collect()
        }
    };
    let mut best = f64::MAX;
    for &x in &seg(a) {
        for &y in &seg(b) {
            best = best.min(seg_seg(x, y).0);
        }
    }
    best
}

/// The compass heading (0..8, the search's DX/DY order: E, SE, S, SW, W, NW, N, NE; y runs down) nearest a direction.
pub fn compass(dx: f64, dy: f64) -> u8 {
    let mut best = (f64::MIN, 0u8);
    for d in 0..8 {
        let (x, y) = (crate::search::DX[d] as f64, crate::search::DY[d] as f64);
        let dot = (x * dx + y * dy) / hypot(x, y);
        if dot > best.0 + 1e-12 {
            best = (dot, d as u8);
        }
    }
    best.1
}

pub const COMPASS: [&str; 9] = ["E", "SE", "S", "SW", "W", "NW", "N", "NE", "C"];

/// A compass heading as a unit vector.
pub fn unit(d: u8) -> (f64, f64) {
    let (x, y) = (crate::search::DX[d as usize] as f64, crate::search::DY[d as usize] as f64);
    let l = hypot(x, y);
    (x / l, y / l)
}
