//! Exit candidates: each pad's real ways out, found by a short search in the field as it stands before
//! any copper: straight out and the seven other headings, out of a pocket through each gap that leads
//! to the open board, down a via beside the pad, a via in the pad itself.

use super::demand::{Demand, Want};
use super::gaps::Gaps;
use super::geo::unit;
use crate::field::{ddist, sdist};
use crate::geom::hypot;
use crate::route::Router;
use crate::search::{DX, DY, SEVERAL_NETS};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A pad shut in a pocket is searched this far for a gap that leads out, mm.
pub const POCKET_MM: f64 = 8.0;
/// and for this many nodes at most.
pub const POCKET_POPS: u64 = 8000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum How {
    /// on the pad's layer, heading this way (a compass heading, `geo::COMPASS`)
    Dir(u8),
    /// out of a pocket through this gap
    Gap(u32),
    /// a via beside the pad
    Via,
    /// a via in the pad itself (a big pad)
    InPad,
    /// a through-hole pad leaving on another layer
    Layer(u8),
    /// a pad of a plane net joining a pad of its net that is in the plane or takes a via itself
    Join(u32),
}

#[derive(Clone)]
pub struct Cand {
    pub how: How,
    /// the layer its track runs on
    pub layer: u8,
    /// its nodes (x, y) from the pad's access node to its end
    pub path: Vec<(u16, u16)>,
    /// where it goes through the board
    pub via: Option<(u16, u16)>,
    /// the gaps it passes, in order
    pub gaps: Vec<u32>,
    /// what it costs: its length, how far its heading is off where the pad is going, its via
    pub cost: u32,
    pub len: f64,
    /// found not to work when it was tried for real
    pub dead: bool,
}

impl Cand {
    pub fn name(&self, r: &Router, g: &Gaps) -> String {
        match self.how {
            How::Dir(d) => super::geo::COMPASS[d as usize].to_string(),
            How::Gap(k) => format!("past {}|{}", g.name(r, g.gaps[k as usize].a), g.name(r, g.gaps[k as usize].b)),
            How::Via => "via".into(),
            How::InPad => "via in pad".into(),
            How::Layer(l) => format!("on {}", r.sc.layers[l as usize]),
            How::Join(q) => format!("to {}.{}", r.sc.pads[q as usize].ref_, r.sc.pads[q as usize].num),
        }
    }
    /// The path as points, mm.
    pub fn points(&self, r: &Router) -> Vec<(f64, f64)> {
        self.path.iter().map(|&(x, y)| (r.f.wx(x as usize), r.f.wy(y as usize))).collect()
    }
}

/// Where a pad of a plane net may join its net without a via of its own: (net, field index of a node)
/// -> the pad there, one that is in the plane (through-hole) or big enough to take a via itself.
pub type Anchors = std::collections::HashMap<(u16, u32), u32>;

pub fn anchors(r: &Router) -> Anchors {
    let mut out = Anchors::new();
    for (ni, n) in r.sc.nets.iter().enumerate() {
        if n.plane.is_none() || n.pads.len() < 2 {
            continue;
        }
        for &q in &n.pads {
            let pad = &r.sc.pads[q];
            let (bx0, by0, bx1, by1) = pad.shape.bbox();
            if !pad.tht && (bx1 - bx0).min(by1 - by0) < r.cfg.vip_mm {
                continue;
            }
            // any node well inside its copper is a place to join it
            let (x0, y0) = r.f.node_near(bx0, by0);
            let (x1, y1) = r.f.node_near(bx1, by1);
            for &l in &r.pad_layers(q) {
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        if pad.shape.depth(r.f.wx(x), r.f.wy(y)) >= 0.05 {
                            out.entry((ni as u16, r.f.idx(l, y, x) as u32)).or_insert(q as u32);
                        }
                    }
                }
            }
        }
    }
    out
}

/// The search's own ground, kept from pad to pad.
pub struct Work {
    g: Vec<u32>,
    par: Vec<u8>,
    last_gap: Vec<u32>,
    stamp: Vec<u32>,
    now: u32,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
    pub pops: u64,
}

