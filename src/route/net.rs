//! A whole net: its pads joined one at a time into a tree.

use super::{Ask, Job, JobKind, Router};
use crate::geom::hypot;
use crate::scene::Scene;
use std::collections::HashMap;

/// What comes into a net's tree.
enum Come {
    Pad(usize),
    /// a group of the pour's pieces
    Fill(u32),
    /// an island of old copper (`old`)
    Isle(usize),
}

/// A net's tree as it grows.
struct Grown {
    joined: Vec<bool>,
    /// per pad: how far the nearest joined pad is
    mind: Vec<f64>,
    /// every node the net's copper stands on (a later pad may join it anywhere)
    tree: Vec<(u8, u16, u16)>,
    /// the pour's groups the tree has, and their nodes with fill on all four sides
    taken: Vec<u32>,
    fill_t: Vec<u32>,
    /// the islands of old copper the tree has
    isles: Vec<usize>,
    /// the fill the tree leans on: (a cell, the group it was in)
    ends: Vec<(u32, u32)>,
}

pub const NET: JobKind = JobKind { name: "net", pass: "route", route: Router::route_net };

impl Router {
    /// One job a net that has no plane: wide nets first (they need the room), then the short before
    /// the long.
    pub(crate) fn make_net_jobs(&mut self) {
        // what never changes about the pours of the nets that have one (`fill`)
        for ni in 1..self.sc.nets.len() {
            if self.sc.nets[ni].pads.len() >= 2 && self.sc.nets[ni].plane.is_none() {
                if let Some(b) = self.pour_base(ni as u16) {
                    self.pours.insert(ni as u16, b);
                }
            }
        }
        let mut nets = Vec::new();
        for (ni, n) in self.sc.nets.iter().enumerate() {
            if ni == 0 || !n.wanted() || n.plane.is_some() {
                continue;
            }
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for &p in &n.pads {
                let pad = &self.sc.pads[p];
                x0 = x0.min(pad.x);
                y0 = y0.min(pad.y);
                x1 = x1.max(pad.x);
                y1 = y1.max(pad.y);
            }
            nets.push((ni, n.width, (x1 - x0) + (y1 - y0)));
        }
        nets.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.2.partial_cmp(&b.2).unwrap()));
        for (ni, _, _) in nets {
            self.add_job(Job::new(NET, ni as u16, None));
        }
    }

    /// Is this a surface pad inside a ring of its part's other pads (the exposed pad of a QFN)? Such a pad
    /// cannot be left on its own layer.
    pub(crate) fn ringed(&self, p: usize) -> bool {
        let pad = &self.sc.pads[p];
        if pad.tht {
            return false;
        }
        let (x0, y0, x1, y1) = pad.shape.bbox();
        if (x1 - x0).min(y1 - y0) < self.cfg.vip_mm {
            return false;
        }
        let (mut l, mut r, mut u, mut d) = (false, false, false, false);
        for (qi, q) in self.sc.pads.iter().enumerate() {
            if qi == p || q.ref_ != pad.ref_ || q.back != pad.back {
                continue;
            }
            let (in_x, in_y) = (q.x > x0 && q.x < x1, q.y > y0 && q.y < y1);
            l |= q.x < x0 && in_y;
            r |= q.x > x1 && in_y;
            u |= q.y < y0 && in_x;
            d |= q.y > y1 && in_x;
        }
        l && r && u && d
    }

    /// A whole net: its pads joined one at a time into a tree, the nearest pad next. Each search runs
    /// from the new pad to the tree: a pad that other copper has shut in then shows it at once (its
    /// search runs dry in a few nodes) instead of after the whole board has been searched from the far side.
    /// A net with a pour: the pads its fill joins come into the tree together, and a track may end
    /// anywhere in fill the tree has (`fill`).
    pub(crate) fn route_net(&mut self, j: usize) {
        let net = self.jobs[j].net;
        let pads: Vec<usize> = self.sc.nets[net as usize].pads.clone();
        let n = pads.len();
        // node -> pad, for telling which pad a path ended on
        // (two pads that face each other may have the one node between them: both are kept)
        let mut at: HashMap<(u8, u16, u16), Vec<usize>> = HashMap::new();
        for (i, &p) in pads.iter().enumerate() {
            for &l in &self.pad_layers(p) {
                for a in &self.acc[p] {
                    at.entry((l as u8, a.y, a.x)).or_default().push(i);
                }
            }
        }
        // an exposed pad of a net with no plane to drop to takes a via in itself, and the net leaves it
        // from that via on the other layers
        let mut pad_via: Vec<Option<(u16, u16)>> = vec![None; n];
        for i in 0..n {
            let p = pads[i];
            if !self.ringed(p) {
                continue;
            }
            let l = self.pad_layers(p)[0];
            let Some(&ol) = self.routable.iter().find(|&&ll| ll != l) else { continue };
            if let Some((x, y)) = self.via_site_in_pad(net, p) {
                self.commit(j, &[(l as u8, y as u16, x as u16), (ol as u8, y as u16, x as u16)], Some(p), None);
                pad_via[i] = Some((x as u16, y as u16));
            }
        }
        let (mut cx, mut cy) = (0.0, 0.0);
        for &p in &pads {
            cx += self.sc.pads[p].x / n as f64;
            cy += self.sc.pads[p].y / n as f64;
        }
        let dist = |a: usize, b: usize, sc: &Scene| hypot(sc.pads[pads[a]].x - sc.pads[pads[b]].x, sc.pads[pads[a]].y - sc.pads[pads[b]].y);
        let pour = self.pour(net);
        // With a pour the tree starts in the pour: in its largest piece (with the pieces and the pads
        // that piece is joined to). A pour is copper to go down into, as a plane is: every pad then
        // reaches it the shortest way it can, and none is first wired to another pad through it.
        let mut most: HashMap<u32, usize> = HashMap::new();
        let mut size: HashMap<u32, usize> = HashMap::new();
        if let Some(pm) = &pour {
            for &g in &pm.pad_group {
                *most.entry(g).or_insert(0) += 1;
            }
            for c in 1..pm.cells.len() {
                *size.entry(pm.comp_group[c]).or_insert(0) += pm.cells[c].len();
            }
        }
        let big = size.iter().max_by_key(|&(&g, &c)| (c, std::cmp::Reverse(g))).map(|(&g, _)| g).or_else(|| most.iter().max_by_key(|&(&g, &c)| (c, std::cmp::Reverse(g))).map(|(&g, _)| g));
        let nearest = |in_big: bool| {
            (0..n).filter(|&i| !self.acc[pads[i]].is_empty() && (!in_big || pour.as_ref().map_or(true, |pm| Some(pm.pad_group[i]) == big))).min_by(|&a, &b| {
                let da = hypot(self.sc.pads[pads[a]].x - cx, self.sc.pads[pads[a]].y - cy);
                let db = hypot(self.sc.pads[pads[b]].x - cx, self.sc.pads[pads[b]].y - cy);
                da.partial_cmp(&db).unwrap()
            })
        };
        // (the seed: a pad of the piece the tree starts in; where that piece joins no pad yet, the tree
        // starts as the fill alone, and the pad nearest the middle only sets the order of the others)
        let in_fill = nearest(true);
        let seed = in_fill.or_else(|| nearest(false));
        let Some(seed) = seed else {
            self.jobs[j].open = pads.clone();
            self.jobs[j].done = true;
            return;
        };
        let mut failed = vec![false; n];
        // the tree as it grows
        let mut g = Grown { joined: vec![false; n], mind: (0..n).map(|i| dist(i, seed, &self.sc)).collect(), tree: Vec::new(), taken: Vec::new(), fill_t: Vec::new(), isles: Vec::new(), ends: Vec::new() };
        // pad access nodes whose stub to the pad's centre is drawn already
        let mut stubbed: Vec<(usize, u16, u16)> = Vec::new();
        let (fw, per) = (self.f.w, self.f.w * self.f.h);
        // the net's islands of old copper (`old`), and for each the pieces of the pour its copper lies in:
        // (a cell of the piece, the piece's group)
        let isle_of: Vec<Option<usize>> = pads.iter().map(|p| self.old.of_pad.get(p).copied()).collect();
        let mut isle_fill: std::collections::BTreeMap<usize, Vec<(u32, u32)>> = std::collections::BTreeMap::new();
        if let Some(pm) = &pour {
            for (k, isle) in self.old.isles.iter().enumerate().filter(|(_, i)| i.net == net) {
                let mut seen: Vec<u32> = Vec::new();
                for &(l, y, x) in &isle.nodes {
                    let cell = self.f.idx(l as usize, y as usize, x as usize);
                    let c = pm.comp[cell];
                    if c != 0 && !seen.contains(&c) {
                        seen.push(c);
                        isle_fill.entry(k).or_default().push((cell as u32, pm.comp_group[c as usize]));
                    }
                }
            }
        }
        // What comes into the tree comes with all it is joined to already: a pad with the fill of its pour
        // and with its island of old copper, fill with the pads and the islands it reaches, an island with
        // its pads and the fill it lies in.
        let take = |first: Come, r: &Router, g: &mut Grown| {
            let mut todo = vec![first];
            while let Some(what) = todo.pop() {
                match what {
                    Come::Pad(i) => {
                        if g.joined[i] {
                            continue;
                        }
                        g.joined[i] = true;
                        for a in &r.acc[pads[i]] {
                            for &l in &r.pad_layers(pads[i]) {
                                g.tree.push((l as u8, a.y, a.x));
                            }
                        }
                        if let Some((x, y)) = pad_via[i] {
                            for &l in &r.routable {
                                g.tree.push((l as u8, y, x));
                            }
                        }
                        for k in 0..n {
                            let d = dist(k, i, &r.sc);
                            if d < g.mind[k] {
                                g.mind[k] = d;
                            }
                        }
                        if let Some(pm) = &pour {
                            todo.push(Come::Fill(pm.pad_group[i]));
                        }
                        if let Some(k) = isle_of[i] {
                            todo.push(Come::Isle(k));
                        }
                    }
                    Come::Fill(grp) => {
                        let Some(pm) = &pour else { continue };
                        if g.taken.contains(&grp) {
                            continue;
                        }
                        g.taken.push(grp);
                        todo.extend((0..n).filter(|&i| !g.joined[i] && pm.pad_group[i] == grp).map(Come::Pad));
                        // (its nodes with fill on all four sides: a track may end on any of them)
                        for c in (1..pm.cells.len()).filter(|&c| pm.comp_group[c] == grp) {
                            for &k in &pm.cells[c] {
                                let k = k as usize;
                                let (x, y) = ((k % per) % fw, (k % per) / fw);
                                if x > 0 && y > 0 && x + 1 < fw && y + 1 < r.f.h && [k - 1, k + 1, k - fw, k + fw].iter().all(|&q| pm.comp[q] == c as u32) {
                                    g.fill_t.push(k as u32);
                                }
                            }
                        }
                        for (&k, cells) in &isle_fill {
                            if cells.iter().any(|c| c.1 == grp) {
                                todo.push(Come::Isle(k));
                            }
                        }
                    }
                    Come::Isle(k) => {
                        if g.isles.contains(&k) {
                            continue;
                        }
                        g.isles.push(k);
                        g.tree.extend(r.old.isles[k].nodes.iter().copied());
                        todo.extend(r.old.isles[k].pads.iter().filter_map(|p| pads.iter().position(|q| q == p)).map(Come::Pad));
                        // old copper that lies in the pour is one with it, and leans on that fill
                        for &(cell, grp) in isle_fill.get(&k).map_or(&[][..], |v| &v[..]) {
                            g.ends.push((cell, grp));
                            todo.push(Come::Fill(grp));
                        }
                    }
                }
            }
        };
        // (copper a pass laid for the net before the route, a pair's run: the tree starts there, so each
        // pad goes to the end of it that is nearest and none is first wired to another past it)
        let laid = (0..self.old.isles.len()).find(|&k| self.old.isles[k].net == net && self.old.isles[k].laid);
        match (&pour, big, in_fill, laid) {
            (Some(_), Some(grp), None, _) if size.contains_key(&grp) => take(Come::Fill(grp), self, &mut g),
            (None, _, _, Some(k)) => take(Come::Isle(k), self, &mut g),
            _ => take(Come::Pad(seed), self, &mut g),
        }
        // the pads the board itself leaves no way to (`Job::walled`): one with no node to stand on, and
        // those a routing before this saw walled in. What stays does not change, so they are not tried
        // again; but a pour's fill does, and a net with one is asked afresh each time.
        let mut walled: Vec<usize> = if pour.is_some() && !self.strict { Vec::new() } else { self.jobs[j].walled.clone() };
        let mut walled_loose = 0;
        for i in 0..n {
            if self.acc[pads[i]].is_empty() && !walled.contains(&pads[i]) {
                walled.push(pads[i]);
            }
            if walled.contains(&pads[i]) && (pour.is_none() || self.acc[pads[i]].is_empty()) {
                failed[i] = true;
            }
        }
        // old copper of the net that no pad stands on is part of the net all the same (KiCad counts a
        // track that hangs loose as not joined): each such island is joined like a pad, once the pads are
        let lone: Vec<usize> = (0..self.old.isles.len()).filter(|&k| self.old.isles[k].net == net && self.old.isles[k].pads.is_empty()).collect();
        let mut loose: Vec<usize> = Vec::new();
        loop {
            let next = (0..n).filter(|&i| !g.joined[i] && !failed[i]).min_by(|&a, &b| g.mind[a].partial_cmp(&g.mind[b]).unwrap());
            let isle = if next.is_none() { lone.iter().copied().find(|k| !g.isles.contains(k) && !loose.contains(k)) } else { None };
            if next.is_none() && isle.is_none() {
                break;
            }
            let mut sources: Vec<(u8, u16, u16, u32)> = Vec::new();
            let mut mates: Vec<usize> = Vec::new();
            if let Some(u) = next {
                self.pad_sources(pads[u], &mut sources);
                if let Some((x, y)) = pad_via[u] {
                    for &l in &self.routable {
                        sources.push((l as u8, y, x, 0));
                    }
                }
                // a pad that stands in an island of old copper: the track may leave from anywhere on
                // the island, or from another pad of it
                if let Some(k) = isle_of[u] {
                    sources.extend(self.old.isles[k].nodes.iter().map(|&(l, y, x)| (l, y, x, 0)));
                    mates = self.old.isles[k].pads.iter().filter_map(|p| pads.iter().position(|q| q == p)).filter(|&i| i != u).collect();
                    for &i in &mates {
                        self.pad_sources(pads[i], &mut sources);
                    }
                }
            }
            if let Some(k) = isle {
                sources.extend(self.old.isles[k].nodes.iter().map(|&(l, y, x)| (l, y, x, 0)));
                // (no node of it where a track may stand, or nothing yet to join it to)
                if sources.is_empty() || (g.tree.is_empty() && g.fill_t.is_empty()) {
                    loose.push(k);
                    continue;
                }
            }
            // the goal is any node of the tree; the heuristic aims at the tree node nearest the pad
            let (ux, uy) = match next {
                Some(u) => self.f.node_near(self.sc.pads[pads[u]].x, self.sc.pads[pads[u]].y),
                None => (sources[0].2 as usize, sources[0].1 as usize),
            };
            let mut aim = g.tree.first().copied().unwrap_or((0, 0, 0));
            let mut best = i64::MAX;
            for &k in &g.fill_t {
                let k = k as usize;
                self.search.mark_target_at(k);
                let (x, y) = ((k % per) % fw, (k % per) / fw);
                let (dx, dy) = (x as i64 - ux as i64, y as i64 - uy as i64);
                let d = dx * dx + dy * dy;
                if d < best {
                    best = d;
                    aim = ((k / per) as u8, y as u16, x as u16);
                }
            }
            for &(l, y, x) in &g.tree {
                self.search.mark_target(&self.f, l as usize, y as usize, x as usize);
                let (dx, dy) = (x as i64 - ux as i64, y as i64 - uy as i64);
                let d = dx * dx + dy * dy;
                if d < best {
                    best = d;
                    aim = (l, y, x);
                }
            }
            // a pad with a front and a back is aimed at down its whole middle, not at its centre
            let tbox = at.get(&aim).and_then(|v| v.iter().find_map(|&i| self.exit_box.get(&pads[i]).copied())).unwrap_or((aim.2 as i32, aim.1 as i32, aim.2 as i32, aim.1 as i32));
            let ask = Ask {
                net,
                sources,
                tbox,
                tlayers: 1 << aim.0,
                plane_goal: None,
                own_vias: self.net_vias[net as usize].clone(),
                own_pads: Vec::new(),
                via_mm: self.cfg.via_mm,
                rip_mm: self.cfg.rip_mm,
                hard: 30_000 + (self.f.w * self.f.h) as u64 / 40,
                cap: 400_000 + (self.f.w * self.f.h) as u64 / 4,
                shut: false,
                what: match next {
                    _ if !self.cfg.verbose => String::new(),
                    Some(u) => format!("net {} {}.{}", self.sc.nets[net as usize].name, self.sc.pads[pads[u]].ref_, self.sc.pads[pads[u]].num),
                    None => format!("net {} old copper", self.sc.nets[net as usize].name),
                },
            };
            let found = self.solve(&ask);
            self.search.clear_targets();
            let Some((path, may_clash)) = found else {
                match (next, isle) {
                    (Some(u), _) => {
                        failed[u] = true;
                        if self.no_way && !walled.contains(&pads[u]) {
                            walled.push(pads[u]);
                        }
                    }
                    (_, Some(k)) => {
                        loose.push(k);
                        walled_loose += self.no_way as usize;
                    }
                    _ => {}
                }
                continue;
            };
            let before = self.jobs[j].clash_cells.len();
            if may_clash {
                self.note_clashes(j, &path);
            }
            let clashed = self.jobs[j].clash_cells.len() > before;
            let path = if clashed { path } else { self.shape(net, path) };
            // the far end: on a joined pad's access node the track runs on to that pad's centre (once)
            let last = *path.last().unwrap();
            let mut to = at.get(&last).and_then(|v| v.iter().copied().find(|&i| Some(i) != next && g.joined[i])).map(|i| pads[i]);
            if let Some(p) = to {
                if stubbed.contains(&(p, last.2, last.1)) {
                    to = None;
                } else {
                    stubbed.push((p, last.2, last.1));
                }
            }
            // the near end: the pad, or the pad of its island the path left from; a path that starts on
            // the pad's own via has its piece from the pad's centre already, and one that starts on old
            // copper starts there (`Router::copper_at`)
            let first = path[0];
            let starts_at = |i: usize| self.pad_layers(pads[i]).contains(&(first.0 as usize)) && self.access_at(pads[i], first.2, first.1).is_some();
            let from = match next {
                Some(u) if pad_via[u].map_or(false, |(x, y)| first.2 == x && first.1 == y) => None,
                Some(u) if starts_at(u) => Some(pads[u]),
                _ => mates.iter().copied().find(|&i| starts_at(i)).map(|i| pads[i]),
            };
            if let Some(p) = from {
                stubbed.push((p, first.2, first.1));
            }
            self.commit(j, &path, from, to);
            // (a path laid through other copper is kept as found, wiggles and all: once that copper has
            // gone it is pulled tight and drawn again, `redraw`)
            self.jobs[j].rough |= clashed;
            // a track that ends in the pour's fill leans on that fill
            if let Some(pm) = &pour {
                let k = self.f.idx(last.0 as usize, last.1 as usize, last.2 as usize);
                let c = pm.comp[k] as usize;
                if c != 0 && g.taken.contains(&pm.comp_group[c]) {
                    g.ends.push((k as u32, pm.comp_group[c]));
                }
            }
            for (k, &(l, y, x)) in path.iter().enumerate() {
                let change = (k > 0 && path[k - 1].0 != l) || (k + 1 < path.len() && path[k + 1].0 != l);
                if change {
                    for &ll in &self.routable {
                        g.tree.push((ll as u8, y, x));
                    }
                } else {
                    g.tree.push((l, y, x));
                }
            }
            match (next, isle) {
                (Some(u), _) => take(Come::Pad(u), self, &mut g),
                (_, Some(k)) => take(Come::Isle(k), self, &mut g),
                _ => {}
            }
        }
        self.jobs[j].loose = loose.iter().filter_map(|&k| self.old.isles[k].nodes.first().map(|&(_, y, x)| (self.f.wx(x as usize), self.f.wy(y as usize)))).collect();
        if let Some(pm) = &pour {
            self.jobs[j].pour_pads = pm.pad_group.clone();
            self.jobs[j].pour_ends = g.ends;
        }
        self.jobs[j].open = (0..n).filter(|&i| !g.joined[i]).map(|i| pads[i]).collect();
        // (a pad that came in with the fill or with old copper after all is not walled)
        walled.retain(|p| self.jobs[j].open.contains(p));
        if !self.strict {
            self.jobs[j].walled_loose = walled_loose;
        }
        self.jobs[j].walled = walled;
        self.jobs[j].done = true;
    }
}
