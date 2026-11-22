//! A run: the board through the passes, in order; both ways at once when a board pass changed it.

use super::{Cfg, Router};
use crate::passes::{Step, FIELD, PASSES};
use crate::scene::Scene;
use crate::Error;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A routed board and what the run has to say about how it chose it.
pub struct Routed {
    /// the board that was kept: its scene, its jobs with their copper, its notes, its clock
    pub router: Router,
    /// how the run chose between two boards, when it had two
    pub note: String,
    /// the seconds `route` took
    pub seconds: f64,
    /// the seconds the caller spent reading the scene, for the plan's timing (0 unless the caller says)
    pub read_s: f64,
}

fn on(cfg: &Cfg, i: usize) -> bool {
    !cfg.skip.iter().any(|s| s == PASSES[i].name)
}

/// One board through the plan passes and the route. `limit`, `progress`: see `Router`.
///
/// How a pad is left and which layer a pour is crossed on are matters of style, and style does not cost
/// a net: a board that stays open with the style rules on (`passes::exits`, `passes::cross`) is routed
/// once more without them, in what is left of the time, and that board is kept when fewer of its nets
/// are open (`Router::sides` then says how many the rules had left open).
/// Only when a net is open for want of room among the others: a net the board itself walls in
/// (`Job::walled`) is open either way, and a board with none but such nets is routed once. Nor is a
/// board whose time ran out routed again.
fn board(sc: Scene, cfg: &Cfg, poured: Vec<(u16, usize)>, limit: Option<Arc<AtomicU64>>, progress: Option<Arc<AtomicU64>>) -> Result<Router, String> {
    const STYLE: [&str; 2] = ["exits", "cross"];
    let t = Instant::now();
    let r = board_once(sc.clone(), cfg, poured.clone(), limit.clone(), progress.clone())?;
    let open = r.open_nets().len();
    if open == 0 || r.abandoned || r.out_of_time() || !r.open_for_room() || STYLE.iter().all(|p| cfg.skip.iter().any(|s| s == p)) {
        return Ok(r);
    }
    if cfg.verbose {
        for j in (0..r.jobs.len()).filter(|&j| r.for_room(j)) {
            let job = &r.jobs[j];
            eprintln!("  open for want of room: {} {} ({} pads open, {} of them walled in; {} loose pieces, {} walled in)", job.kind.name, r.sc.nets[job.net as usize].name, job.open.len(), job.walled.len(), job.loose.len(), job.walled_loose);
        }
    }
    let mut free = cfg.clone();
    free.skip.extend(STYLE.iter().map(|p| p.to_string()));
    if cfg.budget_s > 0.0 {
        // (0 would mean no limit)
        free.budget_s = (cfg.budget_s - t.elapsed().as_secs_f64()).max(0.01);
    }
    let mut other = board_once(sc, &free, poured, limit, progress)?;
    if other.open_nets().len() < open {
        other.sides = Some(open);
        return Ok(other);
    }
    Ok(r)
}

fn board_once(sc: Scene, cfg: &Cfg, poured: Vec<(u16, usize)>, limit: Option<Arc<AtomicU64>>, progress: Option<Arc<AtomicU64>>) -> Result<Router, String> {
    let t = Instant::now();
    let mut r = Router::new(sc, cfg.clone(), poured)?;
    r.limit = limit;
    r.progress = progress;
    r.clock.add(FIELD, t.elapsed().as_secs_f64());
    r.clock.enter(FIELD);
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Plan(f), true) = (p.step, on(cfg, i)) {
            r.clock.enter(i);
            f(&mut r);
            r.clock.leave();
        }
    }
    // (after the plan passes: where a via may stand depends on the fixed copper they added)
    r.find_via_sites();
    r.clock.leave();
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Path(f), true) = (p.step, on(cfg, i)) {
            r.shapers.push((i, f));
        }
    }
    r.stats.build_s = t.elapsed().as_secs_f64();
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Route(f), true) = (p.step, on(cfg, i)) {
            r.clock.enter(i);
            f(&mut r);
            r.clock.leave();
        }
    }
    Ok(r)
}

/// The search a changed board may do, given what the plain board did: a quarter more, and four million
/// nodes (under half a second: a small board is not sent back to tracks for its rails over that).
fn allowed(plain_work: u64) -> u64 {
    plain_work + plain_work / 4 + 4_000_000
}

