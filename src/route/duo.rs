//! Differential pairs: two nets that run side by side.
//!
//! A pair runs between two parts that both of its nets have a pad on (a connector or its ESD array, the
//! chip). Between them the two tracks are one thing in the negotiation: a centre line, found by the
//! search for a net of its own that has no pad and keeps half the pair's breadth more from everything
//! than a plain net does, and the two tracks drawn off that line, one to each side, the pair's gap apart
//! (`Trunk`). The line pays for what it runs through like any path, so the pair and its neighbours
//! settle who gives way by price; lifting either net of the pair lifts both, and both are routed again
//! together, the first of them (the lead) finding the line. Each net's tree then starts at the two ends
//! of its track: every pad goes to the nearer end.
//!
//! What this is not: the pair stays on one layer between its two parts (a pair with no way there is
//! routed as two nets, and the plan says so), and its two halves are not made the same length (the plan
//! gives the difference).

use super::solve::Ask;
use super::{Node, Router, Track};
use crate::geom::hypot;
use crate::scene::{Class, Net, Scene};

/// How far from the middle of a pair's two pads its side-by-side run may begin, mm.
const REACH: f64 = 2.5;

type Pt = (f64, f64);

pub(crate) struct Duo {
    pub name: String,
    pub p: u16,
    pub n: u16,
    /// the pad-less net the centre line is searched as
    pub centre: u16,
    pub layer: usize,
    pub width: f64,
    pub gap: f64,
    /// at each of the two parts the pair runs between: the pad of p, the pad of n
    pub ends: [(usize, usize); 2],
    /// how often the pair, laid side by side, left a pad open or joined one only the long way round
    pub fails: u32,
    /// given up: its nets are routed as two nets from here on
    pub off: bool,
    /// what the plan says of it
    pub said: String,
}

/// A net's half of its pair's run: the track, and at each end the node a track of the net may end on
/// with the exact point that node stands for.
#[derive(Clone)]
pub struct Trunk {
    pub track: Track,
    pub ends: [(Node, Pt); 2],
}

/// The two parts a pair runs between: the two farthest apart of the parts both nets have a pad on, with
/// the pad of each net there (the nearest two, where a part has more).
fn ends(sc: &Scene, p: usize, n: usize) -> Option<[(usize, usize); 2]> {
    let mut shared: Vec<(usize, usize)> = Vec::new();
    for &a in &sc.nets[p].pads {
        let ref_ = &sc.pads[a].ref_;
        if ref_.is_empty() || shared.iter().any(|e| sc.pads[e.0].ref_ == *ref_) {
            continue;
        }
        let mut best: Option<(f64, usize, usize)> = None;
        for &x in sc.nets[p].pads.iter().filter(|&&x| sc.pads[x].ref_ == *ref_) {
            for &y in sc.nets[n].pads.iter().filter(|&&y| sc.pads[y].ref_ == *ref_) {
                let d = hypot(sc.pads[x].x - sc.pads[y].x, sc.pads[x].y - sc.pads[y].y);
                if best.map_or(true, |b| d < b.0) {
                    best = Some((d, x, y));
                }
            }
        }
        if let Some((_, x, y)) = best {
            shared.push((x, y));
        }
    }
    if shared.len() < 2 {
        return None;
    }
    let mid = |e: &(usize, usize)| ((sc.pads[e.0].x + sc.pads[e.1].x) / 2.0, (sc.pads[e.0].y + sc.pads[e.1].y) / 2.0);
    let (mut bi, mut bj, mut bd) = (0, 1, -1.0);
    for i in 0..shared.len() {
        for j in i + 1..shared.len() {
            let (a, b) = (mid(&shared[i]), mid(&shared[j]));
            let d = hypot(a.0 - b.0, a.1 - b.1);
            if d > bd {
                (bi, bj, bd) = (i, j, d);
            }
        }
    }
    Some([shared[bi], shared[bj]])
}

