//! Laying copper: a found path into tracks and vias, in the field and on its job.

use super::ends::{Pt, NO_NODE};
use super::{Access, Router, Track};
use crate::field::{ddist, sdist, Dist, DynObj, Kind, ALL_LAYERS, TICK};
use crate::geom::hypot;
use std::time::Instant;

impl Router {
    /// The widest a narrowed track may be drawn, from the width it has up to `most`: the most at which
    /// every piece of it keeps its clearance from fixed and routed copper, to a hundredth of a
    /// millimetre.
    fn widest(&self, net: u16, t: &Track, most: f64) -> f64 {
        let clear = |w: f64| t.pts.windows(2).all(|s| self.piece_clear(s[0], s[1], w / 2.0, net, t.layer as usize));
        if clear(most) {
            return most;
        }
        let (mut lo, mut hi) = (t.width, most);
        for _ in 0..8 {
            let mid = (lo + hi) / 2.0;
            if clear(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        ((lo * 100.0 + 1e-9).floor() / 100.0).max(t.width)
    }

    /// Tracks of job j into the field.
    pub(crate) fn stand(&mut self, j: usize, tracks: &[Track]) {
        let net = self.jobs[j].net;
        let more = self.more_clr(net);
        for t in tracks {
            for s in t.pts.windows(2) {
                if hypot(s[0].0 - s[1].0, s[0].1 - s[1].1) < 1e-7 {
                    continue;
                }
                let id = self.f.add_obj(DynObj { kind: Kind::Track, net, job: j as u32, locked: false, layer: t.layer, ax: s[0].0, ay: s[0].1, bx: s[1].0, by: s[1].1, r: t.width / 2.0 + more, alive: true });
                self.jobs[j].objs.push(id);
            }
        }
    }

    /// Lay a found path as copper for job j. `from_pad` / `to_pad`: the pads its ends stand on.
    pub(crate) fn commit(&mut self, j: usize, path: &[(u8, u16, u16)], from_pad: Option<usize>, to_pad: Option<usize>) {
        // a net that asks more clearance than the board keeps others that much further from its copper
        let more = self.more_clr(self.jobs[j].net);
        let tc = Instant::now();
        let net = self.jobs[j].net;
        let n = &self.sc.nets[net as usize];
        let (w_nom, w_min) = (n.width, self.w_min(net));
        let (t_min, t_nom) = (self.t_min(net), self.t_nom(net));
        let wide = t_nom > t_min;
        let mut tracks: Vec<Track> = Vec::new();
        let mut vias: Vec<(u16, u16)> = Vec::new();
        let mut moved: Vec<((u8, u16, u16), (f64, f64))> = Vec::new();
        let mut rough = false;
        let mut ends = (0u32, 0u32);

        // the last node that carries track: a plane drop ends one node later, on the plane's layer (which
        // carries other nets' tracks too when the plane is the net's host's: `Cfg::drops`)
        let m = path.len();
        let lands = m >= 2 && n.plane == Some(path[m - 1].0 as usize) && path[m - 2].0 != path[m - 1].0 && self.jobs[j].pad.is_some();
        let end_k = if lands { m - 2 } else { (0..m).rev().find(|&k| self.costs.step[path[k].0 as usize][0] != 0).unwrap_or(m - 1) };
        // split into runs on one layer
        let mut i = 0;
        while i < path.len() {
            let l = path[i].0;
            let mut k = i;
            while k + 1 < path.len() && path[k + 1].0 == l {
                k += 1;
            }
            let run = &path[i..=k];
            if self.costs.step[l as usize][0] != 0 {
                // per node: does the full width fit here?
                let fits: Vec<bool> = run
                    .iter()
                    .map(|&(l, y, x)| {
                        if !wide {
                            return true;
                        }
                        let c = &self.f.cells[self.f.idx(l as usize, y as usize, x as usize)];
                        sdist(c, net) >= t_nom && ddist(c, net) >= t_nom
                    })
                    .collect();
                // a narrowed stretch is at least a few nodes long: a one-node neck where a branch leaves
                // a track is a stub so short that KiCad's dangling-end test misreads it
                let mut fits = fits;
                if wide {
                    let raw = fits.clone();
                    for (i, &ok) in raw.iter().enumerate() {
                        if !ok {
                            for k in i.saturating_sub(3)..(i + 4).min(raw.len()) {
                                fits[k] = false;
                            }
                        }
                    }
                }
                let mut pts: Vec<Pt> = run.iter().zip(&fits).enumerate().map(|(q, (&(_, y, x), &ok))| ((self.f.wx(x as usize), self.f.wy(y as usize)), ok, q)).collect();
                // the piece from a pad's centre to the first node: along the pad's fence when the node is
                // a fence's end, else straight; at full width only where that is known to be clear
                let stub = |p: usize, node: (u8, u16, u16), at: (f64, f64), fits: bool| -> Vec<((f64, f64), bool)> {
                    let c = (self.sc.pads[p].x, self.sc.pads[p].y);
                    if let Some(Access { via: Some(e0), wide, .. }) = self.access_at(p, node.2, node.1) {
                        return vec![(c, wide), (e0, wide)];
                    }
                    let d = hypot(c.0 - at.0, c.1 - at.1);
                    if d <= 1e-6 {
                        return vec![];
                    }
                    // a pad that stands in the edge's or another fixed thing's keep-clear zone (a switch at
                    // the board edge): the track ends at the node, which is in the pad's copper already
                    if !self.statics.track_clear(c, at, w_min / 2.0, self.net_clr(net), 1 << node.0, net) {
                        return vec![];
                    }
                    let m = (d / TICK).ceil() as Dist;
                    let cell = &self.f.cells[self.f.idx(node.0 as usize, node.1 as usize, node.2 as usize)];
                    // nor does the piece go on to the centre when that takes it too near another net's
                    // track: the node was clear, the centre is off the grid and up to 0.07 mm nearer
                    if ddist(cell, net) < (t_min as f64 - d.min(0.02) / TICK) as Dist + m {
                        return vec![];
                    }
                    let room = sdist(cell, net) >= t_nom.saturating_add(m) && ddist(cell, net) >= t_nom.saturating_add(m);
                    vec![(c, !wide || (fits && room))]
                };
                // each end on its true line where that is clear (`ends`), else as found: the stub, then
                // the nodes
                let grid: Vec<(f64, f64)> = pts.iter().map(|p| p.0).collect();
                let mut gone: Vec<(usize, (f64, f64))> = Vec::new();
                if i == 0 && from_pad.is_none() {
                    // the path starts on copper the net has already (an island of old copper): its first
                    // piece runs to that copper's own line where that is clear. (Where it is not, the
                    // track starts at the node, which stands in that copper: a node is within half a
                    // pitch of a track's middle and a pitch of a via's.)
                    if let Some(at) = self.copper_at(j, run[0]) {
                        match self.true_end(net, l as usize, &pts, &[(at, pts[0].1)]) {
                            Some((drawn, g)) => {
                                pts = drawn;
                                gone.extend(g);
                            }
                            None => rough = true,
                        }
                    }
                }
                if i == 0 {
                    if let Some(p) = from_pad {
                        let st = stub(p, run[0], pts[0].0, pts[0].1);
                        match self.true_end(net, l as usize, &pts, &st) {
                            Some((drawn, g)) => {
                                pts = drawn;
                                gone.extend(g);
                                ends.0 += 1;
                            }
                            None => {
                                rough |= !st.is_empty();
                                ends.1 += (!st.is_empty() && pts.len() > 1) as u32;
                                for (q, v) in st.into_iter().enumerate() {
                                    pts.insert(q, (v.0, v.1, NO_NODE));
                                }
                            }
                        }
                    }
                }
                if k == end_k {
                    // the far end: a pad's centre, or the track of the net that stands at the last node
                    let last = pts[pts.len() - 1];
                    let st = match to_pad {
                        Some(p) => stub(p, run[run.len() - 1], last.0, last.1),
                        None => self.copper_at(j, run[run.len() - 1]).map(|at| vec![(at, last.1)]).unwrap_or_default(),
                    };
                    let back: Vec<Pt> = pts.iter().rev().copied().collect();
                    match self.true_end(net, l as usize, &back, &st) {
                        Some((mut drawn, g)) => {
                            drawn.reverse();
                            pts = drawn;
                            gone.extend(g);
                            ends.0 += to_pad.is_some() as u32;
                        }
                        None => {
                            rough |= !st.is_empty();
                            ends.1 += (to_pad.is_some() && !st.is_empty() && pts.len() > 1) as u32;
                            if to_pad.is_some() {
                                for v in st.into_iter().rev() {
                                    pts.push((v.0, v.1, NO_NODE));
                                }
                            }
                        }
                    }
                }
                // the nodes whose copper is not on them now, for the paths that will end there
                for &(at, _, q) in &pts {
                    if q != NO_NODE && hypot(at.0 - grid[q].0, at.1 - grid[q].1) > 1e-9 {
                        moved.push((run[q], at));
                    }
                }
                for (q, at) in gone {
                    moved.push((run[q], at));
                }
                // steps of one width into one polyline, straight runs merged
                let mut cur: Option<Track> = None;
                for s in 0..pts.len().saturating_sub(1) {
                    let (a, b) = (pts[s].0, pts[s + 1].0);
                    let width = if pts[s].1 && pts[s + 1].1 { w_nom } else { w_min };
                    let fresh = match &cur {
                        Some(t) => (t.width - width).abs() > 1e-9,
                        None => true,
                    };
                    if fresh {
                        if let Some(t) = cur.take() {
                            tracks.push(t);
                        }
                        cur = Some(Track { layer: l, width, pts: vec![a, b], arcs: Vec::new() });
                    } else if let Some(t) = cur.as_mut() {
                        let m = t.pts.len();
                        let (p, q) = (t.pts[m - 2], t.pts[m - 1]);
                        let cross = (q.0 - p.0) * (b.1 - q.1) - (q.1 - p.1) * (b.0 - q.0);
                        let dot = (q.0 - p.0) * (b.0 - q.0) + (q.1 - p.1) * (b.1 - q.1);
                        if cross.abs() < 1e-9 && dot > 0.0 {
                            t.pts[m - 1] = b;
                        } else {
                            t.pts.push(b);
                        }
                    }
                }
                if let Some(t) = cur.take() {
                    tracks.push(t);
                }
            }
            if k + 1 < path.len() {
                let (ll, y, x) = path[k];
                let here = (x, y);
                // a drop that ends on a pad of its net lays no via there
                let on_pad = k == end_k && to_pad.map_or(false, |p| !self.sc.pads[p].tht && self.pad_layers(p).contains(&(ll as usize)));
                if !on_pad && !self.net_vias[net as usize].contains(&here) && !self.net_tht[net as usize].contains(&here) && !vias.contains(&here) {
                    vias.push(here);
                } else if let Some(&o) = self.via_owner.get(&(net, x, y)) {
                    // the layer change borrows a via another job of the net laid: this job stands or
                    // falls with that one, wherever on the path the via is
                    if o != j && !self.deps[o].contains(&j) {
                        self.deps[o].push(j);
                    }
                }
            }
            i = k + 1;
        }

        // a neck is as wide as there is room for: once the nets have found their ways (`redraw`), each
        // narrowed stretch is widened to the most that is clear of what stands beside it, measured
        // exactly, up to the net's own width. (While they are finding them a neck is drawn at its
        // narrowest: what it does not take, another net may need.)
        if wide && self.settled {
            for t in tracks.iter_mut().filter(|t| t.width < w_nom - 1e-9) {
                t.width = self.widest(net, t, w_nom);
            }
        }
        self.stand(j, &tracks);
        for &(x, y) in &vias {
            let (px, py) = (self.f.wx(x as usize), self.f.wy(y as usize));
            let small = self.via_is_small(net, x as usize, y as usize);
            let vr = if small { self.via_r_s } else { self.kind(net).r } + more;
            if small {
                self.jobs[j].small.push((x, y));
            }
            let id = self.f.add_obj(DynObj { kind: Kind::Via, net, job: j as u32, locked: false, layer: ALL_LAYERS, ax: px, ay: py, bx: px, by: py, r: vr, alive: true });
            self.jobs[j].objs.push(id);
            self.net_vias[net as usize].push((x, y));
            self.via_owner.insert((net, x, y), j);
            self.jobs[j].vias.push((x, y));
        }
        let tracks_narrow = tracks.iter().any(|t| t.width < w_nom - 1e-9);
        self.jobs[j].tracks.extend(tracks);
        self.jobs[j].moved.extend(moved);
        self.jobs[j].rough |= rough || tracks_narrow;
        self.jobs[j].ends = (self.jobs[j].ends.0 + ends.0, self.jobs[j].ends.1 + ends.1);
        self.jobs[j].paths.push((path.to_vec(), from_pad, to_pad));
        self.jobs[j].nodes += path.len();
        self.stats.lay_s += tc.elapsed().as_secs_f64();
    }
}
