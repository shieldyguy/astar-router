//! Exits: a pad is left by its front or its back.

use super::{Pass, Step};
use crate::geom::{hypot, Shape};
use crate::route::Router;
use crate::search::{DX, DY};
use std::collections::HashMap;

pub const PASS: Pass = Pass {
    name: "exits",
    about: "a long pad is left along its length, down its middle, toward the inside of its part or away from it; a track that crosses the pad's side pays",
    step: Step::Plan(Router::build_exits),
};

impl Router {
    /// Exits: a track leaves a long pad (and arrives at one) along the pad's length, down its middle:
    /// toward the inside of its part or away from it. Any other crossing of the pad's edge costs
    /// `Cfg::exit_mm` (the search: `Costs::exits`). A pad has a front and a back when it is at least 1.3
    /// times as long as it is wide and its length points out of its part, read off where the pad lies
    /// among the part's pads: a pin on a chip's side and a pad in a connector's row have; a square pad,
    /// a tab that lies across its part, and the two pads of a wide capacitor (longer across the part than
    /// along it) have not. A pad with a fence has its way out already; a through-hole pad has no front.
    /// A search that aims at such a pad aims at the whole of its middle line (`exit_box`), so that the
    /// way in by an end is weighed fairly against the way in by the side.
    pub(crate) fn build_exits(&mut self) {
        let pitch = self.f.pitch;
        let mut of: HashMap<&str, Vec<usize>> = HashMap::new();
        for (pi, p) in self.sc.pads.iter().enumerate() {
            of.entry(p.ref_.as_str()).or_default().push(pi);
        }
        let mut exits: HashMap<u32, u8> = HashMap::new();
        let mut boxes: HashMap<usize, (i32, i32, i32, i32)> = HashMap::new();
        for (pi, pad) in self.sc.pads.iter().enumerate() {
            if pad.net == 0 || pad.tht || self.sc.nets[pad.net as usize].pads.len() < 2 || self.acc[pi].iter().any(|a| a.via.is_some()) {
                continue;
            }
            // the pad's long axis, and half its length and half its width
            let (ux, uy, half_len, half_short) = match &pad.shape {
                Shape::Poly { pts, r } if pts.len() == 4 => {
                    let e0 = (pts[1].0 - pts[0].0, pts[1].1 - pts[0].1);
                    let e1 = (pts[2].0 - pts[1].0, pts[2].1 - pts[1].1);
                    let (l0, l1) = (hypot(e0.0, e0.1), hypot(e1.0, e1.1));
                    if l0 < 1e-9 || l1 < 1e-9 {
                        continue;
                    }
                    if l0 >= l1 {
                        (e0.0 / l0, e0.1 / l0, l0 / 2.0 + r, l1 / 2.0 + r)
                    } else {
                        (e1.0 / l1, e1.1 / l1, l1 / 2.0 + r, l0 / 2.0 + r)
                    }
                }
                Shape::Capsule { ax, ay, bx, by, r } => {
                    let l = hypot(bx - ax, by - ay);
                    if l < 1e-6 {
                        continue;
                    }
                    ((bx - ax) / l, (by - ay) / l, l / 2.0 + r, *r)
                }
                _ => continue,
            };
            // how far out the pad lies among its part's pads, along each of its axes: 0 in the middle,
            // 1 on the outermost row (a single row is its own outermost)
            let (mut lo, mut hi) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
            let mates = &of[pad.ref_.as_str()];
            for &qi in mates {
                let q = &self.sc.pads[qi];
                let (dx, dy) = (q.x - pad.x, q.y - pad.y);
                let (a, b) = (dx * ux + dy * uy, -dx * uy + dy * ux);
                lo = (lo.0.min(a), lo.1.min(b));
                hi = (hi.0.max(a), hi.1.max(b));
            }
            let out = |lo: f64, hi: f64| if hi - lo > 1e-6 { ((lo + hi) / 2.0).abs() / ((hi - lo) / 2.0) } else { 1.0 };
            let (along, across) = (out(lo.0, hi.0), out(lo.1, hi.1));
            // the two pads of a part that face each other along their long sides have the part between them
            let facing = mates.len() == 2 && (hi.1 - lo.1) > (hi.0 - lo.0);
            if half_len < 1.3 * half_short || across > along + 0.1 || facing {
                continue;
            }
            let (ax, ay, half_across) = (ux, uy, half_short);
            // the axis as one of the eight headings; a pad turned to none of them has no exit to prefer
            let Some(h) = (0..8).find(|&h| (DX[h] as f64 * ax + DY[h] as f64 * ay) / hypot(DX[h] as f64, DY[h] as f64) > 0.985) else { continue };
            let mask = (1u8 << h) | (1u8 << ((h + 4) & 7));
            // down the middle: the grid line nearest the pad's centre line, and on a broad pad its middle half
            let band = if half_across >= 0.6 { half_across * 0.5 } else { pitch * 0.5 + 1e-6 };
            let l = if pad.back { self.f.nl - 1 } else { 0 };
            let b = pad.shape.bbox();
            let ((x0, y0), (x1, y1)) = (self.f.node_near(b.0, b.1), self.f.node_near(b.2, b.3));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let ci = self.f.idx(l, y, x);
                    let c = &self.f.cells[ci];
                    let (px, py) = (self.f.wx(x), self.f.wy(y));
                    if c.sn1 != pad.net || c.sd1 != 0 || pad.shape.dist(px, py) > 0.011 {
                        continue;
                    }
                    let off = (-(px - pad.x) * ay + (py - pad.y) * ax).abs();
                    *exits.entry(ci as u32).or_insert(0) |= if off <= band { mask } else { 0 };
                    if off <= band {
                        let (x, y) = (x as i32, y as i32);
                        let b = boxes.entry(pi).or_insert((x, y, x, y));
                        *b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
                    }
                }
            }
        }
        self.costs.exits = exits;
        self.exit_box = boxes;
    }
}
