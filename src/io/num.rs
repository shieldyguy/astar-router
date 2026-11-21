//! Numbers as KiCad keeps them: whole nanometres, turned the way KiCad turns them, so that what the
//! reader gives is what KiCad itself would say of the board.

/// Nanometres in a millimetre.
pub const NM: f64 = 1e6;

/// Millimetres (as a board file writes them) to whole nanometres.
/// A board is not a kilometre across: a length beyond that is held there, so that sums of lengths
/// cannot overflow. (The scene's validator then refuses it, with the place.)
pub fn nm(mm: f64) -> i64 {
    (mm * NM).round().clamp(-1e15, 1e15) as i64
}

/// Whole nanometres to millimetres.
pub fn mm(nm: i64) -> f64 {
    nm as f64 / NM
}

/// A point in nanometres.
pub type P = (i64, i64);

/// KiCad's turn of a point about the origin by `deg` (counter-clockwise on the screen, y down):
/// exact for the four right angles, rounded to the nanometre otherwise.
pub fn rot(p: P, deg: f64) -> P {
    let (x, y) = rotf((p.0 as f64, p.1 as f64), deg);
    (x.round() as i64, y.round() as i64)
}

/// The same turn of a point that is not on the nanometre grid.
pub fn rotf(p: (f64, f64), deg: f64) -> (f64, f64) {
    let a = deg.rem_euclid(360.0);
    if a == 0.0 {
        p
    } else if a == 90.0 {
        (p.1, -p.0)
    } else if a == 180.0 {
        (-p.0, -p.1)
    } else if a == 270.0 {
        (-p.1, p.0)
    } else {
        let (s, c) = a.to_radians().sin_cos();
        (p.1 * s + p.0 * c, p.1 * c - p.0 * s)
    }
}

/// Where a thing drawn in a footprint is on the board.
#[derive(Clone, Copy)]
pub struct Frame {
    pub at: P,
    pub deg: f64,
}

impl Frame {
    pub const BOARD: Frame = Frame { at: (0, 0), deg: 0.0 };

    pub fn put(&self, local: P) -> P {
        let r = rot(local, self.deg);
        (self.at.0 + r.0, self.at.1 + r.1)
    }
}