/// The pairs of the scene that can be routed side by side, each with a net of its own for its centre
/// line added to the scene; and what the plan says of those that cannot: (net, why).
pub(crate) fn plan(sc: &mut Scene, skip: bool) -> (Vec<Duo>, Vec<(u16, String)>) {
    let (mut duos, mut not) = (Vec::new(), Vec::new());
    if skip {
        return (duos, not);
    }
    let nl = sc.layers.len();
    for i in 0..sc.pairs.len() {
        let (p, n) = (sc.pairs[i].p as usize, sc.pairs[i].n as usize);
        if !sc.nets[p].wanted() || !sc.nets[n].wanted() || sc.nets[p].plane.is_some() || sc.nets[n].plane.is_some() {
            continue;
        }
        let two = |why: String| (p as u16, format!("routed as two nets, not as a pair: {}", why));
        let Some(e) = ends(sc, p, n) else {
            not.push(two("its two nets share fewer than two parts".into()));
            continue;
        };
        let side = |q: usize| if sc.pads[q].tht { None } else { Some(if sc.pads[q].back { nl - 1 } else { 0 }) };
        let sides: Vec<usize> = [e[0].0, e[0].1, e[1].0, e[1].1].iter().filter_map(|&q| side(q)).collect();
        let layer = sides.first().copied().unwrap_or(0);
        if sides.iter().any(|&l| l != layer) {
            not.push(two("its pads are on both sides of the board, and a pair is kept to one layer".into()));
            continue;
        }
        if sc.nets.iter().any(|x| x.plane == Some(layer)) || [p, n].iter().any(|&x| sc.nets[x].keep != 0 && sc.nets[x].keep & (1 << layer) == 0) {
            not.push(two(format!("its pads' layer, {}, is not open to its tracks", sc.layers[layer])));
            continue;
        }
        // the pair's own sizes: its width, and its gap, which is no less than the clearance between its nets
        let clr = sc.rules.clearance.max(sc.nets[p].clearance).max(sc.nets[n].clearance);
        let width = sc.pairs[i].width.unwrap_or(sc.nets[p].width.max(sc.nets[n].width));
        let gap = sc.pairs[i].gap.unwrap_or(clr).max(clr);
        // the centre line's net keeps from everything what the outer edge of either track must: half the
        // pair's breadth more than the clearance. It is searched at the narrowest a track of its width is
        // drawn (`Router::w_min`), so it keeps the difference to its full width too: the pair never necks.
        let narrow = {
            let w = width.min(sc.rules.track);
            match sc.rules.floors.track {
                Some(f) if f > 0.0 && f < w => f,
                _ => w,
            }
        };
        let centre = sc.nets.len() as u16;
        let mut line = Net::plain(format!("{} (centre line)", sc.pairs[i].name), Class::Signal, width, clr + (width + gap) / 2.0 + 0.01 + (width - narrow) / 2.0);
        // (kept to the pair's layer: its search never looks on another)
        line.keep = 1 << layer;
        sc.nets.push(line);
        duos.push(Duo { name: sc.pairs[i].name.clone(), p: p as u16, n: n as u16, centre, layer, width, gap, ends: e, fails: 0, off: false, said: String::new() });
    }
    (duos, not)
}

/// A line of points drawn `d` to its left (negative: to its right). On the inside of a turn the corner
/// is mitred; on the outside it is cut flat, so that no point of the drawn line is further than `d`
/// from the line it is drawn beside. None where a run is too short for its corners at that distance.
fn beside(pts: &[Pt], d: f64) -> Option<Vec<Pt>> {
    let k = pts.len();
    let dir = |i: usize| {
        let (a, b) = (pts[i], pts[i + 1]);
        let l = hypot(b.0 - a.0, b.1 - a.1);
        ((b.0 - a.0) / l, (b.1 - a.1) / l)
    };
    let left = |u: Pt| (-u.1, u.0);
    let mut out = Vec::with_capacity(k + 2);
    // (the run each point of `out` starts, to see below that it still points the way it did)
    let mut run_of: Vec<usize> = Vec::with_capacity(k + 2);
    for j in 0..k {
        if j == 0 || j == k - 1 {
            let m = left(dir(if j == 0 { 0 } else { k - 2 }));
            out.push((pts[j].0 + m.0 * d, pts[j].1 + m.1 * d));
            run_of.push(j);
            continue;
        }
        let (u, v) = (dir(j - 1), dir(j));
        let (a, b) = (left(u), left(v));
        let s = 1.0 + a.0 * b.0 + a.1 * b.1;
        if s < 0.3 {
            return None; // a turn too sharp
        }
        // a turn to the left has its inside on the left
        let turn = u.0 * v.1 - u.1 * v.0;
        if turn * d >= 0.0 {
            out.push((pts[j].0 + (a.0 + b.0) / s * d, pts[j].1 + (a.1 + b.1) / s * d));
            run_of.push(j);
        } else {
            out.push((pts[j].0 + a.0 * d, pts[j].1 + a.1 * d));
            run_of.push(usize::MAX);
            out.push((pts[j].0 + b.0 * d, pts[j].1 + b.1 * d));
            run_of.push(j);
        }
    }
    for i in 0..out.len() - 1 {
        if run_of[i] == usize::MAX {
            continue;
        }
        let (u, v) = (dir(run_of[i]), (out[i + 1].0 - out[i].0, out[i + 1].1 - out[i].1));
        if u.0 * v.0 + u.1 * v.1 <= 1e-6 {
            return None;
        }
    }
    Some(out)
}

