//! Redrawing: once the rounds are over, copper that was drawn to fit a board that has since changed is
//! drawn again from its paths.
//!
//! How a path is drawn (`Router::commit`) depends on what stands beside it at the time: a stretch is
//! narrowed where the net's full width does not fit, and an end is laid as the search found it where its
//! true line is not clear (`ends`). During the rounds that is often another net's copper the path
//! crossed, which is then lifted and routed another way. So every job that was drawn with such a
//! concession is drawn once more, from the same paths, against the board as it stands at the end.
//!
//! A path that kept clear of a neighbour is shaped by where that neighbour stood, too: it steps aside
//! for copper that is routed another way a moment later, and nothing marks it. So the last time, once
//! every net has its way, every job is pulled tight and drawn again, not only the marked ones.
//!
//! The new drawing is kept only when it is clear. A path's nodes were clear when the search found them,
//! not for ever: a via laid since may stand at its clearance from an end that was drawn on its true line,
//! a little off the nodes, and nearer than that to the nodes themselves. Drawn again, such an end may go
//! back on its nodes. Then the job keeps the drawing it had.

use super::Router;
use crate::field::Kind;

impl Router {
    /// Draw again every job that has a narrowed stretch or an end laid as found, and the `last` time
    /// every job. Its vias stay where they are; its tracks are lifted and laid from its paths, in the
    /// order they were found.
    pub(crate) fn redraw(&mut self, last: bool) {
        // (from the last time on a neck is drawn as wide as there is room for: `commit`)
        self.settled |= last;
        for j in 0..self.jobs.len() {
            if !(self.jobs[j].rough || last) || self.jobs[j].paths.is_empty() {
                continue;
            }
            let paths = std::mem::take(&mut self.jobs[j].paths);
            let was = (self.jobs[j].tracks.clone(), self.jobs[j].moved.clone(), self.jobs[j].nodes, self.jobs[j].ends);
            let (tracks, vias): (Vec<u32>, Vec<u32>) = std::mem::take(&mut self.jobs[j].objs).into_iter().partition(|&id| self.f.objs[id as usize].kind == Kind::Track);
            self.f.set_locked(&tracks, false);
            self.f.remove_objs(&tracks);
            let job = &mut self.jobs[j];
            job.objs = vias;
            job.tracks.clear();
            job.moved.clear();
            job.nodes = 0;
            job.rough = false;
            job.ends = (0, 0);
            // the nodes where one path of the job ends on another: they stay where they are
            let joints: Vec<super::Node> = paths.iter().flat_map(|p| [p.0[0], p.0[p.0.len() - 1]]).collect();
            for (path, from, to) in &paths {
                // pulled tight against the board as it stands (a path kept as it was found through
                // another net's copper has its wiggles taken out once that copper has gone), stretch by
                // stretch between the joints
                let mut drawn: Vec<super::Node> = Vec::with_capacity(path.len());
                let mut a = 0;
                for b in 1..path.len() {
                    if b + 1 == path.len() || joints.contains(&path[b]) {
                        let piece = self.shape(self.jobs[j].net, path[a..=b].to_vec());
                        drawn.extend_from_slice(&piece[if a == 0 { 0 } else { 1 }..]);
                        a = b;
                    }
                }
                if drawn.is_empty() {
                    drawn = path.clone();
                }
                self.commit(j, &drawn, *from, *to);
            }
            // (the rule itself, less the half micron KiCad's own check allows: what the field let stand)
            let net = self.jobs[j].net;
            let clear = self.jobs[j].tracks.iter().all(|t| t.pts.windows(2).all(|s| self.piece_off(s[0], s[1], t.width / 2.0, net, t.layer as usize)));
            if !clear {
                let (tracks, vias): (Vec<u32>, Vec<u32>) = std::mem::take(&mut self.jobs[j].objs).into_iter().partition(|&id| self.f.objs[id as usize].kind == Kind::Track);
                self.f.remove_objs(&tracks);
                self.jobs[j].objs = vias;
                self.stand(j, &was.0);
                let job = &mut self.jobs[j];
                (job.tracks, job.moved, job.nodes, job.ends) = was;
                job.paths = paths;
            }
            if self.jobs[j].locked {
                let objs = self.jobs[j].objs.clone();
                self.f.set_locked(&objs, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::field::{DynObj, Kind, NO_JOB};
    use crate::route::{route, Cfg};
    use crate::scene::Scene;

    #[test]
    fn copper_narrowed_for_what_has_since_gone_is_drawn_at_its_width_again() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scenes/true_ends.json")).unwrap();
        let mut sc = Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap();
        // (the fab's narrowest track is half the nets' own, so there is something to narrow to)
        sc.rules.floors.track = Some(0.1);
        let mut r = route(sc, &Cfg { two_ways: false, radius_mm: 0.0, ..Cfg::default() }).unwrap().router;
        let j = (0..r.jobs.len()).find(|&j| r.sc.nets[r.jobs[j].net as usize].name == "C").unwrap();
        let drawn = |r: &crate::route::Router| -> Vec<(f64, Vec<(f64, f64)>)> { r.jobs[j].tracks.iter().map(|t| (t.width, t.pts.clone())).collect() };
        let before = drawn(&r);
        assert!(!r.jobs[j].rough && before.iter().all(|t| t.0 == 0.2), "{:?}", before);
        // a piece of another net's copper beside the long run of C, near enough that only a neck passes it
        let (_, pts) = before.iter().max_by(|a, b| a.1.len().cmp(&b.1.len())).unwrap();
        let (a, b) = pts.windows(2).max_by(|p, q| (p[0].0 - p[1].0).hypot(p[0].1 - p[1].1).partial_cmp(&(q[0].0 - q[1].0).hypot(q[0].1 - q[1].1)).unwrap()).map(|w| (w[0], w[1])).unwrap();
        let len = (b.0 - a.0).hypot(b.1 - a.1);
        let (mid, across) = (((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0), (-(b.1 - a.1) / len, (b.0 - a.0) / len));
        let d = 0.05 + 0.05 + r.sc.rules.clearance + 0.04;
        let other = r.sc.nets.iter().position(|n| n.name == "A").unwrap() as u16;
        let at = (mid.0 + across.0 * d, mid.1 + across.1 * d);
        let block = r.f.add_obj(DynObj { kind: Kind::Track, net: other, job: NO_JOB, locked: false, layer: 0, ax: at.0, ay: at.1, bx: at.0, by: at.1, r: 0.05, alive: true });
        r.jobs[j].rough = true;
        r.redraw(true);
        assert!(r.jobs[j].rough && drawn(&r).iter().any(|t| t.0 < 0.2), "{:?}", drawn(&r));
        // the piece is taken away: drawn again, C is as it was before
        r.f.remove_objs(&[block]);
        r.redraw(true);
        assert!(!r.jobs[j].rough);
        assert_eq!(drawn(&r), before);
    }
    /// An end drawn on its pad's own line stands a little off the nodes its path was found on. Copper
    /// laid since may be at its clearance from that line and nearer than that to the nodes. Drawn again,
    /// the end would go back on its nodes (its own line is no longer clear by the margin a new drawing
    /// asks) and into that clearance: the job keeps the drawing it had.
    #[test]
    fn a_drawing_that_would_not_be_clear_is_not_taken() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scenes/true_ends.json")).unwrap();
        let sc = Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap();
        let mut r = route(sc, &Cfg { two_ways: false, radius_mm: 0.0, ..Cfg::default() }).unwrap().router;
        // a straight piece that runs beside its path's nodes, not on them: (job, its ends, how far the
        // nodes are off it and to which side)
        let mut found = None;
        for (j, job) in r.jobs.iter().enumerate() {
            let nodes: Vec<(u8, f64, f64)> = job.paths.iter().flat_map(|p| p.0.iter().map(|&(l, y, x)| (l, r.f.wx(x as usize), r.f.wy(y as usize)))).collect();
            for t in &job.tracks {
                for w in t.pts.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let len = (b.0 - a.0).hypot(b.1 - a.1);
                    let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                    let Some(n) = nodes.iter().filter(|n| n.0 == t.layer).min_by(|p, q| (p.1 - mid.0).hypot(p.2 - mid.1).partial_cmp(&(q.1 - mid.0).hypot(q.2 - mid.1)).unwrap()) else { continue };
                    let off = ((b.0 - a.0) * (n.2 - a.1) - (b.1 - a.1) * (n.1 - a.0)) / len;
                    if len > 1.0 && off.abs() > 0.01 && off.abs() < 0.06 && found.is_none() {
                        found = Some((j, t.layer, t.width, a, b, off));
                    }
                }
            }
        }
        let (j, layer, width, a, b, off) = found.expect("the fixture has an end drawn off its nodes");
        let before: Vec<Vec<(f64, f64)>> = r.jobs[j].tracks.iter().map(|t| t.pts.clone()).collect();
        // another net's track along the piece, on the nodes' side: its clearance and 2 microns from it
        let len = (b.0 - a.0).hypot(b.1 - a.1);
        let across = (-(b.1 - a.1) / len * off.signum(), (b.0 - a.0) / len * off.signum());
        let d = width / 2.0 + 0.05 + r.sc.rules.clearance + 0.002;
        let other = (1..r.sc.nets.len()).find(|&n| n as u16 != r.jobs[j].net).unwrap() as u16;
        r.f.add_obj(DynObj { kind: Kind::Track, net: other, job: NO_JOB, locked: false, layer, ax: a.0 + across.0 * d, ay: a.1 + across.1 * d, bx: b.0 + across.0 * d, by: b.1 + across.1 * d, r: 0.05, alive: true });
        r.jobs[j].rough = true;
        r.redraw(true);
        assert_eq!(r.jobs[j].tracks.iter().map(|t| t.pts.clone()).collect::<Vec<_>>(), before);
    }
}
