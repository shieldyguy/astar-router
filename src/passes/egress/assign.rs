//! The assignment: one way out a pad, such that no gap carries more than its lanes, no two ways out of
//! different nets come nearer than a track and its clearance, and no two vias stand too close; of all
//! such, the one that leaves the fewest pads without a way and then costs least.
//!
//! Ways out clash pairwise, which a flow cannot say, so each group of pads whose ways out can clash or
//! share a gap is searched exactly (depth first, the cheapest way first, cut off by what is already
//! known to be as good), up to a number of steps; past that the best found so far stands.

use super::demand::Demand;
use super::exits::{Cand, How};
use super::gaps::Gaps;
use super::geo::line_line;
use crate::route::Router;
use std::collections::HashMap;

/// What a pad without a way out costs: more than any assignment of ways.
pub const NONE: u64 = 1_000_000_000;
/// Steps of the exact search a group of pads may take.
pub const STEPS: u64 = 20_000;

/// What may not be chosen together.
pub struct Model {
    /// demand -> the number of its first candidate among all candidates
    pub first: Vec<usize>,
    /// per candidate: the candidates of other pads it cannot stand beside
    pub clash: Vec<Vec<u32>>,
    /// per candidate: the demand it belongs to
    pub owner: Vec<usize>,
    /// per gap: its lanes
    pub caps: Vec<u32>,
    /// groups of demands that can be settled apart
    pub groups: Vec<Vec<usize>>,
}

/// A path with its straight runs as single pieces, mm.
fn corners(r: &Router, c: &Cand) -> Vec<(f64, f64)> {
    let pts = c.points(r);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (i, &p) in pts.iter().enumerate() {
        if i > 0 && i + 1 < pts.len() {
            let (a, b) = (pts[i - 1], pts[i + 1]);
            if ((p.0 - a.0) * (b.1 - p.1) - (p.1 - a.1) * (b.0 - p.0)).abs() < 1e-9 {
                continue;
            }
        }
        out.push(p);
    }
    out
}