impl Work {
    pub fn new(r: &Router) -> Work {
        let n = r.f.w * r.f.h;
        Work { g: vec![0; n], par: vec![0; n], last_gap: vec![0; n], stamp: vec![0; n], now: 0, heap: BinaryHeap::new(), pops: 0 }
    }
}

/// The pad's reach from its centre in a heading: how far its copper goes that way.
fn support(r: &Router, p: usize, d: u8) -> f64 {
    let pad = &r.sc.pads[p];
    let (bx0, by0, bx1, by1) = pad.shape.bbox();
    let (ux, uy) = unit(d);
    [(bx0, by0), (bx1, by0), (bx1, by1), (bx0, by1)].iter().map(|&(x, y)| (x - pad.x) * ux + (y - pad.y) * uy).fold(0.0, f64::max)
}

/// May a via of this net stand at a node, as far as fixed copper and the cost terms go?
pub fn via_site(r: &Router, net: u16, x: usize, y: usize, dynamic: bool) -> bool {
    let f = &r.f;
    let vi = y * f.w + x;
    let vl = r.costs.via_net[vi];
    if vl != 0 && vl != net && (vl != SEVERAL_NETS || r.costs.via_price(vi, net).is_none()) {
        return false;
    }
    (0..f.nl).all(|l| {
        let c = &f.cells[f.idx(l, y, x)];
        c.sd1 >= r.kind(net).t && (!dynamic || ddist(c, net) >= r.kind(net).t)
    })
}

