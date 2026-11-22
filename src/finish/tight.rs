//! Pull tight: a found path redrawn as the fewest straight runs and 45 degree elbows the field allows.

use crate::field::{ddist, sdist, NO_DIR};
use crate::search::{DX, DY};
use crate::passes::{Pass, Step};
use crate::route::Router;

pub const PASS: Pass = Pass {
    name: "tight",
    about: "every path the route finds is redrawn, before it is laid, as the fewest straight runs and 45 degree elbows the field allows",
    step: Step::Path(Router::smooth),
};

impl Router {
    /// How many nodes of the straight run from (x, y), `k` steps along (dx, dy), are too tight for the net's full width `t_nom`; None when one is
    /// not clear for it even at `t_min`.
    pub(crate) fn line_necks(&self, net: u16, l: usize, x: i32, y: i32, dx: i32, dy: i32, k: i32, t_min: crate::field::Dist, t_nom: crate::field::Dist) -> Option<u32> {
        let mut n = 0;
        for s in 1..=k {
            let (nx, ny) = (x + dx * s, y + dy * s);
            if nx < 0 || ny < 0 || nx >= self.f.w as i32 || ny >= self.f.h as i32 {
                return None;
            }
            let c = &self.f.cells[self.f.idx(l, ny as usize, nx as usize)];
            let d = sdist(c, net).min(ddist(c, net));
            let p = &self.f.cells[self.f.idx(l, (ny - dy) as usize, (nx - dx) as usize)];
            // (a step the grid is not sure of is measured, as the search measured it)
            if (d < t_min || sdist(p, net).min(ddist(p, net)) < t_min) && !self.step_free(net, l, ((nx - dx) as usize, (ny - dy) as usize), (nx as usize, ny as usize)) {
                return None;
            }
            // (a node another net's cost term prices counts like a tight one: the search paid to keep
            // out of a pin's lane, and the redrawing does not take the track back into it)
            n += (t_nom > t_min && d < t_nom) as u32 + (self.costs.step_price(&self.f, self.f.idx(l, ny as usize, nx as usize), net, 1) > 0) as u32;
        }
        Some(n)
    }

    /// Pull a found path tight: on each layer, replace stretches of it by a straight run or one 45 degree
    /// elbow wherever that is clear, taking the longest stretch first. The search only promises a short
    /// path; this is what makes it a drawn one. Ends, vias and the length (never longer) are kept.
    pub(crate) fn smooth(&self, net: u16, path: &[(u8, u16, u16)]) -> Vec<(u8, u16, u16)> {
        let (t_min, t_nom) = (self.t_min(net), self.t_nom(net));
        let mut out: Vec<(u8, u16, u16)> = Vec::with_capacity(path.len());
        let mut i = 0;
        while i < path.len() {
            let l = path[i].0;
            let mut k = i;
            while k + 1 < path.len() && path[k + 1].0 == l {
                k += 1;
            }
            let run = &path[i..=k];
            if self.costs.step[l as usize][0] == 0 {
                out.extend_from_slice(run);
            } else {
                // a pad with a front and a back is left and reached where the search did it
                // (`passes::exits`): the stretch in such a pad and the stretches between are redrawn each
                // by itself, and none turns 90 degrees where it meets the pad's edge
                let pinned = |n: &(u8, u16, u16)| self.costs.exits.contains_key(&(self.f.idx(n.0 as usize, n.1 as usize, n.2 as usize) as u32));
                let step = |a: (u8, u16, u16), b: (u8, u16, u16)| (b.2 as i32 - a.2 as i32, b.1 as i32 - a.1 as i32);
                let fence = |n: &(u8, u16, u16)| {
                    let c = &self.f.cells[self.f.idx(n.0 as usize, n.1 as usize, n.2 as usize)];
                    (c.fdir != NO_DIR && c.sn1 == net).then(|| (DX[c.fdir as usize], DY[c.fdir as usize]))
                };
                let mut s = 0;
                while s < run.len() {
                    let pin = pinned(&run[s]);
                    let mut e = s;
                    while e + 1 < run.len() && pinned(&run[e + 1]) == pin {
                        e += 1;
                    }
                    if e - s < 2 {
                        out.extend_from_slice(&run[s..=e]);
                    } else {
                        // (a run that starts or ends at a pin's fence leaves it, and meets it, on its line)
                        let from = if s > 0 { Some(step(run[s - 1], run[s])) } else { fence(&run[s]) };
                        let to = if e + 1 < run.len() { Some(step(run[e], run[e + 1])) } else { fence(&run[e]).map(|h| (-h.0, -h.1)) };
                        match (from, to) {
                            // the hops are taken from the pad's end, so that the longest one is the one at
                            // the pad
                            (None, Some(t)) => {
                                let back: Vec<(u8, u16, u16)> = run[s..=e].iter().rev().copied().collect();
                                let mut drawn = Vec::with_capacity(back.len());
                                self.smooth_run(net, l as usize, &back, t_min, t_nom, Some((-t.0, -t.1)), None, &mut drawn);
                                out.extend(drawn.into_iter().rev());
                            }
                            _ => self.smooth_run(net, l as usize, &run[s..=e], t_min, t_nom, from, to, &mut out),
                        }
                    }
                    s = e + 1;
                }
            }
            i = k + 1;
        }
        out
    }

