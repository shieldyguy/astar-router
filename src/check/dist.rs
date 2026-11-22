//! Exact distances between two pieces of copper, for the check. Built on `geom::seg_dist2` alone.

use crate::geom::{seg_dist2, Shape};

pub type Pt = (f64, f64);

/// A piece of copper: a segment grown by `r` (a track, an oval or round pad, a via, a hole), or an
/// area: every point inside `outer` and outside each of `holes`, grown by `r`.
#[derive(Clone, Debug)]
pub enum Solid {
    Cap { a: Pt, b: Pt, r: f64 },
    Area { outer: Vec<Pt>, holes: Vec<Vec<Pt>>, r: f64 },
}

/// Far inside: what `gap` gives when one piece lies within the other's area.
pub const WITHIN: f64 = -1.0e9;

impl Solid {
    pub fn circle(x: f64, y: f64, r: f64) -> Solid {
        Solid::Cap { a: (x, y), b: (x, y), r }
    }

    pub fn of_shape(s: &Shape) -> Solid {
        match s {
            Shape::Circle { x, y, r } => Solid::circle(*x, *y, *r),
            Shape::Capsule { ax, ay, bx, by, r } => Solid::Cap { a: (*ax, *ay), b: (*bx, *by), r: *r },
            Shape::Poly { pts, r } => Solid::Area { outer: pts.clone(), holes: Vec::new(), r: *r },
        }
    }

    pub fn bbox(&self) -> (f64, f64, f64, f64) {
        match self {
            Solid::Cap { a, b, r } => (a.0.min(b.0) - r, a.1.min(b.1) - r, a.0.max(b.0) + r, a.1.max(b.1) + r),
            Solid::Area { outer, r, .. } => {
                let mut bb = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for &(x, y) in outer {
                    bb = (bb.0.min(x), bb.1.min(y), bb.2.max(x), bb.3.max(y));
                }
                (bb.0 - r, bb.1 - r, bb.2 + r, bb.3 + r)
            }
        }
    }

    /// Is the point inside the copper (the area itself, not its growth; a segment has no inside).
    pub fn holds(&self, p: Pt) -> bool {
        match self {
            Solid::Cap { .. } => false,
            Solid::Area { outer, holes, .. } => in_loop(outer, p) && !holes.iter().any(|h| in_loop(h, p)),
        }
    }

    /// A point of the piece.
    fn any_point(&self) -> Pt {
        match self {
            Solid::Cap { a, .. } => *a,
            Solid::Area { outer, .. } => outer[0],
        }
    }

    fn r(&self) -> f64 {
        match self {
            Solid::Cap { r, .. } | Solid::Area { r, .. } => *r,
        }
    }

    /// Calls `f` with every segment of the piece's spine: the segment itself, or every edge of the area.
    fn spine(&self, f: &mut dyn FnMut(Pt, Pt)) {
        match self {
            Solid::Cap { a, b, .. } => f(*a, *b),
            Solid::Area { outer, holes, .. } => {
                for l in std::iter::once(outer).chain(holes.iter()) {
                    let n = l.len();
                    for i in 0..n {
                        f(l[i], l[(i + 1) % n]);
                    }
                }
            }
        }
    }
}