/// The ways out of one pad. `dynamic`: copper laid in the field is in the way too.
pub fn candidates(r: &Router, gaps: &Gaps, anchors: &Anchors, d: &Demand, ws: &mut Work, dynamic: bool) -> Vec<Cand> {
    let f = &r.f;
    let p = d.pad;
    let pad = &r.sc.pads[p];
    let net = pad.net;
    let fan = r.cfg.lane_mm;
    let l = if pad.tht { r.routable[0] } else if pad.back { f.nl - 1 } else { 0 };
    let mut out: Vec<Cand> = Vec::new();
    if r.acc[p].is_empty() || !r.routable.contains(&l) {
        in_pad(r, d, &mut out);
        return out;
    }
    let t_min = r.t_min(net);
    let (orth, diag) = ((1000.0 * f.pitch).round() as u32, (1414.2 * f.pitch).round() as u32);
    let base = l * f.w * f.h;
    let legal = |i: usize| -> bool {
        let c = &f.cells[base + i];
        sdist(c, net) >= t_min && (!dynamic || ddist(c, net) >= t_min)
    };
    // where each heading's way out ends: the lane's length past the pad's copper
    let drop = d.want == Want::Drop;
    let mut target: [Option<(usize, usize)>; 8] = [None; 8];
    // a heading's way is a way in that heading only when it gets there nearly straight
    let mut most: [u32; 8] = [0; 8];
    let mut reach: f64 = 0.0;
    if !drop {
        for h in 0..8u8 {
            let s = support(r, p, h);
            reach = reach.max(s + fan);
            let (ux, uy) = unit(h);
            // (where something stands at the lane's end, the lane is as long as there is room for)
            for k in [1.0, 0.75, 0.5] {
                let (tx, ty) = (pad.x + ux * (s + fan * k), pad.y + uy * (s + fan * k));
                if tx < f.wx(1) || ty < f.wy(1) || tx > f.wx(f.w - 2) || ty > f.wy(f.h - 2) {
                    continue;
                }
                let (nx, ny) = f.node_near(tx, ty);
                if legal(ny * f.w + nx) {
                    target[h as usize] = Some((nx, ny));
                    most[h as usize] = (((s + fan * k) * 1.1 + 0.1) * 1000.0) as u32;
                    break;
                }
            }
        }
    }
    let near = ((reach.max(fan) * 1.5 + 0.3) * 1000.0) as u32;
    let pops0 = ws.pops;
    let far = (POCKET_MM * 1000.0) as u32;
    let need2 = {
        let n = (r.sc.rules.via + r.sc.rules.clearance) / f.pitch;
        (n * n - 1e-9).ceil() as i64
    };

    ws.now = ws.now.wrapping_add(1);
    if ws.now == 0 {
        ws.stamp.fill(0);
        ws.now = 1;
    }
    ws.heap.clear();
    for a in &r.acc[p] {
        let i = a.y as usize * f.w + a.x as usize;
        let c0 = if a.via.is_some() { 0 } else { (hypot(f.wx(a.x as usize) - pad.x, f.wy(a.y as usize) - pad.y) * 1000.0) as u32 };
        if a.x == 0 || a.y == 0 || a.x as usize >= f.w - 1 || a.y as usize >= f.h - 1 {
            continue;
        }
        if ws.stamp[i] != ws.now || c0 < ws.g[i] {
            ws.stamp[i] = ws.now;
            ws.g[i] = c0;
            ws.par[i] = 255;
            ws.last_gap[i] = 0;
            ws.heap.push(Reverse((c0, i as u32)));
        }
    }
    let own_obj = (gaps.pad0 + p) as u32;
    let trace = |ws: &Work, end: usize| -> (Vec<(u16, u16)>, Vec<u32>) {
        let (mut path, mut through) = (Vec::new(), Vec::new());
        let mut i = end;
        loop {
            path.push(((i % f.w) as u16, (i / f.w) as u16));
            if let Some(&g) = gaps.wall.get(&((base + i) as u32)) {
                // (the line between the pad itself and its neighbour is where its way starts, not a
                // gap it passes; nor is a gap between two pads of its own net)
                let gp = &gaps.gaps[g as usize - 1];
                let own = gp.a == own_obj || gp.b == own_obj || (r.statics.objs[gp.a as usize].net == net && r.statics.objs[gp.b as usize].net == net);
                if !own && through.last() != Some(&(g - 1)) {
                    through.push(g - 1);
                }
            }
            let pd = ws.par[i];
            if pd == 255 || path.len() > 100_000 {
                break;
            }
            i = (i as i64 - DX[pd as usize] as i64 - DY[pd as usize] as i64 * f.w as i64) as usize;
        }
        path.reverse();
        through.reverse();
        (path, through)
    };
    // how far a way out's heading is off where the pad is going, as a price
    let off = |x: usize, y: usize| -> u32 {
        if d.dist < 1e-9 {
            return 0;
        }
        let (ex, ey) = (f.wx(x) - pad.x, f.wy(y) - pad.y);
        let el = hypot(ex, ey);
        if el < 1e-9 {
            return 0;
        }
        let b = d.bearing.to_radians();
        (fan * 1000.0 * (1.0 - (ex * b.cos() + ey * b.sin()) / el)) as u32
    };
    let via_price = (if drop { r.cfg.plane_via_mm } else { r.cfg.via_mm } * 1000.0) as u32;
    let (mut dirs, mut vias, mut pocket_ways, mut joins) = (0usize, 0usize, 0usize, 0usize);
    let mut want_dirs = target.iter().filter(|t| t.is_some()).count();
    while let Some(Reverse((g, i))) = ws.heap.pop() {
        let i = i as usize;
        if ws.g[i] != g {
            continue;
        }
        ws.pops += 1;
        let (x, y) = (i % f.w, i / f.w);
        let in_pocket_phase = dirs == 0 && g > near;
        // enough: a drop has its vias; every heading is settled and a via found or not to be had nearby;
        // past the near ground only a pad that found no heading goes on, to find its way out of a pocket
        let via_done = vias >= if drop { 3 } else { 2 } || (vias >= 1 && g > (fan * 1000.0) as u32);
        if g > far || ws.pops - pops0 > POCKET_POPS || (drop && via_done) || (!drop && want_dirs == 0 && dirs > 0 && via_done) || (g > near && (dirs > 0 || (drop && vias > 0))) {
            break;
        }
        // a heading's end
        if !drop {
            for h in 0..8 {
                if target[h] == Some((x, y)) {
                    target[h] = None;
                    want_dirs -= 1;
                    let region = gaps.region(l, y, x, f.w);
                    if g <= most[h] && region.map_or(true, |k| gaps.regions[k].open) {
                        let (path, through) = trace(ws, i);
                        if let Some(&k) = through.last() {
                            out.retain(|c| c.how != How::Gap(k));
                        }
                        out.push(Cand { how: How::Dir(h as u8), layer: l as u8, len: g as f64 / 1000.0, path, via: None, gaps: through, cost: g + off(x, y), dead: false });
                        dirs += 1;
                    }
                }
            }
            // out of a pocket: the first node of the open board past each gap
            if in_pocket_phase && ws.last_gap[i] != 0 && pocket_ways < 4 && gaps.open_at(l, y, x, f.w) && hypot(f.wx(x) - pad.x, f.wy(y) - pad.y) >= fan {
                let k = ws.last_gap[i] - 1;
                if !out.iter().any(|c| c.how == How::Gap(k) || c.gaps.last() == Some(&k)) {
                    let (mut path, through) = trace(ws, i);
                    // past its last gap the way out is a lane straight on from the gap's line
                    if let Some(cut) = path.iter().rposition(|&(px, py)| gaps.wall.get(&((base + py as usize * f.w + px as usize) as u32)) == Some(&(k + 1))) {
                        let gp = &gaps.gaps[k as usize];
                        let (mut nx, mut ny) = (-(gp.pb.1 - gp.pa.1) / gp.width, (gp.pb.0 - gp.pa.0) / gp.width);
                        let back = path[cut.saturating_sub(3)];
                        if (path[cut].0 as f64 - back.0 as f64) * nx + (path[cut].1 as f64 - back.1 as f64) * ny < 0.0 {
                            (nx, ny) = (-nx, -ny);
                        }
                        let h = super::geo::compass(nx, ny) as usize;
                        path.truncate(cut + 1);
                        let steps = (fan / (f.pitch * hypot(DX[h] as f64, DY[h] as f64))).round() as usize;
                        for _ in 0..steps {
                            let (lx, ly) = *path.last().unwrap();
                            let (qx, qy) = (lx as i64 + DX[h] as i64, ly as i64 + DY[h] as i64);
                            if qx < 1 || qy < 1 || qx as usize >= f.w - 1 || qy as usize >= f.h - 1 || !legal(qy as usize * f.w + qx as usize) {
                                break;
                            }
                            path.push((qx as u16, qy as u16));
                        }
                    }
                    out.push(Cand { how: How::Gap(k), layer: l as u8, len: g as f64 / 1000.0, path, via: None, gaps: through, cost: g + off(x, y), dead: false });
                    pocket_ways += 1;
                }
            }
        }
        // a pad of the net that reaches the plane itself
        if drop && joins < 2 {
            if let Some(&q) = anchors.get(&(net, (base + i) as u32)) {
                if q as usize != p && !out.iter().any(|c| c.how == How::Join(q)) {
                    let (path, mut through) = trace(ws, i);
                    let q_obj = gaps.pad0 as u32 + q;
                    through.retain(|&k| gaps.gaps[k as usize].a != q_obj && gaps.gaps[k as usize].b != q_obj);
                    out.push(Cand { how: How::Join(q), layer: l as u8, len: g as f64 / 1000.0, path, via: None, gaps: through, cost: g, dead: false });
                    joins += 1;
                }
            }
        }
        // a via site
        if vias < (if drop { 3 } else { 2 }) && via_site(r, net, x, y, dynamic) {
            let clear = out.iter().all(|c| match c.via {
                Some((vx, vy)) => {
                    let (ddx, ddy) = (vx as i64 - x as i64, vy as i64 - y as i64);
                    ddx * ddx + ddy * ddy >= need2
                }
                None => true,
            });
            if clear {
                let (path, through) = trace(ws, i);
                out.push(Cand { how: How::Via, layer: l as u8, len: g as f64 / 1000.0, path, via: Some((x as u16, y as u16)), gaps: through, cost: g + via_price, dead: false });
                vias += 1;
            }
        }
        for nd in 0..8 {
            let (nx, ny) = (x as i64 + DX[nd] as i64, y as i64 + DY[nd] as i64);
            if nx < 1 || ny < 1 || nx as usize >= f.w - 1 || ny as usize >= f.h - 1 {
                continue;
            }
            let j = ny as usize * f.w + nx as usize;
            if !legal(j) {
                continue;
            }
            let ng = g + if nd & 1 == 0 { orth } else { diag };
            if ws.stamp[j] != ws.now || ng < ws.g[j] {
                ws.stamp[j] = ws.now;
                ws.g[j] = ng;
                ws.par[j] = nd as u8;
                // (a node on a gap's line lies in no region: only those are looked up)
                ws.last_gap[j] = match if gaps.label[l][j] == 0 { gaps.wall.get(&((base + j) as u32)) } else { None } {
                    Some(&k) => k,
                    None => ws.last_gap[i],
                };
                ws.heap.push(Reverse((ng, j as u32)));
            }
        }
    }
    // a through-hole pad may leave on any layer that carries tracks
    if pad.tht && !drop {
        for &l2 in &r.routable {
            if l2 == l {
                continue;
            }
            let b2 = l2 * f.w * f.h;
            let free = (0..8u8).any(|h| {
                let (ux, uy) = unit(h);
                let s = support(r, p, h) + fan;
                let (nx, ny) = f.node_near(pad.x + ux * s, pad.y + uy * s);
                nx >= 1 && ny >= 1 && nx < f.w - 1 && ny < f.h - 1 && sdist(&f.cells[b2 + ny * f.w + nx], net) >= t_min
            });
            if free {
                let a = r.acc[p][0];
                out.push(Cand { how: How::Layer(l2 as u8), layer: l2 as u8, len: fan, path: vec![(a.x, a.y)], via: None, gaps: Vec::new(), cost: (fan * 1000.0) as u32 + 1, dead: false });
            }
        }
    }
    if drop {
        in_pad(r, d, &mut out);
    }
    out.sort_by_key(|c| c.cost);
    out
}