    /// `from`, `to`: the heading in which the track comes off a pad on to the run's first node (over
    /// the pad's edge, or out of a pin's fence), and the one in which it goes on from the last. The run
    /// turns less than 90 degrees there, and where it can it runs straight on at the pad and bends at its
    /// other end: between two pads that face each other a row apart, in the middle.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn smooth_run(&self, net: u16, l: usize, run: &[(u8, u16, u16)], t_min: crate::field::Dist, t_nom: crate::field::Dist, from: Option<(i32, i32)>, to: Option<(i32, i32)>, out: &mut Vec<(u8, u16, u16)>) {
        let m = run.len() - 1;
        // per node: is it too tight for the net's full width, and does another net's cost term price it
        // (a pin's lane)? `line_necks` counts the same
        let costs: Vec<u32> = run
            .iter()
            .map(|&(_, y, x)| {
                let i = self.f.idx(l, y as usize, x as usize);
                let c = &self.f.cells[i];
                (t_nom > t_min && (sdist(c, net) < t_nom || ddist(c, net) < t_nom)) as u32 + (self.costs.step_price(&self.f, i, net, 1) > 0) as u32
            })
            .collect();
        // prefix sums, to ask how much of that a stretch i..=j has at once
        let mut bad = vec![0u32; m + 2];
        for i in 0..=m {
            bad[i + 1] = bad[i] + costs[i];
        }
        let sgn = |v: i32| v.signum();
        // one hop from node i to node j: straight, or an elbow tried both ways; the legs as (dx, dy, steps)
        // (`straight`: only a hop that begins in the heading `from`)
        let connect = |i: usize, j: usize, prev: Option<(i32, i32)>, straight: bool| -> Option<Vec<(i32, i32, i32)>> {
            let (ax, ay) = (run[i].2 as i32, run[i].1 as i32);
            let (bx, by) = (run[j].2 as i32, run[j].1 as i32);
            let (dx, dy) = (bx - ax, by - ay);
            let (adx, ady) = (dx.abs(), dy.abs());
            let nd = adx.min(ady);
            let no = (adx - ady).abs();
            let dd = (sgn(dx), sgn(dy));
            let od = if adx > ady { (sgn(dx), 0) } else { (0, sgn(dy)) };
            // a stretch is never redrawn with more of it too tight for the full width than it had (its
            // first node stays where it is, so it is not counted)
            let had = bad[j + 1] - bad[i + 1];
            let mut opts: Vec<Vec<(i32, i32, i32)>> = Vec::new();
            if nd == 0 || no == 0 {
                opts.push(vec![if nd == 0 { (od.0, od.1, no) } else { (dd.0, dd.1, nd) }]);
            } else {
                opts.push(vec![(od.0, od.1, no), (dd.0, dd.1, nd)]);
                opts.push(vec![(dd.0, dd.1, nd), (od.0, od.1, no)]);
                // the headings the hop should begin and end in: off a pad and on to one, straight
                let (begin, end) = (if i == 0 { from } else { None }, if j == m { to } else { None });
                if let (Some(b), Some(e)) = (begin, end) {
                    // straight at both pads, the other leg between: its two halves as even as they come
                    if b == e && b == od && no >= 2 {
                        opts.push(vec![(od.0, od.1, no - no / 2), (dd.0, dd.1, nd), (od.0, od.1, no / 2)]);
                    } else if b == e && b == dd && nd >= 2 {
                        opts.push(vec![(dd.0, dd.1, nd - nd / 2), (od.0, od.1, no), (dd.0, dd.1, nd / 2)]);
                    }
                }
                // first the one that is straight at a pad; then the one that carries on the way the
                // track was going
                let score = |legs: &Vec<(i32, i32, i32)>| {
                    let (f, g) = ((legs[0].0, legs[0].1), (legs[legs.len() - 1].0, legs[legs.len() - 1].1));
                    2 * (begin == Some(f)) as i32 + 2 * (end == Some(g)) as i32 + (prev == Some(f)) as i32
                };
                opts.sort_by_key(|legs| -score(legs));
            }
            'opt: for legs in opts {
                let f = (legs[0].0, legs[0].1);
                if let Some(p) = prev {
                    // no bend sharper than 90 degrees where this hop joins the last
                    if p.0 * f.0 + p.1 * f.1 < 0 {
                        continue;
                    }
                }
                // nor one of 90 degrees at a pad's edge
                if let (0, Some(p)) = (i, from) {
                    if p.0 * f.0 + p.1 * f.1 <= 0 || (straight && p != f) {
                        continue;
                    }
                }
                if let (true, Some(p)) = (j == m, to) {
                    let g = legs[legs.len() - 1];
                    if p.0 * g.0 + p.1 * g.1 <= 0 {
                        continue;
                    }
                }
                let (mut x, mut y) = (ax, ay);
                let mut necks = 0;
                for &(sx, sy, n) in &legs {
                    match self.line_necks(net, l, x, y, sx, sy, n, t_min, t_nom) {
                        Some(k) => necks += k,
                        None => continue 'opt,
                    }
                    x += sx * n;
                    y += sy * n;
                }
                if necks > had {
                    continue;
                }
                return Some(legs);
            }
            None
        };
        out.push(run[0]);
        let mut i = 0;
        let mut prev: Option<(i32, i32)> = None;
        while i < m {
            // the farthest node reachable by one hop: try the end, then halve; then feel forward again.
            // Off a pad, first the farthest that a hop reaches which begins straight on the pad's line:
            // a hop that bends at the pad reaches further, and is not wanted for that
            let reach = |straight: bool| -> Option<(usize, Vec<(i32, i32, i32)>)> {
                let mut j = m;
                let mut fail = m + 1;
                let mut legs = None;
                loop {
                    if let Some(v) = connect(i, j, prev, straight) {
                        legs = Some(v);
                        break;
                    }
                    if j == i + 1 {
                        break;
                    }
                    fail = j;
                    j = i + ((j - i) / 2).max(1);
                }
                let mut legs = legs?;
                while fail > j + 1 && fail <= m {
                    let mid = (j + fail) / 2;
                    match connect(i, mid, prev, straight) {
                        Some(v) => {
                            j = mid;
                            legs = v;
                        }
                        None => fail = mid,
                    }
                }
                Some((j, legs))
            };
            let hop = if i == 0 && from.is_some() { reach(true).or_else(|| reach(false)) } else { reach(false) };
            let (j, legs) = hop.unwrap_or_else(|| {
                // the next node as the search had it (a bend the rule above refused, kept as found)
                let (ax, ay) = (run[i].2 as i32, run[i].1 as i32);
                let (bx, by) = (run[i + 1].2 as i32, run[i + 1].1 as i32);
                (i + 1, vec![(bx - ax, by - ay, 1)])
            });
            let (mut x, mut y) = (run[i].2 as i32, run[i].1 as i32);
            for &(sx, sy, n) in &legs {
                for _ in 0..n {
                    x += sx;
                    y += sy;
                    out.push((l as u8, y as u16, x as u16));
                }
                prev = Some((sx, sy));
            }
            i = j;
        }
    }
}
