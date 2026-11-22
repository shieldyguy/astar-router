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
fn both_ways(changed: Scene, poured: Vec<(u16, usize)>, plain: Scene, drawn: Vec<(u16, usize)>, cfg: &Cfg) -> Routed {
    let (c1, c2) = (cfg.clone(), cfg.clone());
    // (both boards were checked before the threads were started: `route`)
    let checked = "the board was checked";
    race(move |limit| board(changed, &c1, poured, Some(limit), None).expect(checked), move |stop, progress| board(plain, &c2, drawn, Some(stop), Some(progress)).expect(checked), allowed)
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
        } else if self.router.notes.iter().any(|n| n.kind == "short") {
            crate::exit::UNSURE
        } else {
            crate::exit::WHOLE
        }
    }
}

/// The board through the passes that draw on it, the board passes, the plan passes and the route.
///
/// What a `Draw` pass draws (the ground's copper, the planes the run names) is the board from there on.
/// What a `Try` pass lays (a pair, side by side, before anything else) is copper nothing moves, and may
/// be in a neighbour's way: a board that stays open with it is routed once more without, and that board
/// is kept when fewer of its nets are open; the plan then says the pair was routed as two nets.
fn routed(sc: Scene, cfg: &Cfg) -> Result<Routed, Error> {
    // a drawing the router cannot take (`check`) is left out, and the board routed as it came
    let (mut sc, mut drawn) = (sc, Vec::new());
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Draw(f), true) = (p.step, on(cfg, i)) {
            let mut next = sc.clone();
            let got = f(&mut next, cfg);
            if !got.is_empty() && super::check(&next, cfg).is_ok() {
                sc = next;
                drawn.extend(got);
            }
        }
    }
    let mut tried = sc.clone();
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Try(f), true) = (p.step, on(cfg, i)) {
            let mut next = tried.clone();
            f(&mut next, cfg);
            if super::check(&next, cfg).is_ok() {
                tried = next;
            }
        }
    }
    if tried.copper.tracks.len() == sc.copper.tracks.len() {
        // (nothing was laid: what the passes said of that is all that is kept of them)
        sc.said = tried.said;
        return both_or_one(sc, drawn, cfg);
    }
    let t = Instant::now();
    let with = both_or_one(tried.clone(), drawn.clone(), cfg)?;
    let open = with.open_nets().len();
    // a pad that could be joined to the copper laid for its net only the long way round (the laid copper
    // came out on the wrong side of something): the track it took, and what the straight way would be
    let mut round: Option<(f64, f64)> = None;
    for net in 1..tried.nets.len() {
        let ends: Vec<(f64, f64)> = tried
            .copper
            .tracks
            .iter()
            .filter(|t| t.laid && t.net as usize == net)
            .flat_map(|t| match (t.path.first(), t.path.last()) {
                (Some(crate::scene::PathPiece::Line(a)), Some(crate::scene::PathPiece::Line(b))) => vec![(a[0], a[1]), (b[2], b[3])],
                _ => Vec::new(),
            })
            .collect();
        if ends.is_empty() {
            continue;
        }
        let straight: f64 = tried.nets[net].pads.iter().map(|&q| ends.iter().map(|e| crate::geom::hypot(tried.pads[q].x - e.0, tried.pads[q].y - e.1)).fold(f64::MAX, f64::min)).sum();
        let took: f64 = with.router.jobs.iter().filter(|j| j.net as usize == net).flat_map(|j| j.tracks.iter()).map(|t| t.length()).sum();
        if took > 2.0 * straight + 2.0 && round.map_or(true, |r| took - straight > r.0 - r.1) {
            round = Some((took, straight));
        }
    }
    if (open == 0 && round.is_none()) || with.router.abandoned || with.router.out_of_time() {
        return Ok(with);
    }
    let mut free = cfg.clone();
    if cfg.budget_s > 0.0 {
        free.budget_s = (cfg.budget_s - t.elapsed().as_secs_f64()).max(0.01);
    }
    let why = match round {
        Some((took, straight)) if open == 0 => format!("its pads could be joined to it only the long way round ({:.1} mm of track where {:.1} mm is the straight way)", took, straight),
        _ => format!("it left {} nets open that are joined without it", open),
    };
    for (net, kind, text) in &tried.said {
        let text = if text.starts_with("laid ") { format!("routed as two nets: laid side by side first, {}", why) } else { text.clone() };
        sc.said.push((*net, *kind, text));
    }
    let without = both_or_one(sc, drawn, &free)?;
    Ok(if without.open_nets().len() < open || (round.is_some() && without.open_nets().len() <= open) { without } else { with })
}

/// The board through the board passes, the plan passes and the route: both ways when a board pass
/// changed it.
fn both_or_one(sc: Scene, drawn: Vec<(u16, usize)>, cfg: &Cfg) -> Result<Routed, Error> {
    let mut changed = sc.clone();
    let mut poured = drawn.clone();
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Board(f), true) = (p.step, on(cfg, i)) {
            poured.extend(f(&mut changed, cfg));
        }
    }
    Ok(if poured.len() == drawn.len() || super::check(&changed, cfg).is_err() {
        // (a board the board passes made unroutable is routed as it came)
        Routed::of(board(sc, cfg, drawn, None, None).map_err(Error::BadInput)?, String::new())
    } else if cfg.two_ways {
        both_ways(changed, poured, sc, drawn, cfg)
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