/// Even-odd: is the point inside the loop.
pub fn in_loop(l: &[Pt], p: Pt) -> bool {
    let n = l.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (xi, yi) = l[i];
        let (xj, yj) = l[j];
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn cross(o: Pt, a: Pt, b: Pt) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

fn nearest_on(p: Pt, a: Pt, b: Pt) -> Pt {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (a.0 + t * dx, a.1 + t * dy)
}

/// The distance between segments a-b and c-d, and the point of a-b nearest to c-d.
pub fn seg_seg(a: Pt, b: Pt, c: Pt, d: Pt) -> (f64, Pt) {
    // they cross: the distance is 0, at the crossing
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        let t = d1 / (d1 - d2);
        return (0.0, (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
    }
    // else the nearest pair has an end of one of them in it
    let mut best = (seg_dist2(a.0, a.1, c.0, c.1, d.0, d.1), a);
    let e = seg_dist2(b.0, b.1, c.0, c.1, d.0, d.1);
    if e < best.0 {
        best = (e, b);
    }
    for q in [c, d] {
        let e = seg_dist2(q.0, q.1, a.0, a.1, b.0, b.1);
        if e < best.0 {
            best = (e, nearest_on(q, a, b));
        }
    }
    (best.0.sqrt(), best.1)
}

/// The air between two pieces of copper, and the point of `a`'s spine nearest to `b`: above 0 they are
/// apart by that much, at or below 0 they touch or overlap (by that much, or `WITHIN` when one lies
/// inside the other's area).
pub fn gap(a: &Solid, b: &Solid) -> (f64, Pt) {
    let mut best = (f64::MAX, a.any_point());
    a.spine(&mut |p, q| {
        b.spine(&mut |s, t| {
            let (d, at) = seg_seg(p, q, s, t);
            if d < best.0 {
                best = (d, at);
            }
        });
    });
    let g = best.0 - a.r() - b.r();
    // one lies inside the other's area: whole (no edge of one is near the other), or with its rim just
    // at the other's edge (a hole's round pad in the corner of a thermal pad, a radius from two sides)
    if b.holds(a.any_point()) || a.holds(b.any_point()) {
        return (WITHIN, a.any_point());
    }
    if g > 0.0 {
        return (g, best.1);
    }
    // two areas whose edges meet overlap, however thin their rims
    if best.0 == 0.0 && matches!(a, Solid::Area { .. }) && matches!(b, Solid::Area { .. }) {
        return (WITHIN, best.1);
    }
    (g, best.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(ax: f64, ay: f64, bx: f64, by: f64, r: f64) -> Solid {
        Solid::Cap { a: (ax, ay), b: (bx, by), r }
    }

    #[test]
    fn segments() {
        assert_eq!(seg_seg((0.0, 0.0), (2.0, 0.0), (1.0, -1.0), (1.0, 1.0)).0, 0.0);
        assert!((seg_seg((0.0, 0.0), (2.0, 0.0), (3.0, 4.0), (3.0, 9.0)).0 - 17f64.sqrt()).abs() < 1e-12);
        assert!((seg_seg((0.0, 0.0), (2.0, 0.0), (0.0, 1.0), (2.0, 1.0)).0 - 1.0).abs() < 1e-12);
        // an end on the other segment
        assert_eq!(seg_seg((0.0, 0.0), (2.0, 0.0), (1.0, 0.0), (1.0, 5.0)).0, 0.0);
    }

    #[test]
    fn tracks_and_pads() {
        // two tracks 0.2 wide whose centre lines are 0.35 apart: 0.15 of air
        let (g, _) = gap(&cap(0.0, 0.0, 5.0, 0.0, 0.1), &cap(0.0, 0.35, 5.0, 0.35, 0.1));
        assert!((g - 0.15).abs() < 1e-12);
        // a track ending in the middle of a square pad lies within it; one beside it does not
        let pad = Solid::Area { outer: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], holes: vec![], r: 0.0 };
        assert!(gap(&cap(0.5, 0.5, 0.5, 0.6, 0.05), &pad).0 <= WITHIN);
        assert!(gap(&cap(0.5, 0.5, 3.0, 0.5, 0.05), &pad).0 <= 0.0);
        assert!((gap(&cap(1.3, -1.0, 1.3, 2.0, 0.1), &pad).0 - 0.2).abs() < 1e-12);
        // a rounded pad (a polygon grown by r)
        let round = Solid::Area { outer: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], holes: vec![], r: 0.1 };
        assert!((gap(&cap(1.3, -1.0, 1.3, 2.0, 0.1), &round).0 - 0.1).abs() < 1e-12);
        // a via in the hole of an island is outside its copper
        let island = Solid::Area { outer: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], holes: vec![vec![(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0)]], r: 0.0 };
        assert!((gap(&Solid::circle(5.0, 5.0, 0.3), &island).0 - 0.7).abs() < 1e-12);
        assert!(gap(&Solid::circle(2.0, 2.0, 0.3), &island).0 <= WITHIN);
        // a round pad inside a square one, its rim at two of the square's sides: it lies within it
        assert!(gap(&Solid::circle(0.25, 0.25, 0.25), &pad).0 <= WITHIN);
        assert!(gap(&pad, &Solid::circle(0.25, 0.25, 0.25)).0 <= WITHIN);
    }
}
