//! Copper the board came with: the tracks and vias already on it are part of their nets.
//!
//! Nothing here is moved or lifted. What it joins is joined: the pads a piece of old copper touches, and
//! the pieces that touch each other, are one island, and an island comes into its net's tree whole, pads
//! and copper. A new track may start or end anywhere on it. Two pieces touch as KiCad counts it: an end
//! of one (a via's middle, a pad's middle) lies in the other's copper, on a layer both are on.

use super::{Node, Router};
use crate::field::sdist;
use crate::geom::{arc_points, hypot, Shape};
use crate::scene::PathPiece;
use std::collections::HashMap;

/// Pieces of old copper of one net that touch, with the pads they touch.
pub(crate) struct Isle {
    pub net: u16,
    /// the pads it joins (indices into the scene's pads)
    pub pads: Vec<usize>,
    /// the nodes that stand on its copper, where a track of the net may stand
    pub nodes: Vec<Node>,
    /// the layers it reaches: its tracks' own, every layer of a via's span, every layer for a through pad
    pub layers: u32,
}

#[derive(Default)]
pub(crate) struct Old {
    pub isles: Vec<Isle>,
    /// pad -> the island it stands in
    pub of_pad: HashMap<usize, usize>,
    /// node -> the point of old copper it stands for: the nearest on a track's middle line, a via's middle
    pub at: HashMap<Node, (f64, f64)>,
    /// how many tracks and vias with a net the board came with
    pub pieces: (usize, usize),
}

/// One piece: a straight stretch of track, or a via.
struct Piece {
    net: u16,
    lmask: u32,
    a: (f64, f64),
    b: (f64, f64),
    r: f64,
    via: bool,
}

impl Piece {
    fn holds(&self, p: (f64, f64)) -> bool {
        Shape::Capsule { ax: self.a.0, ay: self.a.1, bx: self.b.0, by: self.b.1, r: self.r }.dist(p.0, p.1) <= 1e-6
    }
    /// The points another piece must hold for the two to be one: a track's two ends, a via's middle.
    fn ends(&self) -> [(f64, f64); 2] {
        [self.a, self.b]
    }
}

fn find(up: &mut [usize], mut a: usize) -> usize {
    while up[a] != a {
        up[a] = up[up[a]];
        a = up[a];
    }
    a
}

