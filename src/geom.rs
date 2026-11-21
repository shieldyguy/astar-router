//! Shapes and distances, in millimetres.

#[derive(Clone, Debug)]
pub enum Shape {
    /// A polygon grown by `r` (r = 0: the polygon itself).
    Poly { pts: Vec<(f64, f64)>, r: f64 },
    Circle { x: f64, y: f64, r: f64 },
    /// A segment a-b grown by `r`: a track, an oval pad.
    Capsule { ax: f64, ay: f64, bx: f64, by: f64, r: f64 },
}

/// How much short of its clearance astar lets a piece of copper be, in mm: the same everywhere, in the
/// grid's thresholds and in every exact check of a piece against what stands beside it. KiCad's DRC
/// lets a clearance be short by 0.5 um; 0.4 um of that is used, and the rest covers the rounding of
/// what is written. It is what lets a track as wide as its pad leave a row of pads whose gap is the
/// board's clearance exactly.
pub const SLACK: f64 = 0.0004;

pub fn hypot(a: f64, b: f64) -> f64 {
    (a * a + b * b).sqrt()
}

fn orient(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Squared distance between segments a-b and c-d: 0 where they cross.
pub fn seg_seg_dist2(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> f64 {
    let (d1, d2, d3, d4) = (orient(c, d, a), orient(c, d, b), orient(a, b, c), orient(a, b, d));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return 0.0;
    }
    seg_dist2(a.0, a.1, c.0, c.1, d.0, d.1)
        .min(seg_dist2(b.0, b.1, c.0, c.1, d.0, d.1))
        .min(seg_dist2(c.0, c.1, a.0, a.1, b.0, b.1))
        .min(seg_dist2(d.0, d.1, a.0, a.1, b.0, b.1))
}

/// Squared distance from point p to segment a-b.
#[inline]
pub fn seg_dist2(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let qx = ax + t * dx - px;
    let qy = ay + t * dy - py;
    qx * qx + qy * qy
}

impl Shape {
    /// Distance from a point to the shape's copper: 0 inside it.
    pub fn dist(&self, px: f64, py: f64) -> f64 {
        match self {
            Shape::Circle { x, y, r } => (((px - x) * (px - x) + (py - y) * (py - y)).sqrt() - r).max(0.0),
            Shape::Capsule { ax, ay, bx, by, r } => (seg_dist2(px, py, *ax, *ay, *bx, *by).sqrt() - r).max(0.0),
            Shape::Poly { pts, r } => {
                let n = pts.len();
                if n == 0 {
                    return f64::MAX;
                }
                let mut inside = false;
                let mut best = f64::MAX;
                let mut j = n - 1;
                for i in 0..n {
                    let (xi, yi) = pts[i];
                    let (xj, yj) = pts[j];
                    if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
                        inside = !inside;
                    }
                    let d2 = seg_dist2(px, py, xi, yi, xj, yj);
                    if d2 < best {
                        best = d2;
                    }
                    j = i;
                }
                if inside {
                    0.0
                } else {
                    (best.sqrt() - r).max(0.0)
                }
            }
        }
    }

    /// Distance from the segment a-b to the shape's copper, exactly: 0 where it touches or lies inside.
    pub fn dist_seg(&self, a: (f64, f64), b: (f64, f64)) -> f64 {
        match self {
            Shape::Circle { x, y, r } => (seg_dist2(*x, *y, a.0, a.1, b.0, b.1).sqrt() - r).max(0.0),
            Shape::Capsule { ax, ay, bx, by, r } => (seg_seg_dist2(a, b, (*ax, *ay), (*bx, *by)).sqrt() - r).max(0.0),
            Shape::Poly { pts, r } => {
                // (an end inside the polygon; else the nearest edge, which a crossing makes 0)
                if pts.is_empty() {
                    return f64::MAX;
                }
                if self.dist(a.0, a.1) == 0.0 || self.dist(b.0, b.1) == 0.0 {
                    return 0.0;
                }
                let mut best = f64::MAX;
                let mut j = pts.len() - 1;
                for i in 0..pts.len() {
                    best = best.min(seg_seg_dist2(a, b, pts[i], pts[j]));
                    j = i;
                }
                (best.sqrt() - r).max(0.0)
            }
        }
    }

    /// How far inside the copper a point is: its distance to the edge, 0 when it is outside.
    pub fn depth(&self, px: f64, py: f64) -> f64 {
        match self {
            Shape::Circle { x, y, r } => (r - ((px - x) * (px - x) + (py - y) * (py - y)).sqrt()).max(0.0),
            Shape::Capsule { ax, ay, bx, by, r } => (r - seg_dist2(px, py, *ax, *ay, *bx, *by).sqrt()).max(0.0),
            Shape::Poly { pts, r } => {
                if self.dist(px, py) > 0.0 {
                    return 0.0;
                }
                let n = pts.len();
                let mut best = f64::MAX;
                let mut inside = false;
                let mut j = n - 1;
                for i in 0..n {
                    let (xi, yi) = pts[i];
                    let (xj, yj) = pts[j];
                    if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
                        inside = !inside;
                    }
                    best = best.min(seg_dist2(px, py, xi, yi, xj, yj));
                    j = i;
                }
                if inside {
                    best.sqrt() + r
                } else {
                    (r - best.sqrt()).max(0.0)
                }
            }
        }
    }

    /// (min x, min y, max x, max y) of the copper.
    pub fn bbox(&self) -> (f64, f64, f64, f64) {
        match self {
            Shape::Circle { x, y, r } => (x - r, y - r, x + r, y + r),
            Shape::Capsule { ax, ay, bx, by, r } => (ax.min(*bx) - r, ay.min(*by) - r, ax.max(*bx) + r, ay.max(*by) + r),
            Shape::Poly { pts, r } => {
                let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for &(x, y) in pts {
                    b.0 = b.0.min(x);
                    b.1 = b.1.min(y);
                    b.2 = b.2.max(x);
                    b.3 = b.3.max(y);
                }
                (b.0 - r, b.1 - r, b.2 + r, b.3 + r)
            }
        }
    }
}