/// A via inside the pad itself: a pad big enough to hold one well inside its copper, where that via is
/// clear of every other net on every layer (the drops pass's own rule).
fn in_pad(r: &Router, d: &Demand, out: &mut Vec<Cand>) {
    let pad = &r.sc.pads[d.pad];
    if d.want != Want::Drop || pad.tht {
        return;
    }
    let f = &r.f;
    let (bx0, by0, bx1, by1) = pad.shape.bbox();
    if (bx1 - bx0).min(by1 - by0) < r.cfg.vip_mm {
        return;
    }
    let l = if pad.back { f.nl - 1 } else { 0 };
    let need = r.kind(pad.net).size / 2.0 + 0.05;
    let (x0, y0) = f.node_near(bx0, by0);
    let (x1, y1) = f.node_near(bx1, by1);
    let mut best: Option<(f64, usize, usize)> = None;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (px, py) = (f.wx(x), f.wy(y));
            let vl = r.costs.via_net[y * f.w + x];
            if pad.shape.depth(px, py) < need || (vl != 0 && vl != pad.net) || !(0..f.nl).all(|ll| sdist(&f.cells[f.idx(ll, y, x)], pad.net) >= r.kind(pad.net).t) {
                continue;
            }
            let dist = hypot(px - pad.x, py - pad.y);
            if best.map_or(true, |b| dist < b.0) {
                best = Some((dist, x, y));
            }
        }
    }
    if let Some((dist, x, y)) = best {
        out.push(Cand { how: How::InPad, layer: l as u8, len: dist, path: vec![(x as u16, y as u16)], via: Some((x as u16, y as u16)), gaps: Vec::new(), cost: (dist * 1000.0) as u32 + (r.cfg.plane_via_mm * 1000.0) as u32, dead: false });
    }
}