impl Router {
    /// The islands of old copper (`Router::old`). Called once, when the field and the pads' nodes stand.
    pub(crate) fn find_old(&mut self) {
        let sc = &self.sc;
        let nl = sc.layers.len();
        let all: u32 = (1u32 << nl) - 1;
        let mut pieces: Vec<Piece> = Vec::new();
        let mut count = (0, 0);
        for t in sc.copper.tracks.iter().filter(|t| t.net != 0 && sc.nets[t.net as usize].pads.len() >= 2) {
            count.0 += 1;
            for piece in &t.path {
                let pts: Vec<(f64, f64)> = match piece {
                    PathPiece::Line(v) => vec![(v[0], v[1]), (v[2], v[3])],
                    PathPiece::Arc(v) => arc_points((v[0], v[1]), (v[2], v[3]), (v[4], v[5]), 0.005),
                };
                for w in pts.windows(2) {
                    pieces.push(Piece { net: t.net, lmask: 1 << t.layer, a: w[0], b: w[1], r: t.width / 2.0, via: false });
                }
            }
        }
        for v in sc.copper.vias.iter().filter(|v| v.net != 0 && sc.nets[v.net as usize].pads.len() >= 2) {
            count.1 += 1;
            let lmask = v.span.map_or(all, |(a, z)| (a.min(z)..=a.max(z)).fold(0u32, |m, l| m | (1 << l)));
            pieces.push(Piece { net: v.net, lmask, a: (v.x, v.y), b: (v.x, v.y), r: v.size / 2.0, via: true });
        }
        let mut old = Old { pieces: count, ..Old::default() };
        if pieces.is_empty() {
            self.old = old;
            return;
        }
        // per net: its pieces, then its pads, joined where they touch
        let mut by_net: HashMap<u16, Vec<usize>> = HashMap::new();
        for (k, p) in pieces.iter().enumerate() {
            by_net.entry(p.net).or_default().push(k);
        }
        let mut nets: Vec<u16> = by_net.keys().copied().collect();
        nets.sort();
        let (pitch, reach) = (self.f.pitch, self.f.pitch * 0.5 + 1e-9);
        for net in nets {
            let ks = &by_net[&net];
            let pads = &sc.nets[net as usize].pads;
            let n = ks.len();
            let mut up: Vec<usize> = (0..n + pads.len()).collect();
            let join = |up: &mut Vec<usize>, a: usize, b: usize| {
                let (ra, rb) = (find(up, a), find(up, b));
                if ra != rb {
                    up[ra.max(rb)] = ra.min(rb);
                }
            };
            let boxes: Vec<(f64, f64, f64, f64)> = ks.iter().map(|&k| (pieces[k].a.0.min(pieces[k].b.0) - pieces[k].r, pieces[k].a.1.min(pieces[k].b.1) - pieces[k].r, pieces[k].a.0.max(pieces[k].b.0) + pieces[k].r, pieces[k].a.1.max(pieces[k].b.1) + pieces[k].r)).collect();
            for i in 0..n {
                for j in i + 1..n {
                    let (p, q) = (&pieces[ks[i]], &pieces[ks[j]]);
                    let (bi, bj) = (boxes[i], boxes[j]);
                    if p.lmask & q.lmask == 0 || bi.0 > bj.2 || bj.0 > bi.2 || bi.1 > bj.3 || bj.1 > bi.3 {
                        continue;
                    }
                    if p.ends().iter().any(|&e| q.holds(e)) || q.ends().iter().any(|&e| p.holds(e)) {
                        join(&mut up, i, j);
                    }
                }
            }
            for (pi, &p) in pads.iter().enumerate() {
                let pad = &sc.pads[p];
                let pmask = if pad.tht { all } else if pad.back { 1 << (nl - 1) } else { 1 };
                let pb = pad.shape.bbox();
                for i in 0..n {
                    let q = &pieces[ks[i]];
                    let bi = boxes[i];
                    if pmask & q.lmask == 0 || bi.0 > pb.2 || pb.0 > bi.2 || bi.1 > pb.3 || pb.1 > bi.3 {
                        continue;
                    }
                    if q.ends().iter().any(|&e| pad.shape.dist(e.0, e.1) <= 1e-6) || q.holds((pad.x, pad.y)) {
                        join(&mut up, i, n + pi);
                    }
                }
            }
            // the islands: every group that holds a piece
            let mut isle_of: HashMap<usize, usize> = HashMap::new();
            for i in 0..n {
                let root = find(&mut up, i);
                let next = old.isles.len();
                let k = *isle_of.entry(root).or_insert(next);
                if k == next {
                    old.isles.push(Isle { net, pads: Vec::new(), nodes: Vec::new(), layers: 0 });
                }
                let q = &pieces[ks[i]];
                old.isles[k].layers |= q.lmask;
                // the nodes on it: along a track's middle line, the node nearest each point of it that lies
                // within half a pitch of the line; a via's nearest node, on each layer it has
                let t = self.t_min(net);
                let len = hypot(q.b.0 - q.a.0, q.b.1 - q.a.1);
                let steps = if q.via { 0 } else { (len / (pitch / 4.0)).ceil() as usize };
                for s in 0..=steps {
                    let u = if steps == 0 { 0.0 } else { s as f64 / steps as f64 };
                    let at = (q.a.0 + (q.b.0 - q.a.0) * u, q.a.1 + (q.b.1 - q.a.1) * u);
                    let (x, y) = self.f.node_near(at.0, at.1);
                    let (wx, wy) = (self.f.wx(x), self.f.wy(y));
                    // the point of the piece's middle line nearest the node
                    let foot = if len < 1e-9 {
                        q.a
                    } else {
                        let v = (((wx - q.a.0) * (q.b.0 - q.a.0) + (wy - q.a.1) * (q.b.1 - q.a.1)) / (len * len)).clamp(0.0, 1.0);
                        (q.a.0 + (q.b.0 - q.a.0) * v, q.a.1 + (q.b.1 - q.a.1) * v)
                    };
                    let d = hypot(wx - foot.0, wy - foot.1);
                    if d > if q.via { pitch } else { reach } {
                        continue;
                    }
                    for &l in self.routable.iter().filter(|&&l| q.lmask & (1 << l) != 0) {
                        if sdist(&self.f.cells[self.f.idx(l, y, x)], net) < t {
                            continue;
                        }
                        let node = (l as u8, y as u16, x as u16);
                        match old.at.get(&node) {
                            Some(&was) if hypot(wx - was.0, wy - was.1) <= d => {}
                            Some(_) => {
                                old.at.insert(node, foot);
                            }
                            None => {
                                old.at.insert(node, foot);
                                old.isles[k].nodes.push(node);
                            }
                        }
                    }
                }
            }
            for (pi, &p) in pads.iter().enumerate() {
                let root = find(&mut up, n + pi);
                if let Some(&k) = isle_of.get(&root) {
                    old.isles[k].pads.push(p);
                    old.of_pad.insert(p, k);
                    if sc.pads[p].tht {
                        old.isles[k].layers = all;
                    }
                }
            }
        }
        self.old = old;
    }

    /// Where the copper is that a path's end node stands for: a track of the job's own that was drawn off
    /// its node, or old copper of the net.
    pub(crate) fn copper_at(&self, j: usize, node: Node) -> Option<(f64, f64)> {
        self.jobs[j].moved.iter().rev().find(|m| m.0 == node).map(|m| m.1).or_else(|| self.old.at.get(&node).copied())
    }
}
