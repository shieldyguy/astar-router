//! Copper the board came with: the tracks and vias already on it are part of their nets.
//!
//! Nothing here is moved or lifted. What it joins is joined: the pads a piece of old copper touches, and
//! the pieces that touch each other, are one island, and an island comes into its net's tree whole, pads
//! and copper. A new track may start or end anywhere on it. Two pieces touch as KiCad counts it: their
//! copper overlaps on a layer both are on, by a nanometre or more. That is so of a track whose round end
//! lies on a pad, of a track that runs over a pad, of two tracks that cross, and of two pads that lie on
//! each other with no track at all (a pin over the edge of a thermal pad, a hole's pad inside it): pads
//! that touch are an island too. What a footprint draws in copper round a pad (a tab, an antenna) is not
//! old copper in this sense: KiCad gives it no net and joins nothing through it, so a track of the pad's
//! net that ends on it is not joined, and the track goes on to the pad.

use super::{Node, Router};
use crate::field::sdist;
use crate::geom::{arc_points, hypot, seg_seg_dist2, Shape};
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
    /// whether it has copper of its own: false for pads that lie on each other with no track between
    pub copper: bool,
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
    /// how many islands are pads alone, lying on each other
    pub pad_isles: usize,
    /// shorts the board came with: a pad, the other net whose copper or pad it lies on, and where. (Pads
    /// of one part that lie on each other are a net tie, the part's own business.)
    pub shorts: Vec<(usize, u16, (f64, f64))>,
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

/// How much two pieces of copper must overlap to be one: KiCad's unit of length, a nanometre.
const TOUCH: f64 = 1e-6;

/// How much copper of two nets must overlap to be called a short the board came with: a micron, well
/// past what rounding leaves between two things drawn edge to edge.
const SHORT: f64 = 1e-3;

impl Piece {
    fn touches(&self, o: &Piece) -> bool {
        seg_seg_dist2(self.a, self.b, o.a, o.b).sqrt() <= self.r + o.r - TOUCH
    }
    fn on(&self, pad: &Shape) -> bool {
        pad.dist_seg(self.a, self.b) <= self.r - TOUCH
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
        for t in sc.copper.tracks.iter().filter(|t| t.net != 0 && !t.drawn && sc.nets[t.net as usize].pads.len() >= 2) {
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
        // per net: its pieces, then its pads, joined where they touch
        let mut by_net: HashMap<u16, Vec<usize>> = HashMap::new();
        for (k, p) in pieces.iter().enumerate() {
            by_net.entry(p.net).or_default().push(k);
        }
        let none: Vec<usize> = Vec::new();
        let (pitch, reach) = (self.f.pitch, self.f.pitch * 0.5 + 1e-9);
        for net in (1..sc.nets.len() as u16).filter(|&n| sc.nets[n as usize].pads.len() >= 2) {
            let ks = by_net.get(&net).unwrap_or(&none);
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
                    if p.touches(q) {
                        join(&mut up, i, j);
                    }
                }
            }
            let pmask = |p: usize| if sc.pads[p].tht { all } else if sc.pads[p].back { 1 << (nl - 1) } else { 1 };
            let pboxes: Vec<(f64, f64, f64, f64)> = pads.iter().map(|&p| sc.pads[p].shape.bbox()).collect();
            for pi in 0..pads.len() {
                for pj in pi + 1..pads.len() {
                    let (bi, bj) = (pboxes[pi], pboxes[pj]);
                    if pmask(pads[pi]) & pmask(pads[pj]) == 0 || bi.0 > bj.2 || bj.0 > bi.2 || bi.1 > bj.3 || bj.1 > bi.3 {
                        continue;
                    }
                    if sc.pads[pads[pi]].shape.overlaps(&sc.pads[pads[pj]].shape, TOUCH) {
                        join(&mut up, n + pi, n + pj);
                    }
                }
            }
            for (pi, &p) in pads.iter().enumerate() {
                let pad = &sc.pads[p];
                let pmask = pmask(p);
                let pb = pboxes[pi];
                for i in 0..n {
                    let q = &pieces[ks[i]];
                    let bi = boxes[i];
                    if pmask & q.lmask == 0 || bi.0 > pb.2 || pb.0 > bi.2 || bi.1 > pb.3 || pb.1 > bi.3 {
                        continue;
                    }
                    if q.on(&pad.shape) {
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
                    old.isles.push(Isle { net, pads: Vec::new(), nodes: Vec::new(), layers: 0, copper: true });
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
            // pads that touch each other with no piece between them are an island too
            let mut mates: HashMap<usize, usize> = HashMap::new();
            for pi in 0..pads.len() {
                *mates.entry(find(&mut up, n + pi)).or_insert(0) += 1;
            }
            for (pi, &p) in pads.iter().enumerate() {
                let root = find(&mut up, n + pi);
                if !isle_of.contains_key(&root) && mates[&root] >= 2 {
                    isle_of.insert(root, old.isles.len());
                    old.isles.push(Isle { net, pads: Vec::new(), nodes: Vec::new(), layers: 0, copper: false });
                    old.pad_isles += 1;
                }
                if let Some(&k) = isle_of.get(&root) {
                    old.isles[k].pads.push(p);
                    old.of_pad.insert(p, k);
                    old.isles[k].layers |= pmask(p);
                }
            }
        }
        // shorts the board came with: a pad that lies on old copper of another net, or on a pad of
        // another net of another part. Nothing here can mend one: it is said, and the run is not whole
        let pmask = |p: usize| if sc.pads[p].tht { all } else if sc.pads[p].back { 1 << (nl - 1) } else { 1 };
        let wired: Vec<usize> = (0..sc.pads.len()).filter(|&p| sc.pads[p].net != 0).collect();
        let pboxes: Vec<(f64, f64, f64, f64)> = wired.iter().map(|&p| sc.pads[p].shape.bbox()).collect();
        for (i, &p) in wired.iter().enumerate() {
            let (pad, pb) = (&sc.pads[p], pboxes[i]);
            for q in pieces.iter().filter(|q| q.net != pad.net && pmask(p) & q.lmask != 0) {
                if q.a.0.min(q.b.0) - q.r > pb.2 || q.a.0.max(q.b.0) + q.r < pb.0 || q.a.1.min(q.b.1) - q.r > pb.3 || q.a.1.max(q.b.1) + q.r < pb.1 {
                    continue;
                }
                if pad.shape.dist_seg(q.a, q.b) <= q.r - SHORT && !old.shorts.iter().any(|s| s.0 == p && s.1 == q.net) {
                    old.shorts.push((p, q.net, (pad.x, pad.y)));
                }
            }
            for (j, &o) in wired.iter().enumerate().skip(i + 1) {
                let (other, ob) = (&sc.pads[o], pboxes[j]);
                if other.net == pad.net || other.ref_ == pad.ref_ || pmask(p) & pmask(o) == 0 || pb.0 > ob.2 || ob.0 > pb.2 || pb.1 > ob.3 || ob.1 > pb.3 {
                    continue;
                }
                if pad.shape.overlaps(&other.shape, SHORT) {
                    old.shorts.push((p, other.net, (other.x, other.y)));
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