/// The board routed both ways at once: as the board passes changed it (a rail poured on the free inner
/// layer makes a tidier board, but takes that layer from the signals, and a crowded board needs it),
/// and plain. Which is kept depends on the two boards' open nets and search work alone, never on which
/// thread was done first: the changed board is kept when it did no more search than `allowed` by the
/// plain one's and is whole, or has fewer nets open than the plain one.
fn both_ways(changed: Scene, poured: Vec<(u16, usize)>, plain: Scene, cfg: &Cfg) -> Routed {
    let (c1, c2) = (cfg.clone(), cfg.clone());
    // (both boards were checked before the threads were started: `route`)
    let checked = "the board was checked";
    race(move |limit| board(changed, &c1, poured, Some(limit), None).expect(checked), move |stop, progress| board(plain, &c2, Vec::new(), Some(stop), Some(progress)).expect(checked), allowed)
}

/// `changed(limit)` and `plain(stop, progress)` each route a board on a thread of its own; see `both_ways`.
fn race<A, B>(changed: A, plain: B, allowed: fn(u64) -> u64) -> Routed
where
    A: FnOnce(Arc<AtomicU64>) -> Router + Send + 'static,
    B: FnOnce(Arc<AtomicU64>, Arc<AtomicU64>) -> Router + Send + 'static,
{
    const POURED: &str = "rail poured on the free inner layer";
    const NEEDED: &str = "rails left as track: the free inner layer is needed for signal";
    // the most search the changed board may do: known once the plain board is in
    let limit = Arc::new(AtomicU64::new(u64::MAX));
    // set to 0 when the plain board is no longer wanted
    let stop = Arc::new(AtomicU64::new(u64::MAX));
    let progress = Arc::new(AtomicU64::new(0));
    let (tx, rx) = std::sync::mpsc::channel::<(bool, Router)>();
    {
        let (limit, tx) = (limit.clone(), tx.clone());
        std::thread::spawn(move || {
            let _ = tx.send((true, changed(limit)));
        });
    }
    {
        let (stop, progress) = (stop.clone(), progress.clone());
        std::thread::spawn(move || {
            let _ = tx.send((false, plain(stop, progress)));
        });
    }
    let failed = "a routing thread failed";
    let (first_changed, first) = rx.recv().expect(failed);
    let (poured, plain) = if !first_changed {
        // the plain board is in: the other may do as much search as that allows, then it gives up
        limit.store(allowed(first.totals().2), Ordering::Relaxed);
        (rx.recv().expect(failed).1, first)
    } else if !first.open_nets().is_empty() {
        let plain = rx.recv().expect(failed).1;
        (first, plain)
    } else {
        // whole: it is kept if the plain board's search allows it. That is known for sure as soon as
        // the plain board has done enough, and at the latest when it is done.
        let work = first.totals().2;
        loop {
            if work <= allowed(progress.load(Ordering::Relaxed)) {
                stop.store(0, Ordering::Relaxed);
                return Routed::of(first, POURED.to_string());
            }
            match rx.recv_timeout(Duration::from_millis(2)) {
                Ok((_, plain)) => break (first, plain),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => panic!("{}", failed),
            }
        }
    };
    let (open_plain, open_poured) = (plain.open_nets().len(), poured.open_nets().len());
    if poured.abandoned || poured.totals().2 > allowed(plain.totals().2) {
        Routed::of(plain, format!("{} (with a rail poured the board takes more search than without)", NEEDED))
    } else if open_poured == 0 {
        Routed::of(poured, POURED.to_string())
    } else if open_poured < open_plain {
        Routed::of(poured, format!("{} (without it: {} nets open)", POURED, open_plain))
    } else {
        Routed::of(plain, format!("{} (with a rail poured: {} nets open at {} nodes of search)", NEEDED, open_poured, poured.totals().2))
    }
}

impl Routed {
    fn of(router: Router, note: String) -> Routed {
        Routed { router, note, seconds: 0.0, read_s: 0.0 }
    }

    /// The plan as JSON: router-bench's plan (its CONTRACT.md) with the run's notes, timing and summary.
    pub fn plan(&self) -> String {
        crate::report::plan::plan_json(&self.router, &self.note, self.read_s, self.read_s + self.seconds)
    }

    /// The run in a line; with `verbose`, what lies behind it.
    pub fn lines(&self, verbose: bool) -> Vec<String> {
        crate::report::plan::lines(&self.router, &self.note, self.read_s, self.read_s + self.seconds, verbose)
    }

    /// The nets with a pad left unjoined: (name, pads joined, pads in all).
    pub fn open_nets(&self) -> Vec<(&str, usize, usize)> {
        self.router.open_nets().into_iter().map(|(n, got, all)| (self.router.sc.nets[n as usize].name.as_str(), got, all)).collect()
    }

