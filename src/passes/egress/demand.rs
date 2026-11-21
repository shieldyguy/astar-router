//! Demand: every pad that has to be joined, what kind of way out it wants and where it is going.

use super::geo::{compass, COMPASS};
use crate::geom::{hypot, Shape};
use crate::route::Router;
use crate::scene::Pad;
use std::collections::HashMap;

/// A pad whose net's nearest other pad is nearer than this, mm, makes a hop, not a journey.
pub const HOP_MM: f64 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Want {
    /// goes somewhere else on the board
    Traveller,
    /// goes to a part beside it
    Hop,
    /// goes down into its net's plane
    Drop,
    /// one of a differential pair
    Pair,
}

impl Want {
    pub fn name(self) -> &'static str {
        match self {
            Want::Traveller => "traveller",
            Want::Hop => "hop",
            Want::Drop => "drop",
            Want::Pair => "pair",
        }
    }
}

/// A part: the pads that share a reference.
pub struct Part {
    pub ref_: String,
    pub pads: Vec<usize>,
    /// the footprint's centre
    pub centre: (f64, f64),
    /// the box of its pads' copper
    pub bbox: (f64, f64, f64, f64),
    pub back: bool,
}

/// A pad that has to be joined.
pub struct Demand {
    pub pad: usize,
    pub part: usize,
    /// which side of its part it is on: a compass heading (`geo::COMPASS`), 8 for the middle
    pub side: u8,
    pub want: Want,
    /// where it is going: the nearest pad of its net on another part (its own part when there is no
    /// other); for a drop, itself
    pub to: (f64, f64),
    /// the heading of that, degrees clockwise from east on the screen, and how far, mm
    pub bearing: f64,
    pub dist: f64,
    /// the copper layers its track may start on
    pub layers: u32,
}

impl Demand {
    pub fn toward(&self) -> &'static str {
        if self.dist < 1e-9 {
            "-"
        } else {
            let a = self.bearing.to_radians();
            COMPASS[compass(a.cos(), a.sin()) as usize]
        }
    }
}

/// The way a pad faces: along its long axis away from its part's centre when it is long, else from the
/// part's centre to the pad. None for a pad in the middle of its part.
pub fn outward(pad: &Pad) -> Option<(f64, f64)> {
    let long = match &pad.shape {
        Shape::Poly { pts, .. } if pts.len() == 4 => {
            let e0 = (pts[1].0 - pts[0].0, pts[1].1 - pts[0].1);
            let e1 = (pts[2].0 - pts[1].0, pts[2].1 - pts[1].1);
            let (l0, l1) = (hypot(e0.0, e0.1), hypot(e1.0, e1.1));
            if l0 >= 1.3 * l1 && l0 > 1e-9 {
                Some((e0.0 / l0, e0.1 / l0))
            } else if l1 >= 1.3 * l0 && l1 > 1e-9 {
                Some((e1.0 / l1, e1.1 / l1))
            } else {
                None
            }
        }
        Shape::Capsule { ax, ay, bx, by, r } => {
            let l = hypot(bx - ax, by - ay);
            if l >= 0.6 * r && l > 1e-6 {
                Some(((bx - ax) / l, (by - ay) / l))
            } else {
                None
            }
        }
        _ => None,
    };
    let (dx, dy) = (pad.x - pad.fx, pad.y - pad.fy);
    match long {
        Some((ux, uy)) => {
            let away = ux * dx + uy * dy;
            let off = hypot(dx, dy);
            if off < 1e-6 {
                None
            } else if away.abs() < 0.3 * off {
                // a long pad that lies across the way out of its part (a chip's two tall pads side by side)
                Some((dx / off, dy / off))
            } else if away > 0.0 {
                Some((ux, uy))
            } else {
                Some((-ux, -uy))
            }
        }
        None => {
            let l = hypot(dx, dy);
            if l < 1e-6 {
                None
            } else {
                Some((dx / l, dy / l))
            }
        }
    }
}