pub fn model(r: &Router, gaps: &Gaps, demands: &[Demand], cands: &[Vec<Cand>]) -> Model {
    let (w, c, via) = (r.sc.rules.track, r.sc.rules.clearance, r.sc.rules.via);
    let f = &r.f;
    let mut first = Vec::with_capacity(cands.len());
    let mut owner: Vec<usize> = Vec::new();
    for (di, cs) in cands.iter().enumerate() {
        first.push(owner.len());
        owner.extend(std::iter::repeat(di).take(cs.len()));
    }
    let n = owner.len();
    let cand = |id: usize| -> &Cand { &cands[owner[id]][id - first[owner[id]]] };
    let lines: Vec<Vec<(f64, f64)>> = (0..n).map(|id| corners(r, cand(id))).collect();
    // candidates by where they are: 1 mm bins over each one's box, a track's reach wider
    let reach = via + c + w;
    let mut bins: HashMap<(i32, i32, u8), Vec<u32>> = HashMap::new();
    let boxes: Vec<(f64, f64, f64, f64)> = lines
        .iter()
        .map(|l| l.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1))))
        .collect();
    for id in 0..n {
        if matches!(cand(id).how, How::Layer(_)) {
            continue;
        }
        let b = boxes[id];
        // (a via is on every layer: layer 255 stands for all)
        let layers: &[u8] = if cand(id).via.is_some() { &[cand(id).layer, 255] } else { &[cand(id).layer] };
        for by in ((b.1 - reach).floor() as i32)..=((b.3 + reach).floor() as i32) {
            for bx in ((b.0 - reach).floor() as i32)..=((b.2 + reach).floor() as i32) {
                for &l in layers {
                    bins.entry((bx, by, l)).or_default().push(id as u32);
                }
            }
        }
    }
    let mut clash: Vec<Vec<u32>> = vec![Vec::new(); n];
    let at = |v: (u16, u16)| (f.wx(v.0 as usize), f.wy(v.1 as usize));
    let mut keys: Vec<&(i32, i32, u8)> = bins.keys().collect();
    keys.sort();
    for key in keys {
        let ids = &bins[key];
        for (k, &a) in ids.iter().enumerate() {
            for &b in &ids[k + 1..] {
                let (a, b) = (a as usize, b as usize);
                if owner[a] == owner[b] || clash[a].contains(&(b as u32)) {
                    continue;
                }
                let (ca, cb) = (cand(a), cand(b));
                if r.sc.pads[demands[owner[a]].pad].net == r.sc.pads[demands[owner[b]].pad].net {
                    // one net: its ways may run together; two of its vias are one via or stand apart
                    if let (Some(va), Some(vb)) = (ca.via, cb.via) {
                        let d = line_line(&[at(va)], &[at(vb)]);
                        if d > 1e-9 && d < via + 0.1 - 1e-6 {
                            clash[a].push(b as u32);
                            clash[b].push(a as u32);
                        }
                    }
                    continue;
                }
                let both_vias = match (ca.via, cb.via) {
                    (Some(va), Some(vb)) => line_line(&[at(va)], &[at(vb)]) < via + c - 1e-6,
                    _ => false,
                };
                let via_on_path = |v: Option<(u16, u16)>, other: usize| v.map_or(false, |v| line_line(&[at(v)], &lines[other]) < via / 2.0 + c + w / 2.0 - 1e-6);
                let paths = ca.layer == cb.layer && line_line(&lines[a], &lines[b]) < w + c - 1e-6;
                let hit = both_vias || paths || via_on_path(ca.via, b) || via_on_path(cb.via, a);
                if hit {
                    clash[a].push(b as u32);
                    clash[b].push(a as u32);
                }
            }
        }
    }
    // groups: pads whose candidates clash or pass the same gap
    let mut up: Vec<usize> = (0..cands.len()).collect();
    fn root(up: &mut Vec<usize>, mut i: usize) -> usize {
        while up[i] != i {
            up[i] = up[up[i]];
            i = up[i];
        }
        i
    }
    for a in 0..n {
        for &b in &clash[a] {
            let (x, y) = (root(&mut up, owner[a]), root(&mut up, owner[b as usize]));
            up[x.max(y)] = x.min(y);
        }
    }
    let mut through: HashMap<u32, usize> = HashMap::new();
    for id in 0..n {
        for &g in &cand(id).gaps {
            let other = *through.entry(g).or_insert(owner[id]);
            let (x, y) = (root(&mut up, owner[id]), root(&mut up, other));
            up[x.max(y)] = x.min(y);
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut at_group: HashMap<usize, usize> = HashMap::new();
    for di in 0..cands.len() {
        let k = root(&mut up, di);
        let g = *at_group.entry(k).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[g].push(di);
    }
    Model { first, clash, owner, caps: gaps.gaps.iter().map(|g| g.lanes).collect(), groups }
}

impl Model {
    /// May these choices (per demand: a candidate or none) stand together?
    pub fn fits(&self, cands: &[Vec<Cand>], choice: &[Option<usize>]) -> bool {
        let mut used: HashMap<u32, u32> = HashMap::new();
        let chosen: Vec<u32> = choice.iter().enumerate().filter_map(|(di, c)| c.map(|c| (self.first[di] + c) as u32)).collect();
        for (di, c) in choice.iter().enumerate() {
            let Some(c) = *c else { continue };
            if cands[di][c].dead || self.clash[self.first[di] + c].iter().any(|o| chosen.contains(o)) {
                return false;
            }
            for &g in &cands[di][c].gaps {
                let u = used.entry(g).or_insert(0);
                *u += 1;
                if *u > self.caps[g as usize] {
                    return false;
                }
            }
        }
        true
    }

    /// What a set of choices costs: `NONE` for each pad without a way, and the ways' own costs.
    pub fn cost(&self, cands: &[Vec<Cand>], choice: &[Option<usize>]) -> u64 {
        choice.iter().enumerate().map(|(di, c)| c.map_or(NONE, |c| cands[di][c].cost as u64)).sum()
    }
}

/// The choices as they stand while a group is being settled.
struct State<'a> {
    m: &'a Model,
    cands: &'a [Vec<Cand>],
    /// per candidate: how many chosen candidates it clashes with
    blocked: Vec<u32>,
    /// per gap: how many chosen candidates pass it
    used: Vec<u32>,
    choice: Vec<Option<usize>>,
}