/// An arc through start, mid, end as a polyline whose chords stay within `err` of the arc.
pub fn arc_points(s: (f64, f64), m: (f64, f64), e: (f64, f64), err: f64) -> Vec<(f64, f64)> {
    let (ax, ay) = s;
    let (bx, by) = m;
    let (cx, cy) = e;
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-12 {
        return vec![s, e];
    }
    let a2 = ax * ax + ay * ay;
    let b2 = bx * bx + by * by;
    let c2 = cx * cx + cy * cy;
    let ux = (a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d;
    let uy = (a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d;
    let r = ((ax - ux) * (ax - ux) + (ay - uy) * (ay - uy)).sqrt();
    let a0 = (ay - uy).atan2(ax - ux);
    let am = (by - uy).atan2(bx - ux);
    let a1 = (cy - uy).atan2(cx - ux);
    let tau = std::f64::consts::TAU;
    let norm = |a: f64| ((a % tau) + tau) % tau;
    // sweep counter-clockwise from a0 to a1 unless the mid point lies the other way round
    let ccw_total = norm(a1 - a0);
    let ccw_mid = norm(am - a0);
    let sweep = if ccw_mid <= ccw_total { ccw_total } else { ccw_total - tau };
    let step = if r > err { 2.0 * (1.0 - err / r).acos() } else { std::f64::consts::FRAC_PI_2 };
    // (an arc so flat that its chords would run into the millions is two points for every use here)
    let n = if step > 0.0 { ((sweep.abs() / step).ceil() as usize).clamp(2, 100_000) } else { 2 };
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let a = a0 + sweep * (i as f64) / (n as f64);
        out.push((ux + r * a.cos(), uy + r * a.sin()));
    }
    out[0] = s;
    out[n] = e;
    out
}

#[cfg(test)]
mod tests {
    use super::Shape;

    #[test]
    fn a_segment_is_measured_against_a_shape_exactly() {
        let sq = Shape::Poly { pts: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], r: 0.0 };
        let near = |a: f64, b: f64| (a - b).abs() < 1e-12;
        // beside an edge, past a corner, across the shape, wholly inside it
        assert!(near(sq.dist_seg((1.2, -5.0), (1.2, 5.0)), 0.2));
        assert!(near(sq.dist_seg((2.0, 1.0), (1.0, 2.0)), 0.5f64.sqrt()));
        assert_eq!(sq.dist_seg((-1.0, 0.5), (2.0, 0.5)), 0.0);
        assert_eq!(sq.dist_seg((0.4, 0.4), (0.6, 0.6)), 0.0);
        // a grown polygon, a circle, a capsule
        let grown = Shape::Poly { pts: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], r: 0.1 };
        assert!(near(grown.dist_seg((1.2, -5.0), (1.2, 5.0)), 0.1));
        assert!(near(Shape::Circle { x: 0.0, y: 0.0, r: 0.3 }.dist_seg((-1.0, 0.5), (1.0, 0.5)), 0.2));
        assert!(near(Shape::Capsule { ax: 0.0, ay: 0.0, bx: 2.0, by: 0.0, r: 0.1 }.dist_seg((1.0, 0.4), (3.0, 0.4)), 0.3));
        // where a point sampled every 0.02 mm would miss it: a thin spike between two samples
        let spike = Shape::Poly { pts: vec![(0.505, 0.3), (0.506, 0.3), (0.5055, 0.001)], r: 0.0 };
        assert!(near(spike.dist_seg((0.0, 0.0), (1.0, 0.0)), 0.001));
    }
}
