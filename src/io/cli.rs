//! The commands of board I/O: `astar scene`, `astar lay`, and the board form of `astar route`.

use super::{copy_siblings, read, write};
use crate::{exit, Cfg};
use std::time::Instant;

fn fail(what: &str) -> i32 {
    eprintln!("astar: {}", what);
    exit::BAD_INPUT
}

/// The value after a flag.
fn value<'a>(args: &'a [String], i: &mut usize) -> Option<&'a str> {
    *i += 1;
    args.get(*i).map(|s| s.as_str())
}

/// `astar scene IN.kicad_pcb [-o SCENE.json]`: the board as a scene 2, to the file or to stdout.
pub fn scene_command(args: &[String]) -> i32 {
    let (mut files, mut out) = (Vec::new(), None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => out = value(args, &mut i),
            a if a.starts_with('-') => return fail("usage: astar scene IN.kicad_pcb [-o SCENE.json]"),
            a => files.push(a),
        }
        i += 1;
    }
    let [board] = files.as_slice() else { return fail("usage: astar scene IN.kicad_pcb [-o SCENE.json]") };
    let rd = match read(board) {
        Ok(rd) => rd,
        Err(e) => return fail(&e.to_string()),
    };
    for n in &rd.notes {
        eprintln!("astar: board: {}", n);
    }
    // a scene the router would not take is still written: the reason says what to look at
    let checked = rd.to_scene();
    let text = rd.scene_json();
    let written = match out {
        Some(p) => std::fs::write(p, text + "\n").map_err(|e| format!("cannot write {}: {}", p, e)),
        None => {
            println!("{}", text);
            Ok(())
        }
    };
    match (written, checked) {
        (Err(e), _) => fail(&e),
        (_, Err(e)) => fail(&format!("{}: the scene is not one the router takes: {}", board, e)),
        _ => exit::WHOLE,
    }
}

/// `astar lay BOARD.kicad_pcb PLAN.json -o OUT.kicad_pcb [--seed N]`: a plan onto the board it was
/// made for.
pub fn lay_command(args: &[String]) -> i32 {
    const USAGE: &str = "usage: astar lay BOARD.kicad_pcb PLAN.json -o OUT.kicad_pcb [--seed N]";
    let (mut files, mut out, mut seed) = (Vec::new(), None, 0u64);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => out = value(args, &mut i),
            "--seed" => match value(args, &mut i).and_then(|s| s.parse().ok()) {
                Some(s) => seed = s,
                None => return fail(USAGE),
            },
            a if a.starts_with('-') => return fail(USAGE),
            a => files.push(a),
        }
        i += 1;
    }
    let ([board, plan], Some(out)) = (files.as_slice(), out) else { return fail(USAGE) };
    let done = read(board).and_then(|rd| {
        let text = std::fs::read_to_string(plan).map_err(|e| crate::Error::BadInput(format!("{}: cannot read: {}", plan, e)))?;
        let laid = write(&rd, &text, out, seed)?;
        copy_siblings(board, out).map_err(crate::Error::BadInput)?;
        Ok(laid)
    });
    match done {
        Ok(laid) => {
            for n in &laid.notes {
                eprintln!("astar: note: {}", n);
            }
            eprintln!("astar: {}: {} tracks, {} arcs, {} vias, {} zones laid{}", out, laid.tracks, laid.arcs, laid.vias, laid.zones, if laid.lifted > 0 { format!(", {} lifted", laid.lifted) } else { String::new() });
            exit::WHOLE
        }
        Err(e) => fail(&e.to_string()),
    }
}

/// Where the board form of `astar route` puts things.
pub struct BoardRun<'a> {
    pub board: &'a str,
    pub out: &'a str,
    /// the plan and the scene are written too when asked
    pub plan: Option<&'a str>,
    pub scene: Option<&'a str>,
    pub seed: u64,
    pub strict: bool,
}

/// `astar route IN.kicad_pcb -o OUT.kicad_pcb`: the board read, routed, written, its sibling files
/// copied. The exit code is the scene form's.
pub fn route_board(run: &BoardRun, cfg: &Cfg) -> i32 {
    let t0 = Instant::now();
    let rd = match read(run.board) {
        Ok(rd) => rd,
        Err(e) => return fail(&e.to_string()),
    };
    for n in &rd.notes {
        eprintln!("astar: board: {}", n);
    }
    if let Some(p) = run.scene {
        if let Err(e) = std::fs::write(p, rd.scene_json() + "\n") {
            return fail(&format!("cannot write {}: {}", p, e));
        }
    }
    let sc = match rd.to_scene() {
        Ok(sc) => sc,
        Err(e) => return fail(&format!("{}: {}", run.board, e)),
    };
    let asked = sc.clone();
    let read_s = t0.elapsed().as_secs_f64();
    let mut routed = match crate::route(sc, cfg) {
        Ok(r) => r,
        Err(e) => return fail(&format!("{}: {}", run.board, e)),
    };
    routed.read_s = read_s;
    routed.seconds = t0.elapsed().as_secs_f64() - read_s;
    let plan = routed.plan();
    if let Some(p) = run.plan {
        if let Err(e) = std::fs::write(p, &plan) {
            return fail(&format!("cannot write {}: {}", p, e));
        }
    }
    let laid = match write(&rd, &plan, run.out, run.seed).and_then(|l| copy_siblings(run.board, run.out).map(|_| l).map_err(crate::Error::BadInput)) {
        Ok(l) => l,
        Err(e) => return fail(&e.to_string()),
    };
    for line in routed.lines(cfg.verbose) {
        eprintln!("{}", line);
    }
    for n in &laid.notes {
        eprintln!("astar: note: {}", n);
    }
    // astar fills no zone: what the written board holds as fill is not what KiCad will make of it
    let pours = asked.copper.zones.iter().filter(|z| z.net != 0 && z.outline.len() >= 3).count();
    if pours + laid.zones > 0 {
        let new = if laid.zones > 0 { format!(" ({} of them drawn by astar, with no fill at all)", laid.zones) } else { String::new() };
        eprintln!("astar: REFILL: {} has {} pours{}, and astar fills none: their fill is as the board came, not round the new copper. Refill before the board is judged or made: kicad-cli pcb drc --refill-zones judges it refilled", run.out, pours + laid.zones, new);
    }
    // the run's word is checked before it is given: the plan against the scene, by the exact check
    let (said, faults) = crate::check::of_run(&rd.scene, &asked, &plan);
    for line in &said {
        eprintln!("{}", line);
    }
    for d in &rd.doubts {
        eprintln!("astar: NOT HELD TO: {}. KiCad's DRC will hold the board to it.", d);
    }
    if faults > 0 {
        return crate::exit::FAULT;
    }
    match routed.exit_code(run.strict) {
        crate::exit::WHOLE if !rd.doubts.is_empty() => crate::exit::UNSURE,
        code => code,
    }
}
