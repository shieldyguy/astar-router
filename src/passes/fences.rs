//! Fences and fan lanes: the way out of a fine-pitch pad.

use crate::field::{sdist, Kind, TICK};
use crate::geom::{hypot, Shape};
use super::{Pass, Step};
use crate::route::{Access, Router, StaticObj};
use crate::search::{Price, DX, DY};
use std::collections::HashSet;

pub const PASS: Pass = Pass {
    name: "fences",
    about: "a pad too narrow for the grid gets an exact stub along its centre line; the lane in front of it costs other nets a toll, and no foreign via may stand in its mouth",
    step: Step::Plan(Router::build_fences),
};

impl Router {
    /// Fences: a narrow pad (a fine-pitch pin) cannot be left on the grid, because a track a few hundredths
    /// off its centre line is too near the next pin. So each one gets a short track laid exactly on its
    /// centre line, out of the pad's end and onto a grid node: one away from the part, as long as there is
    /// room for, and a short one towards it. They are part of the fixed copper of their net from the
    /// start, so every other net keeps its distance and no via lands in a pin's way out; the pad's track
    /// starts at a fence's far end, and only the fence it uses is drawn.
    pub(crate) fn build_fences(&mut self) {
        let pitch = self.cfg.pitch;
        let np = self.sc.pads.len();
        // (pad, where it leaves the pad, its end node, its heading, half its width)
        let mut made: Vec<(usize, (f64, f64), (usize, usize), u8, f64)> = Vec::new();
        // outward fences: (net, layer, end point, direction)
        let mut lanes: Vec<(u16, usize, (f64, f64), (f64, f64), f64)> = Vec::new();
        for outward in [true, false] {
            for pi in 0..np {
                let pad = &self.sc.pads[pi];
                if pad.net == 0 || pad.tht || self.sc.nets[pad.net as usize].pads.len() < 2 {
                    continue;
                }
                // the pad's long axis and half sizes
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
                let net = pad.net;
                let clr = self.net_clr(net);
                // the net's own width where that stands clear, else the narrowest it may be drawn
                let (w_nom, w_min) = (self.sc.nets[net as usize].width, self.w_min(net));
                let widths: &[f64] = if w_nom > w_min + 1e-9 { &[w_nom, w_min] } else { &[w_min] };
                // narrow: a track on the nearest grid line could stand too near what hugs this pad
                let t_mm = self.t_min(net) as f64 * TICK;
                if half_short >= t_mm + pitch * 0.71 - clr || half_len < 1.3 * half_short {
                    continue;
                }
                let lmask = if pad.back { 1u32 << (self.f.nl - 1) } else { 1 };
                let c = (pad.x, pad.y);
                let away = ux * (pad.x - pad.fx) + uy * (pad.y - pad.fy);
                let sgn = if (away >= 0.0) == outward { 1.0 } else { -1.0 };
                let lens: &[f64] = if outward { &[1.0, 0.6, 0.25] } else { &[0.25] };
                for (&w, &k) in widths.iter().flat_map(|w| lens.iter().map(move |k| (w, k))) {
                    let half = w / 2.0;
                    let ext = (self.cfg.fence_mm * k).max(1.5 * pitch);
                    // the fence keeps to the centre line until it is clear of the pad's end, then steps
                    // over to the grid: a step begun at the pad's very edge leaves a sliver beside it
                    let straight = half_len + (ext - 0.1).clamp(0.0, 0.1);
                    let e0 = (c.0 + sgn * ux * straight, c.1 + sgn * uy * straight);
                    let q = (c.0 + sgn * ux * (half_len + ext), c.1 + sgn * uy * (half_len + ext));
                    let (gx, gy) = self.f.node_near(q.0, q.1);
                    let g = (self.f.wx(gx), self.f.wy(gy));
                    if !self.statics.track_clear(c, e0, half, clr, lmask, net) || !self.statics.track_clear(e0, g, half, clr, lmask, net) {
                        continue;
                    }
                    for (a, b) in [(c, e0), (e0, g)] {
                        self.statics.add(StaticObj::fixed(Kind::Fence, Shape::Capsule { ax: a.0, ay: a.1, bx: b.0, by: b.1, r: half }, lmask, net, 0.0));
                    }
                    // the fence's heading, as the nearest of the eight compass steps
                    let head = (0..8).max_by(|&a, &b| {
                        let da = (DX[a] as f64 * sgn * ux + DY[a] as f64 * sgn * uy) / hypot(DX[a] as f64, DY[a] as f64);
                        let db = (DX[b] as f64 * sgn * ux + DY[b] as f64 * sgn * uy) / hypot(DX[b] as f64, DY[b] as f64);
                        da.partial_cmp(&db).unwrap()
                    });
                    made.push((pi, e0, (gx, gy), head.unwrap_or(0) as u8, half));
                    if outward {
                        lanes.push((net, if pad.back { self.f.nl - 1 } else { 0 }, g, (sgn * ux, sgn * uy), half));
                    }
                    break;
                }
            }
        }
        let mut fenced = vec![false; np];
        for &(pi, e0, (gx, gy), _, half) in &made {
            let pad = &self.sc.pads[pi];
            let lmask = if pad.back { 1u32 << (self.f.nl - 1) } else { 1 };
            let g = (self.f.wx(gx), self.f.wy(gy));
            for (a, b) in [((pad.x, pad.y), e0), (e0, g)] {
                self.f.stamp_static(&Shape::Capsule { ax: a.0, ay: a.1, bx: b.0, by: b.1, r: half }, lmask, pad.net, 0.0);
            }
            if !fenced[pi] {
                fenced[pi] = true;
                self.acc[pi].clear();
            }
            let wide = half > self.w_min(self.sc.pads[pi].net) / 2.0 + 1e-9;
            self.acc[pi].push(Access { x: gx as u16, y: gy as u16, via: Some(e0), wide });
        }
        // a fence's end node was checked exactly, so the grid's coarser reading must not close it
        for &(pi, _, (gx, gy), head, half) in &made {
            let pad = &self.sc.pads[pi];
            let l = if pad.back { self.f.nl - 1 } else { 0 };
            let t = if half > self.w_min(pad.net) / 2.0 + 1e-9 { self.t_nom(pad.net) } else { self.t_min(pad.net) };
            let i = self.f.idx(l, gy, gx);
            let c = &mut self.f.cells[i];
            if c.sn1 == pad.net && c.sd2 < t {
                c.sd2 = t;
            }
            if c.sn1 == pad.net {
                c.fdir = head;
            }
        }
        self.stats.fences = made.len();
        // fan lanes: the way out in front of each outward fence. Another net pays to run through it (the
        // cost term "lane"), and no via of another net may stand in its mouth, where it would shut the
        // pin in; where two mouths overlap, a via of either net costs more (the cost term "mouth").
        let p = self.f.pitch;
        let lane = self.costs.term("lane");
        let mouth_term = self.costs.term("mouth");
        self.costs.terms[mouth_term.0 as usize].via_shared = (self.cfg.mouth_mm * 1000.0) as u32;
        let toll = Price::per_width((self.cfg.lane_toll * 1000.0 * p) as u32);
        // a node in two lanes is the first one's
        let mut in_lane: HashSet<usize> = HashSet::new();
        for &(net, l, g, u, half) in &lanes {
            let reach = self.cfg.lane_mm;
            let end = (g.0 + u.0 * reach, g.1 + u.1 * reach);
            let (x0, y0) = self.f.node_near(g.0.min(end.0) - 1.0, g.1.min(end.1) - 1.0);
            let (x1, y1) = self.f.node_near(g.0.max(end.0) + 1.0, g.1.max(end.1) + 1.0);
            let mouth = self.sc.rules.via / 2.0 + self.net_clr(net) + half + 0.01;
            let mouth_end = (g.0 + u.0 * 0.5, g.1 + u.1 * 0.5);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (self.f.wx(x), self.f.wy(y));
                    if crate::geom::seg_dist2(px, py, g.0, g.1, end.0, end.1).sqrt() <= p * 1.05 {
                        let i = self.f.idx(l, y, x);
                        if in_lane.insert(i) {
                            self.costs.charge(&mut self.f, lane, i, net, toll);
                        }
                    }
                    if crate::geom::seg_dist2(px, py, g.0, g.1, mouth_end.0, mouth_end.1).sqrt() < mouth {
                        self.costs.reserve_via(mouth_term, y * self.f.w + x, net);
                    }
                }
            }
        }
        if self.cfg.verbose {
            // a pad none of whose access nodes has a free neighbour cannot be left at all
            for pi in 0..np {
                let pad = &self.sc.pads[pi];
                if self.acc[pi].is_empty() || pad.tht {
                    continue;
                }
                let l = if pad.back { self.f.nl - 1 } else { 0 };
                let t = self.t_min(pad.net);
                let mut free = 0;
                for a in &self.acc[pi] {
                    for d in 0..8 {
                        let (nx, ny) = (a.x as i32 + crate::search::DX[d], a.y as i32 + crate::search::DY[d]);
                        if nx < 0 || ny < 0 || nx >= self.f.w as i32 || ny >= self.f.h as i32 {
                            continue;
                        }
                        let c = &self.f.cells[self.f.idx(l, ny as usize, nx as usize)];
                        if sdist(c, pad.net) >= t {
                            free += 1;
                        }
                    }
                }
                if free == 0 {
                    eprintln!("  shut in by fixed copper: {}.{} [{}] ({} access nodes, fence: {})", pad.ref_, pad.num, self.sc.nets[pad.net as usize].name, self.acc[pi].len(), fenced[pi]);
                    if fenced[pi] && std::env::var("ASTAR_DEBUG").is_ok() {
                        for a in &self.acc[pi] {
                            eprint!("    node ({:.2}, {:.2}) t={}:", self.f.wx(a.x as usize), self.f.wy(a.y as usize), t);
                            for d in 0..8 {
                                let (nx, ny) = (a.x as i32 + crate::search::DX[d], a.y as i32 + crate::search::DY[d]);
                                let c = &self.f.cells[self.f.idx(l, ny as usize, nx as usize)];
                                eprint!(" [{},{}: sd1={} sn1={} sd2={}]", crate::search::DX[d], crate::search::DY[d], c.sd1, c.sn1, c.sd2);
                            }
                            eprintln!();
                        }
                    }
                }
            }
        }
    }
}
