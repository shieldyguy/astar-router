//! The negotiation: everything routed once, then what clashes routed again at a rising price.

use std::collections::HashMap;
use super::Router;
use crate::field::{ddist, TICK};
use crate::passes::{self, Pass, Step};
use std::sync::atomic::Ordering;
use std::time::Instant;

pub const PASS: Pass = Pass {
    name: "route",
    about: "every net without a plane becomes a job; every job is routed once, then what clashes is routed again at a rising price until nothing clashes",
    step: Step::Route(route),
};

fn route(r: &mut Router) {
    r.make_net_jobs();
    r.make_relief_jobs();
    r.run();
}

impl Router {
    /// Has the search done more work than the caller allows it?
    pub(crate) fn over_limit(&self) -> bool {
        match &self.limit {
            Some(l) => self.search.pops > l.load(std::sync::atomic::Ordering::Relaxed),
            None => false,
        }
    }

    pub(crate) fn out_of_time(&self) -> bool {
        if let Some(p) = &self.progress {
            p.store(self.search.pops, Ordering::Relaxed);
        }
        self.over_limit() || (self.cfg.budget_s > 0.0 && self.t0.elapsed().as_secs_f64() > self.cfg.budget_s)
    }

    pub(crate) fn route_job(&mut self, j: usize) {
        self.stats.routings += 1;
        let kind = self.jobs[j].kind;
        self.clock.enter(passes::index(kind.pass).unwrap_or(passes::FIELD));
        (kind.route)(self, j);
        self.clock.leave();
    }

    /// Is job j open for want of room among the other nets, or of search? (Not only at pads the board
    /// itself leaves no way to: `Job::walled`.) Routing it again, or the board another way, may join
    /// such a job; nothing joins a walled pad.
    pub(crate) fn for_room(&self, j: usize) -> bool {
        let job = &self.jobs[j];
        job.open.iter().any(|p| !job.walled.contains(p)) || job.loose.len() > job.walled_loose
    }

    pub(crate) fn open_for_room(&self) -> bool {
        (0..self.jobs.len()).any(|j| self.for_room(j))
    }

    /// A found path through the path passes (pull tight), each on its own clock, to the path to lay.
    pub(crate) fn shape(&mut self, net: u16, path: Vec<super::Node>) -> Vec<super::Node> {
        let mut path = path;
        for k in 0..self.shapers.len() {
            let (id, f) = self.shapers[k];
            self.clock.enter(id);
            path = f(self, net, &path);
            self.clock.leave();
        }
        path
    }