    /// What the command exits with for this board (`crate::exit`). `strict`: a narrowed rule counts.
    pub fn exit_code(&self, strict: bool) -> i32 {
        if self.router.notes.iter().any(|n| n.kind == "open") {
            crate::exit::OPEN
        } else if strict && self.router.notes.iter().any(|n| n.kind == "narrowed") {
            crate::exit::NARROWED
        } else {
            crate::exit::WHOLE
        }
    }
}

/// The board through the board passes, the plan passes and the route: both ways when a board pass
/// changed it.
fn routed(sc: Scene, cfg: &Cfg) -> Result<Routed, Error> {
    let mut changed = sc.clone();
    let mut poured: Vec<(u16, usize)> = Vec::new();
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Board(f), true) = (p.step, on(cfg, i)) {
            poured.extend(f(&mut changed, cfg));
        }
    }
    Ok(if poured.is_empty() || super::check(&changed, cfg).is_err() {
        // (a board the board passes made unroutable is routed as it came)
        Routed::of(board(sc, cfg, Vec::new(), None, None).map_err(Error::BadInput)?, String::new())
    } else if cfg.two_ways {
        both_ways(changed, poured, sc, cfg)
    } else {
        Routed::of(board(changed, cfg, poured, None, None).map_err(Error::BadInput)?, String::new())
    })
}

/// Route a scene: the board passes, the field, the plan passes, the route, the finish. An error is a
/// scene or settings the router cannot take; nets left open are not an error (`Routed::exit_code`).
pub fn route(mut sc: Scene, cfg: &Cfg) -> Result<Routed, Error> {
    let t0 = Instant::now();
    super::check(&sc, cfg).map_err(Error::BadInput)?;
    super::ask(&mut sc, cfg);
    let mut out = routed(sc, cfg)?;
    if let Some(open) = out.router.sides {
        let how = format!("the style rules are left out (a pad left by its end, a pour not crossed for nothing): with them {} nets stayed open", open);
        out.note = if out.note.is_empty() { how } else { format!("{}; {}", out.note, how) };
    }
    let r = &mut out.router;
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Finish(f), true) = (p.step, on(cfg, i)) {
            r.clock.enter(i);
            f(r);
            r.clock.leave();
        }
    }
    if !out.note.is_empty() {
        r.note("choice", 0, None, out.note.clone());
    }
    for line in r.sc.carried_unused() {
        r.note("unused", 0, None, format!("the scene carries {}: not used yet", line));
    }
    r.close_report();
    out.seconds = t0.elapsed().as_secs_f64();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::passes::pour::pour_choice;

    fn fixture() -> Scene {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scenes/every_field.json")).unwrap();
        Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap()
    }

    /// The fixture routed both ways, the poured board held back `wait.0` ms and the plain one `wait.1`.
    fn raced(wait: (u64, u64), allowed: fn(u64) -> u64) -> Routed {
        let cfg = Cfg::default();
        let plain = fixture();
        let mut changed = plain.clone();
        let (net, layer) = pour_choice(&changed, &cfg).expect("the fixture has a rail to pour");
        changed.nets[net].plane = Some(layer);
        let (c1, c2) = (cfg.clone(), cfg.clone());
        race(
            move |limit| {
                std::thread::sleep(Duration::from_millis(wait.0));
                board(changed, &c1, vec![(net as u16, layer)], Some(limit), None).unwrap()
            },
            move |stop, progress| {
                std::thread::sleep(Duration::from_millis(wait.1));
                board(plain, &c2, Vec::new(), Some(stop), Some(progress)).unwrap()
            },
            allowed,
        )
    }

    #[test]
    fn the_choice_does_not_depend_on_which_board_is_in_first() {
        // within what the plain board's search allows: the poured board is kept, whichever is first
        for wait in [(0, 150), (150, 0)] {
            let out = raced(wait, allowed);
            assert_eq!(out.router.poured.len(), 1, "poured held back {} ms, plain {} ms: {}", wait.0, wait.1, out.note);
            assert!(out.router.open_nets().is_empty());
        }
        // a whole poured board that did more search than allowed is given up, also when it is in first
        for wait in [(0, 150), (150, 0)] {
            let out = raced(wait, |work| work / 1000);
            assert!(out.router.poured.is_empty(), "poured held back {} ms, plain {} ms: {}", wait.0, wait.1, out.note);
            assert!(out.note.contains("more search than without"), "{}", out.note);
        }
    }
}