impl State<'_> {
    fn free(&self, di: usize, c: usize) -> bool {
        let cand = &self.cands[di][c];
        !cand.dead && self.blocked[self.m.first[di] + c] == 0 && cand.gaps.iter().all(|&g| self.used[g as usize] < self.m.caps[g as usize])
    }
    fn put(&mut self, di: usize, c: usize) {
        for &o in &self.m.clash[self.m.first[di] + c] {
            self.blocked[o as usize] += 1;
        }
        for &g in &self.cands[di][c].gaps {
            self.used[g as usize] += 1;
        }
        self.choice[di] = Some(c);
    }
    fn take(&mut self, di: usize) -> Option<usize> {
        let c = self.choice[di].take()?;
        for &o in &self.m.clash[self.m.first[di] + c] {
            self.blocked[o as usize] -= 1;
        }
        for &g in &self.cands[di][c].gaps {
            self.used[g as usize] -= 1;
        }
        Some(c)
    }
    /// The pads whose chosen ways stand in the way of this candidate.
    fn in_the_way(&self, di: usize, c: usize) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for &o in &self.m.clash[self.m.first[di] + c] {
            let dj = self.m.owner[o as usize];
            if self.choice[dj] == Some(o as usize - self.m.first[dj]) && !out.contains(&dj) {
                out.push(dj);
            }
        }
        for &g in &self.cands[di][c].gaps {
            if self.used[g as usize] >= self.m.caps[g as usize] {
                for (dj, ch) in self.choice.iter().enumerate() {
                    if let Some(ch) = ch {
                        if self.cands[dj][*ch].gaps.contains(&g) && !out.contains(&dj) {
                            out.push(dj);
                        }
                    }
                }
            }
        }
        out
    }
    /// Give a pad a way out, moving up to two other pads to other ways when that is what it takes,
    /// and those in turn, `depth` deep. True when every pad touched ends with a way.
    fn settle(&mut self, di: usize, depth: u32, busy: &mut Vec<usize>) -> bool {
        for c in 0..self.cands[di].len() {
            if self.free(di, c) {
                self.put(di, c);
                return true;
            }
        }
        if depth == 0 {
            return false;
        }
        busy.push(di);
        for c in 0..self.cands[di].len() {
            if self.cands[di][c].dead {
                continue;
            }
            let movers = self.in_the_way(di, c);
            if movers.is_empty() || movers.len() > 2 || movers.iter().any(|m| busy.contains(m)) {
                continue;
            }
            let keep = (self.blocked.clone(), self.used.clone(), self.choice.clone());
            for &m in &movers {
                self.take(m);
            }
            if self.free(di, c) {
                self.put(di, c);
                if movers.iter().all(|&m| self.settle(m, depth - 1, busy)) {
                    busy.pop();
                    return true;
                }
            }
            (self.blocked, self.used, self.choice) = keep;
        }
        busy.pop();
        false
    }
}

struct Hunt<'a, 'b> {
    st: &'b mut State<'a>,
    order: Vec<usize>,
    /// per place in the order: the least the pads from there on can cost
    floor: Vec<u64>,
    best: Vec<Option<usize>>,
    best_cost: u64,
    steps: u64,
}