/// The parts of a board and the pads that have to be joined.
pub fn demand(r: &Router) -> (Vec<Part>, Vec<Demand>) {
    let sc = &r.sc;
    let mut parts: Vec<Part> = Vec::new();
    let mut by_ref: HashMap<&str, usize> = HashMap::new();
    let mut part_of = vec![0usize; sc.pads.len()];
    for (pi, pad) in sc.pads.iter().enumerate() {
        let k = *by_ref.entry(pad.ref_.as_str()).or_insert_with(|| {
            parts.push(Part { ref_: pad.ref_.clone(), pads: Vec::new(), centre: (pad.fx, pad.fy), bbox: (f64::MAX, f64::MAX, f64::MIN, f64::MIN), back: pad.back });
            parts.len() - 1
        });
        part_of[pi] = k;
        let b = pad.shape.bbox();
        let p = &mut parts[k];
        p.pads.push(pi);
        p.bbox = (p.bbox.0.min(b.0), p.bbox.1.min(b.1), p.bbox.2.max(b.2), p.bbox.3.max(b.3));
    }
    let pairs: Vec<u16> = sc.pairs.iter().flat_map(|p| [p.p, p.n]).collect();
    let all: u32 = r.routable.iter().fold(0, |m, &l| m | (1 << l));
    let mut out = Vec::new();
    for (pi, pad) in sc.pads.iter().enumerate() {
        let net = &sc.nets[pad.net as usize];
        if pad.net == 0 || net.pads.len() < 2 || !net.in_scope {
            continue;
        }
        if net.plane.is_some() && r.pad_in_plane(pi) {
            continue;
        }
        let part = &parts[part_of[pi]];
        // the side: a long pad faces along its axis; any other is on the side of its part's box it is nearest
        let side = match outward(pad) {
            None => 8,
            Some((ux, uy)) => {
                let long = !matches!(pad.shape, Shape::Circle { .. }) && outward_is_axis(pad);
                if long {
                    compass(ux, uy)
                } else {
                    let (hw, hh) = (((part.bbox.2 - part.bbox.0) / 2.0).max(1e-6), ((part.bbox.3 - part.bbox.1) / 2.0).max(1e-6));
                    let (cx, cy) = ((part.bbox.0 + part.bbox.2) / 2.0, (part.bbox.1 + part.bbox.3) / 2.0);
                    let (nx, ny) = ((pad.x - cx) / hw, (pad.y - cy) / hh);
                    if nx.abs() < 0.1 && ny.abs() < 0.1 {
                        8
                    } else if nx.abs() >= ny.abs() {
                        if nx > 0.0 { 0 } else { 4 }
                    } else if ny > 0.0 {
                        2
                    } else {
                        6
                    }
                }
            }
        };
        let layers = if pad.tht { all } else if pad.back { 1 << (sc.layers.len() - 1) } else { 1 };
        let (want, to) = if net.plane.is_some() {
            (Want::Drop, (pad.x, pad.y))
        } else {
            // the nearest pad of the net on another part; on its own part when the net has no other
            let mut best: Option<(bool, f64, usize)> = None;
            for &q in &net.pads {
                if q == pi {
                    continue;
                }
                let key = (part_of[q] == part_of[pi], hypot(sc.pads[q].x - pad.x, sc.pads[q].y - pad.y), q);
                if best.map_or(true, |b| (key.0, key.1) < (b.0, b.1)) {
                    best = Some(key);
                }
            }
            let (_, d, q) = best.expect("a net to join has two pads");
            let want = if pairs.contains(&pad.net) { Want::Pair } else if d <= HOP_MM { Want::Hop } else { Want::Traveller };
            (want, (sc.pads[q].x, sc.pads[q].y))
        };
        let (dx, dy) = (to.0 - pad.x, to.1 - pad.y);
        out.push(Demand { pad: pi, part: part_of[pi], side, want, to, bearing: dy.atan2(dx).to_degrees(), dist: hypot(dx, dy), layers });
    }
    (parts, out)
}

/// Is the pad long enough that its own axis says which way it faces?
fn outward_is_axis(pad: &Pad) -> bool {
    match &pad.shape {
        Shape::Poly { pts, .. } if pts.len() == 4 => {
            let l0 = hypot(pts[1].0 - pts[0].0, pts[1].1 - pts[0].1);
            let l1 = hypot(pts[2].0 - pts[1].0, pts[2].1 - pts[1].1);
            l0 >= 1.3 * l1 || l1 >= 1.3 * l0
        }
        Shape::Capsule { ax, ay, bx, by, r } => hypot(bx - ax, by - ay) >= 0.6 * r,
        _ => false,
    }
}