    /// Route everything once, each net taking the best path it can find, through another net's copper
    /// where that is much the shorter way. Then, round after round, route again every net that stands in
    /// another's copper, with such clashes dearer each round and dearer still where nets have clashed
    /// before, until none is left. Which of two nets gives way is settled by price: the one with the
    /// cheaper way round.
    pub fn run(&mut self) {
        let t = Instant::now();
        let n = self.jobs.len();
        for j in 0..n {
            if self.out_of_time() {
                break;
            }
            self.route_job(j);
        }
        let mut round = 0;
        // (how many rounds have had each set of jobs to route again)
        let mut seen: HashMap<Vec<usize>, u32> = HashMap::new();
        loop {
            let mut bad: Vec<usize> = Vec::new();
            for j in 0..n {
                let retry = !self.jobs[j].done || (self.for_room(j) && self.jobs[j].tries < 3);
                if self.clashing(j) || retry {
                    bad.push(j);
                }
                for k in 0..self.jobs[j].clash_cells.len() {
                    let i = self.jobs[j].clash_cells[k];
                    self.wear[i] = self.wear[i].saturating_add(1);
                }
            }
            // once nothing clashes: a net that leans on its pour's fill is routed again where other nets
            // have since cut that fill (looked at only now: the fill is costly to work out)
            if bad.is_empty() && round < self.cfg.rounds && !self.out_of_time() {
                // (a few times a net: two nets that keep cutting each other's fill would go on for ever,
                // and what is still cut when the rounds end is mended after them)
                bad = (0..n).filter(|&j| !self.jobs[j].locked && self.jobs[j].refills < 3 && self.pour_stale(j)).collect();
                for &j in &bad {
                    self.jobs[j].refills += 1;
                }
            }
            if self.cfg.verbose {
                let open = (0..n).filter(|&j| !self.jobs[j].open.is_empty()).count();
                eprintln!("round {}: {} jobs to route again ({} with a pad open), {:.2} s, {} expanded", round, bad.len(), open, t.elapsed().as_secs_f64(), self.search.pops);
            }
            // rounds that come back to the same jobs to route again are going round in a circle (nets
            // that each want another's place, whatever a clash costs by now; the same set every round, or
            // two sets turn about): the seventh time a set comes up, what clashes is left to the strict
            // pass and the mending
            let times = seen.entry(bad.clone()).or_insert(0);
            *times += 1;
            if bad.is_empty() || round >= self.cfg.rounds || *times >= 7 || self.out_of_time() {
                break;
            }
            round += 1;
            self.pressure *= self.cfg.pressure;
            let mut k = 0;
            while k < bad.len() {
                if self.out_of_time() {
                    break;
                }
                let j = bad[k];
                k += 1;
                let open = !self.jobs[j].done || self.for_room(j);
                if (!open && !self.clashing(j) && !self.pour_stale(j)) || self.jobs[j].locked {
                    continue;
                }
                if open {
                    self.jobs[j].tries += 1;
                } else {
                    self.stats.rips += 1;
                }
                for v in self.lift(j) {
                    if v != j && !bad[k..].contains(&v) {
                        bad.push(v);
                    }
                }
                self.route_job(j);
            }
        }
        self.stats.rounds = round;
        if self.over_limit() {
            // the caller has a board already and this one took too long: it is given up as it stands,
            // clashes and all, and must not be used
            self.abandoned = true;
            self.stats.route_s = t.elapsed().as_secs_f64();
            return;
        }
        // no clash may stay: of what still clashes, the smaller nets first, each routed again touching
        // nothing; a net that then finds no way is left open and said so
        self.strict = true;
        loop {
            // (a locked job stays where it is: the job that clashes with it is the one to move)
            let mut bad: Vec<usize> = (0..n).filter(|&j| self.clashing(j) && !self.jobs[j].locked).collect();
            if bad.is_empty() {
                break;
            }
            bad.sort_by_key(|&j| self.jobs[j].nodes);
            let mut again = self.lift(bad[0]);
            again.sort();
            for v in again {
                self.route_job(v);
            }
        }
        // what was kept as found through copper that has since moved is pulled tight and drawn again,
        // so that it takes no more room than it needs when the open nets are tried once more
        self.redraw(false);
        // what the rounds left open is tried once more, a net at a time (`mend`)
        self.mend();
        // every net has its way now: each is pulled tight and drawn again against the board as it stands
        // (what was narrowed, laid off its true line or stepped aside for copper that has since moved)
        self.redraw(true);
        // a net that leans on its pour stands on the fill as it is now: where other nets' copper has since
        // cut that fill, the net is routed again (touching nothing, so no other net moves); then the
        // reliefs that copper has starved get a track of their own
        for _ in 0..4 {
            let stale: Vec<usize> = (0..n).filter(|&j| !self.jobs[j].locked && self.pour_stale(j)).collect();
            for &j in &stale {
                let mut again = self.lift(j);
                again.sort();
                for v in again {
                    self.route_job(v);
                }
            }
            if self.relieve() == 0 && stale.is_empty() {
                break;
            }
        }
        // what is still open for want of room says what is in its way
        self.blame();
        self.stats.route_s = t.elapsed().as_secs_f64();
        if let Ok(dir) = std::env::var("ASTAR_POUR_DUMP") {
            self.dump_pours(&dir);
        }
        if let Ok(which) = std::env::var("ASTAR_PROBE") {
            self.probe(&which);
        }
    }

    /// Lock a job: its copper stays where it is whatever clashes with it.
    pub fn lock_job(&mut self, j: usize) {
        self.jobs[j].locked = true;
        self.f.set_locked(&self.jobs[j].objs, true);
    }