impl Hunt<'_, '_> {
    fn go(&mut self, k: usize, cost: u64) {
        if cost + self.floor[k] >= self.best_cost {
            return;
        }
        if k == self.order.len() {
            self.best_cost = cost;
            self.best = self.order.iter().map(|&di| self.st.choice[di]).collect();
            return;
        }
        self.steps += 1;
        let di = self.order[k];
        for c in 0..self.st.cands[di].len() {
            if self.steps > STEPS {
                return;
            }
            if !self.st.free(di, c) {
                continue;
            }
            self.st.put(di, c);
            self.go(k + 1, cost + self.st.cands[di][c].cost as u64);
            self.st.take(di);
        }
        if self.steps <= STEPS {
            self.go(k + 1, cost + NONE);
        }
    }
}

/// One candidate a demand, or none where there is no way: (the choices, the groups that were not
/// searched to the end). Each group first greedily, the pads with the fewest ways first, a pad left
/// without a way moving others aside; then the exact search, which keeps that unless it finds better.
pub fn solve(m: &Model, cands: &[Vec<Cand>]) -> (Vec<Option<usize>>, usize) {
    let mut st = State { m, cands, blocked: vec![0u32; m.clash.len()], used: vec![0u32; m.caps.len()], choice: vec![None; cands.len()] };
    let mut cut_short = 0;
    for group in &m.groups {
        let mut order = group.clone();
        order.sort_by_key(|&di| (cands[di].iter().filter(|c| !c.dead).count(), di));
        // the cheapest way first; when that leaves pads without a way, once more with the way that
        // stands in the fewest others' way first, and the better of the two is kept
        let mut tries: Vec<(usize, u64, Vec<Option<usize>>)> = Vec::new();
        for gentle in [false, true] {
            for &di in &order {
                st.take(di);
            }
            let mut left: Vec<usize> = Vec::new();
            for &di in &order {
                let mut ways: Vec<usize> = (0..cands[di].len()).collect();
                if gentle {
                    ways.sort_by_key(|&c| (m.clash[m.first[di] + c].len() + 4 * cands[di][c].gaps.len(), c));
                }
                match ways.into_iter().find(|&c| st.free(di, c)) {
                    Some(c) => st.put(di, c),
                    None => left.push(di),
                }
            }
            for &di in &left {
                let mut busy = Vec::new();
                st.settle(di, 5, &mut busy);
            }
            let none = order.iter().filter(|&&di| st.choice[di].is_none()).count();
            let cost: u64 = order.iter().map(|&di| st.choice[di].map_or(NONE, |c| cands[di][c].cost as u64)).sum();
            tries.push((none, cost, order.iter().map(|&di| st.choice[di]).collect()));
            if none == 0 {
                break;
            }
        }
        let (_, _, kept) = tries.into_iter().min_by_key(|t| (t.0, t.1)).unwrap();
        for &di in &order {
            st.take(di);
        }
        for (k, &di) in order.iter().enumerate() {
            if let Some(c) = kept[k] {
                st.put(di, c);
            }
        }
        if order.len() < 2 {
            continue;
        }
        let first: Vec<Option<usize>> = order.iter().map(|&di| st.choice[di]).collect();
        let first_cost: u64 = order.iter().map(|&di| st.choice[di].map_or(NONE, |c| cands[di][c].cost as u64)).sum();
        let mut floor = vec![0u64; order.len() + 1];
        for k in (0..order.len()).rev() {
            floor[k] = floor[k + 1] + cands[order[k]].iter().filter(|c| !c.dead).map(|c| c.cost as u64).min().unwrap_or(NONE);
        }
        for &di in &order {
            st.take(di);
        }
        let mut h = Hunt { st: &mut st, order, floor, best: first, best_cost: first_cost, steps: 0 };
        h.go(0, 0);
        if h.steps > STEPS {
            cut_short += 1;
        }
        let (order, best) = (h.order, h.best);
        for &di in &order {
            st.take(di);
        }
        for (k, &di) in order.iter().enumerate() {
            if let Some(c) = best[k] {
                st.put(di, c);
            }
        }
    }
    (st.choice, cut_short)
}
