//! Mending: after the rounds, a net that is still open is laid through whatever stands in its way, what
//! it went through is routed again round it, and the move is kept only when fewer pads are open after it
//! than before. Else everything it touched goes back exactly as it was.

use super::{Router, Track};
use crate::field::DynObj;
use std::collections::HashMap;

/// A job's copper, to put back.
struct Kept {
    objs: Vec<DynObj>,
    tracks: Vec<Track>,
    moved: Vec<((u8, u16, u16), (f64, f64))>,
    paths: Vec<(Vec<(u8, u16, u16)>, Option<usize>, Option<usize>)>,
    rough: bool,
    ends: (u32, u32),
    vias: Vec<(u16, u16)>,
    small: Vec<(u16, u16)>,
    nodes: usize,
    open: Vec<usize>,
    done: bool,
    loose: Vec<(f64, f64)>,
    pour_pads: Vec<u32>,
    pour_ends: Vec<(u32, u32)>,
    walled: Vec<usize>,
    walled_loose: usize,
    trunk: Option<super::duo::Trunk>,
}

/// The board's copper as it stands: every job's, and what the router keeps about it.
struct Board {
    jobs: Vec<Kept>,
    deps: Vec<Vec<usize>>,
    net_vias: Vec<Vec<(u16, u16)>>,
    via_owner: HashMap<(u16, u16, u16), usize>,
}

impl Router {
    fn keep(&self) -> Board {
        let jobs = self
            .jobs
            .iter()
            .map(|j| Kept {
                objs: j.objs.iter().map(|&id| self.f.objs[id as usize].clone()).collect(),
                tracks: j.tracks.clone(),
                moved: j.moved.clone(),
                paths: j.paths.clone(),
                rough: j.rough,
                ends: j.ends,
                vias: j.vias.clone(),
                small: j.small.clone(),
                nodes: j.nodes,
                open: j.open.clone(),
                done: j.done,
                loose: j.loose.clone(),
                pour_pads: j.pour_pads.clone(),
                pour_ends: j.pour_ends.clone(),
                walled: j.walled.clone(),
                walled_loose: j.walled_loose,
                trunk: j.trunk.clone(),
            })
            .collect();
        Board { jobs, deps: self.deps.clone(), net_vias: self.net_vias.clone(), via_owner: self.via_owner.clone() }
    }

    /// Every job in `touched` back as the board kept it.
    fn put_back(&mut self, was: Board, touched: &[usize]) {
        for &k in touched {
            let objs = std::mem::take(&mut self.jobs[k].objs);
            self.f.remove_objs(&objs);
        }
        self.pairs.retain(|p| !touched.contains(&p.0) && !touched.contains(&p.1));
        for &k in touched {
            let s = &was.jobs[k];
            let ids: Vec<u32> = s.objs.iter().map(|o| self.f.add_obj(o.clone())).collect();
            let job = &mut self.jobs[k];
            job.objs = ids;
            job.tracks = s.tracks.clone();
            job.moved = s.moved.clone();
            job.paths = s.paths.clone();
            job.rough = s.rough;
            job.ends = s.ends;
            job.vias = s.vias.clone();
            job.small = s.small.clone();
            job.nodes = s.nodes;
            job.open = s.open.clone();
            job.done = s.done;
            job.loose = s.loose.clone();
            job.pour_pads = s.pour_pads.clone();
            job.pour_ends = s.pour_ends.clone();
            job.walled = s.walled.clone();
            job.walled_loose = s.walled_loose;
            job.trunk = s.trunk.clone();
            job.clash_cells.clear();
        }
        self.deps = was.deps;
        self.net_vias = was.net_vias;
        self.via_owner = was.via_owner;
    }

    fn open_pads(&self) -> usize {
        self.jobs.iter().map(|j| j.open.len()).sum()
    }

    /// Lift job j and what leans on it; every job lifted is added to `touched` and returned.
    fn lift_noting(&mut self, j: usize, touched: &mut Vec<usize>) -> Vec<usize> {
        let up = self.lift(j);
        for &v in &up {
            if !touched.contains(&v) {
                touched.push(v);
            }
        }
        up
    }

    /// Job j laid whole: through whatever stands in its way to learn what does, those lifted, j laid
    /// again in the room they left (and drawn tight, which a path through other copper is not), then
    /// they round it; one of them that then finds no way is laid the same way, `depth` deep, but never
    /// through a job laid on the way here (`fixed`). False when that cannot be done; the copper is then
    /// left as it fell and the caller puts the board back.
    fn lay_whole(&mut self, j: usize, depth: u32, fixed: &mut Vec<usize>, touched: &mut Vec<usize>) -> bool {
        if self.out_of_time() || self.jobs[j].locked {
            return false;
        }
        let mut again = self.lift_noting(j, touched);
        self.strict = false;
        let pressure = std::mem::replace(&mut self.pressure, 1.0);
        self.route_job(j);
        self.strict = true;
        self.pressure = pressure;
        let victims: Vec<usize> = self.pairs.iter().filter_map(|p| if p.0 == j { Some(p.1) } else if p.1 == j { Some(p.0) } else { None }).collect();
        // (whose copper it went through: should this way fail, the next try goes through another's: `mend`)
        for &v in &victims {
            if !self.tried.contains(&self.jobs[v].net) {
                self.tried.push(self.jobs[v].net);
            }
        }
        if self.for_room(j) || victims.iter().any(|&v| self.jobs[v].locked || fixed.contains(&v)) {
            return false;
        }
        for v in victims {
            for w in self.lift_noting(v, touched) {
                if !again.contains(&w) {
                    again.push(w);
                }
            }
        }
        for w in self.lift_noting(j, touched) {
            if !again.contains(&w) {
                again.push(w);
            }
        }
        again.retain(|&w| w != j);
        again.sort();
        self.route_job(j);
        if self.for_room(j) || self.clashing(j) {
            return false;
        }
        fixed.push(j);
        for &w in &again {
            self.route_job(w);
        }
        for w in again {
            if self.for_room(w) && (depth == 0 || !self.lay_whole(w, depth - 1, fixed, touched)) {
                return false;
            }
        }
        true
    }