fn length(pts: &[Pt]) -> f64 {
    pts.windows(2).map(|w| hypot(w[1].0 - w[0].0, w[1].1 - w[0].1)).sum()
}

/// The line from `a` mm past its start to `b` mm before its end.
fn cut(pts: &[Pt], a: f64, b: f64) -> Vec<Pt> {
    let total = length(pts);
    let at = |s: f64| -> Pt {
        let mut left = s;
        for w in pts.windows(2) {
            let l = hypot(w[1].0 - w[0].0, w[1].1 - w[0].1);
            if left <= l {
                return (w[0].0 + (w[1].0 - w[0].0) * left / l, w[0].1 + (w[1].1 - w[0].1) * left / l);
            }
            left -= l;
        }
        *pts.last().unwrap()
    };
    let mut out = vec![at(a)];
    let mut run = 0.0;
    for w in pts.windows(2) {
        run += hypot(w[1].0 - w[0].0, w[1].1 - w[0].1);
        if run > a + 1e-9 && run < total - b - 1e-9 {
            out.push(w[1]);
        }
    }
    out.push(at(total - b));
    out
}

impl Router {
    /// The pair a net is in, while that pair is still routed side by side.
    pub(crate) fn duo_of(&self, net: u16) -> Option<usize> {
        self.duos.iter().position(|d| !d.off && (d.p == net || d.n == net))
    }

    /// The job that routes a net's pads (`net::NET`).
    pub(crate) fn net_job(&self, net: u16) -> Option<usize> {
        self.jobs.iter().position(|j| j.net == net && j.kind.name == super::net::NET.name)
    }

    /// A net's half of its pair's run, on the board and in its job.
    pub(crate) fn stand_trunk(&mut self, j: usize) {
        let Some(t) = self.jobs[j].trunk.clone() else { return };
        self.stand(j, std::slice::from_ref(&t.track));
        self.jobs[j].tracks.push(t.track);
        self.jobs[j].moved.extend(t.ends);
    }

    /// Both nets of a pair, routed together: the run side by side (`lay_duo`), then each net's pads to
    /// the ends of its track. `pj`, `nj`: the jobs of the pair's two nets, the lead's first. A pair that
    /// keeps leaving a pad open, or joined only the long way round, is given up and routed as two nets.
    pub(crate) fn route_duo(&mut self, di: usize, pj: usize, nj: usize) {
        // (the mate stands or falls with the lead)
        if self.jobs[nj].done || !self.jobs[nj].objs.is_empty() {
            self.lift(nj);
        }
        let laid = self.lay_duo(di, pj, nj);
        self.route_tree(pj);
        self.route_tree(nj);
        if !laid {
            // no room at its pads, or no way at all: given up at once. Not laid for the copper that stands
            // today: given up the third time
            self.duos[di].fails += 1;
            self.duos[di].off |= self.duos[di].fails >= 3;
            return;
        }
        for (a, b) in [(pj, nj), (nj, pj)] {
            if !self.deps[a].contains(&b) {
                self.deps[a].push(b);
            }
        }
        // what the pads' joins took, and what the straight way to the nearer end would be
        let mut round: Option<(f64, f64)> = None;
        for j in [pj, nj] {
            let Some(t) = &self.jobs[j].trunk else { continue };
            let ends = [t.ends[0].1, t.ends[1].1];
            let own = length(&t.track.pts);
            let net = self.jobs[j].net as usize;
            let straight: f64 = self.sc.nets[net].pads.iter().map(|&q| ends.iter().map(|e| hypot(self.sc.pads[q].x - e.0, self.sc.pads[q].y - e.1)).fold(f64::MAX, f64::min)).sum();
            let took: f64 = self.jobs[j].tracks.iter().map(|t| t.length()).sum::<f64>() - own;
            if took > 2.0 * straight + 2.0 && round.map_or(true, |r| took - straight > r.0 - r.1) {
                round = Some((took, straight));
            }
        }
        // (its own two nets in each other's way: the pads at an end lie the other way round than the
        // tracks come in, and no price settles that)
        let crossed = self.pairs.contains(&(pj, nj)) || self.pairs.contains(&(nj, pj));
        let open = self.for_room(pj) || self.for_room(nj);
        if !open && !crossed && round.is_none() {
            return;
        }
        self.duos[di].fails += 1;
        if self.duos[di].fails < 3 {
            return;
        }
        self.duos[di].off = true;
        self.duos[di].said = match round {
            Some((took, straight)) if !open => format!("routed as two nets: side by side, its pads could be joined to it only the long way round ({:.1} mm of track where {:.1} mm is the straight way)", took, straight),
            _ if crossed && !open => "routed as two nets: side by side, its two tracks come to its pads the other way round than the pads lie".to_string(),
            _ => "routed as two nets: side by side, it left a pad of its own with no way to it".to_string(),
        };
        self.lift(pj);
        self.route_tree(pj);
        self.route_tree(nj);
    }