/// The centre of the circle through three points, None when they lie on a line.
pub fn centre3(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> Option<(f64, f64)> {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d.abs() < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (a.0 * a.0 + a.1 * a.1, b.0 * b.0 + b.1 * b.1, c.0 * c.0 + c.1 * c.1);
    Some(((a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d, (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d))
}

/// Points along the arc from `s` through `m` to `e`, both ends included, no chord farther than `err`
/// from the arc. A flat "arc" is its two ends.
pub fn arc_pts(s: (f64, f64), m: (f64, f64), e: (f64, f64), err: f64) -> Vec<(f64, f64)> {
    let Some(c) = centre3(s, m, e) else { return vec![s, e] };
    let r = ((s.0 - c.0).powi(2) + (s.1 - c.1).powi(2)).sqrt();
    let ang = |p: (f64, f64)| (p.1 - c.1).atan2(p.0 - c.0);
    let (a0, am, a1) = (ang(s), ang(m), ang(e));
    let tau = std::f64::consts::TAU;
    // the sweep that passes through m
    let mut sweep = (a1 - a0).rem_euclid(tau);
    if (am - a0).rem_euclid(tau) > sweep {
        sweep -= tau;
    }
    if s == e {
        sweep = tau;
    }
    let step = if r > err { 2.0 * (1.0 - err / r).acos() } else { tau / 8.0 };
    let n = ((sweep.abs() / step).ceil() as usize).clamp(1, 720);
    let mut out = Vec::with_capacity(n + 1);
    out.push(s);
    for k in 1..n {
        let a = a0 + sweep * k as f64 / n as f64;
        out.push((c.0 + r * a.cos(), c.1 + r * a.sin()));
    }
    out.push(e);
    out
}

/// A cubic Bezier as points, both ends included.
pub fn bezier_pts(p: [(f64, f64); 4], n: usize) -> Vec<(f64, f64)> {
    (0..=n)
        .map(|k| {
            let t = k as f64 / n as f64;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            (w[0] * p[0].0 + w[1] * p[1].0 + w[2] * p[2].0 + w[3] * p[3].0, w[0] * p[0].1 + w[1] * p[1].1 + w[2] * p[2].1 + w[3] * p[3].1)
        })
        .collect()
}

/// A polygon's area, mm2 for points in mm (always 0 or more).
pub fn area(pts: &[(f64, f64)]) -> f64 {
    let mut a = 0.0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        a += p.0 * q.1 - q.0 * p.1;
    }
    a.abs() / 2.0
}

/// Is the point inside the polygon (even-odd)?
pub fn inside(pts: &[(f64, f64)], p: (f64, f64)) -> bool {
    let mut inside = false;
    let mut j = pts.len().wrapping_sub(1);
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn crosses(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let o = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) && d1 != 0.0 && d2 != 0.0 && d3 != 0.0 && d4 != 0.0
}

/// A ring with holes as one ring: each hole is joined to the ring by a seam there and back (as KiCad
/// itself keeps a filled polygon), along the shortest line from a corner of the hole to a corner of
/// the ring that crosses no edge. Inside and outside stay what they were.
pub fn keyhole(outer: &[(f64, f64)], holes: &[Vec<(f64, f64)>]) -> Vec<(f64, f64)> {
    let mut ring = outer.to_vec();
    for (hi, h) in holes.iter().enumerate() {
        if h.len() < 3 {
            continue;
        }
        let mut pairs: Vec<(f64, usize, usize)> = Vec::with_capacity(ring.len() * h.len());
        for (i, a) in ring.iter().enumerate() {
            for (j, b) in h.iter().enumerate() {
                pairs.push(((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2), i, j));
            }
        }
        pairs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let clear = |a: (f64, f64), b: (f64, f64)| {
            let hit = |r: &[(f64, f64)]| (0..r.len()).any(|k| crosses(a, b, r[k], r[(k + 1) % r.len()]));
            !hit(&ring) && !holes[hi..].iter().any(|r| hit(r))
        };
        let &(_, i, j) = pairs.iter().take(4000).find(|p| clear(ring[p.1], h[p.2])).unwrap_or(&pairs[0]);
        let mut next = Vec::with_capacity(ring.len() + h.len() + 2);
        next.extend_from_slice(&ring[..=i]);
        next.extend(h[j..].iter().chain(h[..=j].iter()));
        next.extend_from_slice(&ring[i..]);
        ring = next;
    }
    ring
}

/// The convex hull of the points, counter-clockwise in a y-up reading (Andrew's chain).
pub fn hull(mut pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut h: Vec<(f64, f64)> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = h.len();
        let it: Box<dyn Iterator<Item = &(f64, f64)>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &p in it {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
                h.pop();
            }
            h.push(p);
        }
        h.pop();
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_as_kicad_does() {
        assert_eq!(rot((1000, 0), 90.0), (0, -1000));
        assert_eq!(rot((1000, 0), 180.0), (-1000, 0));
        assert_eq!(rot((1000, 0), 270.0), (0, 1000));
        assert_eq!(rot((1000, 0), -90.0), (0, 1000));
        assert_eq!(rot((1_000_000, 0), 45.0), (707_107, -707_107));
        assert_eq!(nm(30.5499), 30_549_900);
        assert_eq!(mm(30_549_900), 30.5499);
    }

    #[test]
    fn arcs_and_hulls() {
        let pts = arc_pts((1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), 0.001);
        assert!(pts.len() > 8);
        assert!(pts.iter().all(|p| ((p.0 * p.0 + p.1 * p.1).sqrt() - 1.0).abs() < 1e-9 && p.1 >= -1e-9));
        assert_eq!(arc_pts((0.0, 0.0), (1.0, 0.0), (2.0, 0.0), 0.001).len(), 2);
        let h = hull(vec![(0.0, 0.0), (2.0, 0.0), (1.0, 0.5), (2.0, 2.0), (0.0, 2.0)]);
        assert_eq!(h.len(), 4);
        assert!((area(&h) - 4.0).abs() < 1e-12);
        assert!(inside(&h, (1.0, 1.0)) && !inside(&h, (3.0, 1.0)));
        // a square with a square hole: the hole is outside, the area is the difference
        let outer = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let hole = vec![(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0)];
        let k = keyhole(&outer, &[hole]);
        assert_eq!(k.len(), 10);
        assert!(inside(&k, (1.0, 5.0)) && !inside(&k, (5.0, 5.0)) && !inside(&k, (11.0, 5.0)));
    }
}