    /// Lift a job's copper, and with it every finished job whose track ends on it. Returns every job
    /// lifted. A locked job is refused: it is not lifted, nor is anything lifted on its account.
    pub(crate) fn lift(&mut self, j: usize) -> Vec<usize> {
        let tl = Instant::now();
        let mut all: Vec<usize> = Vec::new();
        let mut stack = vec![j];
        while let Some(j) = stack.pop() {
            if all.contains(&j) || self.jobs[j].locked {
                continue;
            }
            all.push(j);
            let net = self.jobs[j].net as usize;
            let objs = std::mem::take(&mut self.jobs[j].objs);
            self.f.remove_objs(&objs);
            let vias = std::mem::take(&mut self.jobs[j].vias);
            self.jobs[j].small.clear();
            self.net_vias[net].retain(|v| !vias.contains(v));
            for &(x, y) in &vias {
                self.via_owner.remove(&(net as u16, x, y));
            }
            self.jobs[j].tracks.clear();
            self.jobs[j].moved.clear();
            self.jobs[j].paths.clear();
            self.jobs[j].rough = false;
            self.jobs[j].ends = (0, 0);
            self.jobs[j].nodes = 0;
            self.jobs[j].clash_cells.clear();
            self.pairs.retain(|p| p.0 != j && p.1 != j);
            self.jobs[j].open.clear();
            self.jobs[j].loose.clear();
            self.jobs[j].pour_pads.clear();
            self.jobs[j].pour_ends.clear();
            self.jobs[j].done = false;
            for v in std::mem::take(&mut self.deps[j]) {
                // (a job with copper; or a drop that has none because old copper joins its pad to this one's)
                if self.jobs[v].done && (!self.jobs[v].objs.is_empty() || self.jobs[v].pad.map_or(false, |p| self.old.of_pad.contains_key(&p))) {
                    stack.push(v);
                }
            }
        }
        self.stats.lift_s += tl.elapsed().as_secs_f64();
        all
    }

    /// Is job j in a clash: laid through another net's copper, or with another net laid through its own?
    pub(crate) fn clashing(&self, j: usize) -> bool {
        self.pairs.iter().any(|p| p.0 == j || p.1 == j)
    }