    /// What stands in the way of each job still open for want of room (`Job::blockers`): the job is laid
    /// once more, free to run through other nets' copper, the nets it runs through and where are noted,
    /// and the board is put back as it was. Run last, on the board as it will be handed over; the first
    /// dozen open jobs are asked, as the report names a dozen.
    pub(crate) fn blame(&mut self) {
        let open: Vec<usize> = (0..self.jobs.len()).filter(|&j| self.for_room(j) && !self.jobs[j].locked).take(12).collect();
        for j in open {
            if self.out_of_time() {
                return;
            }
            let was = self.keep();
            let mut touched: Vec<usize> = Vec::new();
            self.lift_noting(j, &mut touched);
            self.strict = false;
            let pressure = std::mem::replace(&mut self.pressure, 1.0);
            self.route_job(j);
            self.strict = true;
            self.pressure = pressure;
            let net = self.jobs[j].net;
            let reach = self.t_min(net) as f64 * crate::field::TICK + 1e-6;
            let per = self.f.w * self.f.h;
            let mut who: Vec<(u16, (f64, f64))> = Vec::new();
            for &ci in &self.jobs[j].clash_cells {
                let (l, y, x) = (ci / per, (ci % per) / self.f.w, ci % self.f.w);
                let mut hit: Vec<(u32, u16)> = Vec::new();
                self.f.foreign_near(l, y, x, net, reach, &mut hit);
                for (_, n) in hit {
                    if !who.iter().any(|w| w.0 == n) {
                        who.push((n, (self.f.wx(x), self.f.wy(y))));
                    }
                }
            }
            self.put_back(was, &touched);
            self.jobs[j].blockers = who;
        }
    }

    /// The nets the rounds left open, one at a time, the one with the fewest pads open first. The board
    /// is whole and nothing clashes when this starts, and the same when it ends.
    pub(crate) fn mend(&mut self) {
        let n = self.jobs.len();
        for _ in 0..3 {
            // (a job open only where the board walls a pad in cannot be mended)
            let mut todo: Vec<usize> = (0..n).filter(|&j| self.for_room(j) && !self.jobs[j].locked).collect();
            todo.sort_by_key(|&j| (self.jobs[j].open.len(), j));
            let mut better = false;
            for j in todo {
                if self.out_of_time() {
                    return;
                }
                if !self.for_room(j) {
                    continue;
                }
                // a way through one neighbour's copper that does not work out is not the only one: the
                // next try keeps off the nets the last one went through, and goes through another's (a
                // rail that took the room in front of a row of pads, and that no net has clashed with)
                for _ in 0..3 {
                    let before = self.open_pads();
                    let was = self.keep();
                    let (mut touched, mut fixed): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
                    self.tried.clear();
                    let ok = self.lay_whole(j, 2, &mut fixed, &mut touched) && !touched.iter().any(|&k| self.clashing(k)) && self.open_pads() < before;
                    if self.cfg.verbose {
                        eprintln!("  mend {}: {} jobs touched, {} pads open before, {} after: {}", self.sc.nets[self.jobs[j].net as usize].name, touched.len(), before, self.open_pads(), if ok { "kept" } else { "put back" });
                    }
                    if ok {
                        better = true;
                        break;
                    }
                    self.put_back(was, &touched);
                    let more: Vec<u16> = std::mem::take(&mut self.tried).into_iter().filter(|n| !self.shun.contains(n)).collect();
                    if more.is_empty() || self.out_of_time() {
                        break;
                    }
                    self.shun.extend(more);
                }
                self.shun.clear();
            }
            if !better {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::route::{route, Cfg};
    use crate::scene::Scene;

    #[test]
    fn what_is_put_back_is_the_board_as_it_was() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scenes/every_field.json")).unwrap();
        let mut r = route(Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap(), &Cfg { two_ways: false, ..Cfg::default() }).unwrap().router;
        let sum = |r: &crate::route::Router| {
            let copper: Vec<(usize, usize, usize, usize)> = r.jobs.iter().map(|j| (j.objs.len(), j.tracks.len(), j.vias.len(), j.open.len())).collect();
            let alive = r.f.objs.iter().filter(|o| o.alive).count();
            let cells: u64 = r.f.cells.iter().map(|c| c.dd1 as u64 + c.dd2 as u64).sum();
            (copper, alive, cells, r.net_vias.clone(), r.deps.clone())
        };
        let before = sum(&r);
        let was = r.keep();
        // a job with copper and one that leans on another, lifted and put back
        let mut touched = Vec::new();
        for j in (0..r.jobs.len()).filter(|&j| !r.jobs[j].objs.is_empty()).take(3).collect::<Vec<_>>() {
            r.lift_noting(j, &mut touched);
        }
        assert!(!touched.is_empty() && sum(&r) != before);
        r.put_back(was, &touched);
        assert!(sum(&r) == before, "the copper, the field and what the router keeps about them are as they were");
    }
}