    /// The run of a pair side by side: its centre line found, the two tracks drawn off it and stood on
    /// the board, one in each net's job (`Job::trunk`). False when the pair has no such run here; what
    /// the plan says of it is in `Duo::said` either way.
    fn lay_duo(&mut self, di: usize, pj: usize, nj: usize) -> bool {
        let (net, layer, w, gap, e) = { let d = &self.duos[di]; (d.centre, d.layer, d.width, d.gap, d.ends) };
        let (p, n) = (self.duos[di].p, self.duos[di].n);
        let d = (w + gap) / 2.0;
        let two = |why: String| format!("routed as two nets, not as a pair: {}", why);
        let t_min = self.t_min(net);
        let mid = |e: (usize, usize)| ((self.sc.pads[e.0].x + self.sc.pads[e.1].x) / 2.0, (self.sc.pads[e.0].y + self.sc.pads[e.1].y) / 2.0);
        let (ma, mb) = (mid(e[0]), mid(e[1]));
        // where the centre line may stand near an end: every node within reach of the middle of the two
        // pads that the board itself leaves clear for it
        let near = |m: Pt| -> Vec<(usize, usize, f64)> {
            let f = &self.f;
            let ((x0, y0), (x1, y1)) = (f.node_near(m.0 - REACH, m.1 - REACH), f.node_near(m.0 + REACH, m.1 + REACH));
            let mut out = Vec::new();
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let dm = hypot(f.wx(x) - m.0, f.wy(y) - m.1);
                    if dm <= REACH && crate::field::sdist(&f.cells[f.idx(layer, y, x)], net) >= t_min {
                        out.push((x, y, dm));
                    }
                }
            }
            out
        };
        let (from, to) = (near(ma), near(mb));
        if from.is_empty() || to.is_empty() {
            self.duos[di].said = two("there is no room for the two tracks side by side near its pads".into());
            self.duos[di].off = true;
            return false;
        }
        // (a start further from the pads costs three times its distance: the run begins as near as it can)
        let sources: Vec<(u8, u16, u16, u32)> = from.iter().map(|&(x, y, dm)| (layer as u8, y as u16, x as u16, (dm * 3000.0) as u32)).collect();
        let mut tbox = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let best = to.iter().fold(f64::MAX, |b, t| b.min(t.2));
        for &(x, y, dm) in &to {
            // (the far end: the nodes nearest the middle of its pads)
            if dm <= best + 2.0 * self.f.pitch {
                let i = self.f.idx(layer, y, x);
                self.search.mark_target_at(i);
                tbox = (tbox.0.min(x as i32), tbox.1.min(y as i32), tbox.2.max(x as i32), tbox.3.max(y as i32));
            }
        }
        let cells = (self.f.w * self.f.h) as u64;
        let what = if self.cfg.verbose { format!("pair {}", self.duos[di].name) } else { String::new() };
        let ask = Ask { net, sources, tbox, tlayers: 1 << layer, plane_goal: None, own_vias: Vec::new(), own_pads: Vec::new(), via_mm: 1.0e4, rip_mm: self.cfg.rip_mm, hard: 30_000 + cells / 40, cap: 400_000 + cells / 4, shut: false, what };
        // where the board itself leaves the two tracks no way there is nothing to search for
        let found = if self.walled_in(&ask) { None } else { self.solve(&ask) };
        self.search.clear_targets();
        let on = self.sc.layers[layer].clone();
        let Some((path, may_clash)) = found else {
            self.duos[di].said = two(format!("the two tracks side by side find no way on {}", on));
            self.duos[di].off = true;
            return false;
        };
        // what the line runs through is the pair's clash, noted on the lead
        let (cells_before, pairs_before) = (self.jobs[pj].clash_cells.len(), self.pairs.len());
        if may_clash {
            self.note_clashes_as(pj, net, &path);
        }
        let clashed = self.jobs[pj].clash_cells.len() > cells_before;
        let path = if clashed { path } else { self.shape(net, path) };
        let mut centre: Vec<Pt> = Vec::new();
        for q in &path {
            let pt = (self.f.wx(q.2 as usize), self.f.wy(q.1 as usize));
            if centre.last().map_or(true, |l| hypot(l.0 - pt.0, l.1 - pt.1) > 1e-9) {
                // (a point on the line of the two before it only lengthens that run)
                if centre.len() >= 2 {
                    let (a, b) = (centre[centre.len() - 2], centre[centre.len() - 1]);
                    if ((b.0 - a.0) * (pt.1 - a.1) - (b.1 - a.1) * (pt.0 - a.0)).abs() < 1e-9 {
                        centre.pop();
                    }
                }
                centre.push(pt);
            }
        }
        let give_up = |r: &mut Router, why: String| {
            r.jobs[pj].clash_cells.truncate(cells_before);
            r.pairs.truncate(pairs_before);
            r.duos[di].said = why;
            false
        };
        if centre.len() < 2 || length(&centre) < 4.0 * d {
            return give_up(self, two("its pads are too near each other for a run side by side".into()));
        }
        // which side each net is on, by where its pads lie at each end: the same at both, or the pair is crossed
        let cross = |u: Pt, e: (usize, usize)| u.0 * (self.sc.pads[e.0].y - self.sc.pads[e.1].y) - u.1 * (self.sc.pads[e.0].x - self.sc.pads[e.1].x);
        let k = centre.len();
        let u0 = (centre[1].0 - centre[0].0, centre[1].1 - centre[0].1);
        let u1 = (centre[k - 1].0 - centre[k - 2].0, centre[k - 1].1 - centre[k - 2].1);
        let (sa, sb) = (cross(u0, e[0]), cross(u1, e[1]));
        if sa * sb < 0.0 {
            return give_up(self, two("it is crossed: one net is on the left at one end and on the right at the other, and the two tracks may not cross".into()));
        }
        let p_left = if sa.abs() >= sb.abs() { sa > 0.0 } else { sb > 0.0 };
        // the two tracks, cut back from each end until both are clear by the exact measure: of everything
        // fixed, and once no clash may stay of the routed copper too (a clash is the centre line's to pay)
        let half = w / 2.0;
        let strict = self.strict || !clashed;
        let clear = |r: &Router, pts: &[Pt], own: u16| pts.windows(2).all(|s| if strict { r.piece_clear(s[0], s[1], half, own, layer) } else { r.statics.track_clear(s[0], s[1], half, r.net_clr(own), 1 << layer, own) });
        let step = self.f.pitch;
        let most = ((REACH / step) as usize).min(((length(&centre) - 4.0 * d) / step / 2.0) as usize);
        let mut sides: Option<(Vec<Pt>, Vec<Pt>, f64)> = None;
        'cut: for sum in 0..=2 * most {
            for ca in 0..=sum.min(most) {
                let cb = sum - ca;
                if cb > most {
                    continue;
                }
                let c = cut(&centre, ca as f64 * step, cb as f64 * step);
                let (Some(l), Some(rt)) = (beside(&c, d), beside(&c, -d)) else { continue };
                let (pp, nn) = if p_left { (l, rt) } else { (rt, l) };
                if clear(self, &pp, p) && clear(self, &nn, n) {
                    sides = Some((pp, nn, length(&c)));
                    break 'cut;
                }
            }
        }
        let Some((pp, nn, run)) = sides else {
            return give_up(self, two("its two tracks side by side do not fit among the copper near its pads".into()));
        };
        // on the board, then the node at each end where a track of the net may end
        for (j, pts) in [(pj, &pp), (nj, &nn)] {
            let track = Track { layer: layer as u8, width: w, pts: pts.clone(), arcs: Vec::new() };
            self.stand(j, std::slice::from_ref(&track));
            self.jobs[j].tracks.push(track);
        }
        let mut trunks: Vec<Trunk> = Vec::new();
        for (j, pts) in [(pj, &pp), (nj, &nn)] {
            let own = self.jobs[j].net;
            let t = self.t_min(own);
            let node_at = |at: Pt| -> Option<Node> {
                let (x0, y0) = self.f.node_near(at.0, at.1);
                let mut best: Option<(f64, Node)> = None;
                for y in y0.saturating_sub(1)..=(y0 + 1).min(self.f.h - 1) {
                    for x in x0.saturating_sub(1)..=(x0 + 1).min(self.f.w - 1) {
                        let dd = hypot(self.f.wx(x) - at.0, self.f.wy(y) - at.1);
                        let c = &self.f.cells[self.f.idx(layer, y, x)];
                        if dd <= step && crate::field::sdist(c, own) >= t && (!strict || crate::field::ddist(c, own) >= t) && best.map_or(true, |b| dd < b.0) {
                            best = Some((dd, (layer as u8, y as u16, x as u16)));
                        }
                    }
                }
                best.map(|b| b.1)
            };
            let (a, z) = (pts[0], pts[pts.len() - 1]);
            if let (Some(na), Some(nz)) = (node_at(a), node_at(z)) {
                trunks.push(Trunk { track: self.jobs[j].tracks.last().unwrap().clone(), ends: [(na, a), (nz, z)] });
            }
        }
        if trunks.len() < 2 {
            for j in [pj, nj] {
                let objs = std::mem::take(&mut self.jobs[j].objs);
                self.f.remove_objs(&objs);
                self.jobs[j].tracks.clear();
            }
            return give_up(self, two("a track of it cannot be gone on from at the end of the run side by side".into()));
        }
        let (lp, ln) = (length(&pp), length(&nn));
        for (j, t) in [pj, nj].into_iter().zip(trunks) {
            self.jobs[j].moved.extend(t.ends);
            self.jobs[j].trunk = Some(t);
        }
        self.duos[di].said = format!(
            "laid as a pair on {}: {:.1} mm side by side, {} mm wide and {} mm apart, from {} to {}; {} is {:.2} mm the longer there (the halves are not made the same length)",
            on,
            run,
            crate::json::num(w),
            crate::json::num(gap),
            self.sc.pads[e[0].0].ref_,
            self.sc.pads[e[1].0].ref_,
            if lp >= ln { &self.sc.nets[p as usize].name } else { &self.sc.nets[n as usize].name },
            (lp - ln).abs()
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_drawn_beside_another_keeps_its_distance_round_a_corner() {
        let c = [(0.0, 0.0), (4.0, 0.0), (6.0, 2.0), (6.0, 6.0)];
        let (l, r) = (beside(&c, 0.2).unwrap(), beside(&c, -0.2).unwrap());
        for (a, b) in [(&l, &r), (&r, &l)] {
            for w in a.windows(2) {
                for v in b.windows(2) {
                    assert!(crate::geom::seg_seg_dist2(w[0], w[1], v[0], v[1]).sqrt() >= 0.4 - 1e-9);
                }
            }
        }
        // and neither is further than its distance from the line it is drawn beside
        for q in l.iter().chain(&r) {
            let far = c.windows(2).map(|w| crate::geom::seg_seg_dist2(w[0], w[1], *q, *q).sqrt()).fold(f64::MAX, f64::min);
            assert!(far <= 0.2 + 1e-9, "{:?} is {} from the line", q, far);
        }
        // the inner one is the shorter by what the two 45 degree corners take
        assert!((length(&l) - length(&r)).abs() > 0.3);
        // a run too short for its corners at that distance is refused
        assert!(beside(&[(0.0, 0.0), (1.0, 0.0), (1.1, 0.1), (1.1, 2.0)], 0.5).is_none());
        assert_eq!(cut(&c, 1.0, 1.0), vec![(1.0, 0.0), (4.0, 0.0), (6.0, 2.0), (6.0, 5.0)]);
    }
}