    /// Before a path of job j is laid: note the jobs whose copper it runs through, and where.
    pub(crate) fn note_clashes(&mut self, j: usize, path: &[(u8, u16, u16)]) {
        let net = self.jobs[j].net;
        let (t_min, half) = (self.t_min(net), self.w_min(net) / 2.0);
        let mut hit: Vec<(u32, u16)> = Vec::new();
        for (i, &(l, y, x)) in path.iter().enumerate() {
            let (l, y, x) = (l as usize, y as usize, x as usize);
            let is_via = (i > 0 && path[i - 1].0 as usize != l) || (i + 1 < path.len() && path[i + 1].0 as usize != l);
            if is_via && !self.net_vias[net as usize].contains(&(x as u16, y as u16)) && !self.net_tht[net as usize].contains(&(x as u16, y as u16)) {
                // (another net's via whose hole is too near, though its copper is not)
                if self.f.holes[y * self.f.w + x] != 0 {
                    let before = hit.len();
                    self.f.holes_near(y, x, net, &mut hit);
                    if hit.len() > before {
                        self.jobs[j].clash_cells.push(self.f.idx(0, y, x));
                    }
                }
                let tv = if self.via_is_small(net, x, y) { self.t_via_s } else { self.t_via_of(net) };
                for ll in 0..self.f.nl {
                    let ci = self.f.idx(ll, y, x);
                    if ddist(&self.f.cells[ci], net) < tv {
                        self.f.foreign_near(ll, y, x, net, tv as f64 * TICK + 1e-6, &mut hit);
                        self.jobs[j].clash_cells.push(ci);
                    }
                }
            }
            // the track at this node: the pieces to its neighbours on the path, measured against the
            // routed copper beside them (as the search measured them: `Router::t_low`)
            let ci = self.f.idx(l, y, x);
            if self.costs.step[l][0] != 0 && ddist(&self.f.cells[ci], net) < t_min {
                let at = (self.f.wx(x), self.f.wy(y));
                let before = hit.len();
                let mut pieces = 0;
                for k in [i.wrapping_sub(1), i + 1] {
                    if let Some(&(l2, y2, x2)) = path.get(k) {
                        if l2 as usize == l {
                            pieces += 1;
                            self.tight().routed_hits(&self.f, at, (self.f.wx(x2 as usize), self.f.wy(y2 as usize)), half, net, l, &mut hit);
                        }
                    }
                }
                if pieces == 0 {
                    self.tight().routed_hits(&self.f, at, at, half, net, l, &mut hit);
                }
                if hit.len() > before {
                    self.jobs[j].clash_cells.push(ci);
                }
            }
        }
        for (job, _) in hit {
            let v = job as usize;
            if v != j && !self.pairs.contains(&(j, v)) {
                self.pairs.push((j, v));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::field::{Kind, FAR, NO_JOB};
    use crate::route::{route, Cfg};
    use crate::scene::Scene;

    fn routed() -> crate::route::Router {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scenes/every_field.json")).unwrap();
        route(Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap(), &Cfg { two_ways: false, ..Cfg::default() }).unwrap().router
    }

    #[test]
    fn every_object_says_what_it_is_whose_and_which_job_laid_it() {
        let r = routed();
        for (j, job) in r.jobs.iter().enumerate() {
            for &id in &job.objs {
                let o = &r.f.objs[id as usize];
                assert!(o.alive && !o.locked && o.job == j as u32 && o.net == job.net);
                assert!(o.kind == Kind::Track || o.kind == Kind::Via);
                assert_eq!(o.kind == Kind::Via, o.layer == crate::field::ALL_LAYERS);
            }
        }
        let count = |k: Kind| r.statics.objs.iter().filter(|o| o.kind == k).count();
        assert_eq!(count(Kind::Pad), r.sc.pads.len());
        assert!(count(Kind::Hole) >= r.sc.holes.len());
        assert_eq!(count(Kind::Keepout), r.sc.keepouts.iter().filter(|k| k.forbid.tracks).count());
        assert_eq!(count(Kind::Edge), r.sc.outline.len());
        assert_eq!(count(Kind::Fence), 2 * r.stats.fences);
        assert!(r.statics.objs.iter().all(|o| o.locked && o.job == NO_JOB));
    }

    #[test]
    fn lift_takes_a_job_up_and_refuses_a_locked_one() {
        let mut r = routed();
        let with_copper: Vec<usize> = (0..r.jobs.len()).filter(|&j| !r.jobs[j].objs.is_empty() && r.deps[j].is_empty()).collect();
        let (a, b) = (with_copper[0], with_copper[1]);
        // a job lifted has no copper left, on the job or in the field
        let objs = r.jobs[a].objs.clone();
        assert_eq!(r.lift(a), vec![a]);
        assert!(r.jobs[a].objs.is_empty() && r.jobs[a].tracks.is_empty() && r.jobs[a].vias.is_empty() && !r.jobs[a].done);
        assert!(objs.iter().all(|&id| !r.f.objs[id as usize].alive));
        // a locked job is refused: nothing is lifted and its copper stands
        r.lock_job(b);
        let (objs, tracks) = (r.jobs[b].objs.clone(), r.jobs[b].tracks.len());
        assert!(r.lift(b).is_empty());
        assert!(r.jobs[b].done && r.jobs[b].objs == objs && r.jobs[b].tracks.len() == tracks);
        assert!(objs.iter().all(|&id| r.f.objs[id as usize].alive && r.f.objs[id as usize].locked));
        // and the field refuses its copper too, whoever asks
        assert_eq!(r.f.remove_objs(&objs), objs.len());
        let o = r.f.objs[objs[0] as usize].clone();
        let (x, y) = r.f.node_near(o.ax, o.ay);
        let l = if o.layer == crate::field::ALL_LAYERS { 0 } else { o.layer as usize };
        assert_ne!(r.f.cells[r.f.idx(l, y, x)].dd1, FAR);
    }

    #[test]
    fn lifting_a_job_lifts_what_leans_on_it_but_never_a_locked_job() {
        let mut r = routed();
        let free: Vec<usize> = (0..r.jobs.len()).filter(|&j| !r.jobs[j].objs.is_empty() && r.deps[j].is_empty()).collect();
        let (base, leaner, held) = (free[0], free[1], free[2]);
        // two jobs whose tracks end on the copper of `base`: one free, one locked
        r.deps[base].push(leaner);
        r.deps[base].push(held);
        r.lock_job(held);
        let mut lifted = r.lift(base);
        lifted.sort();
        assert_eq!(lifted, { let mut v = vec![base, leaner]; v.sort(); v });
        assert!(!r.jobs[held].objs.is_empty() && r.jobs[held].done);
    }
}
