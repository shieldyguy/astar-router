//! astar, the command: the library (src/lib.rs) with a command line round it.
//!
//!     astar route SCENE.json PLAN.json [flags]     route a scene; exit 0 whole, 1 nets open, 2 bad input,
//!                                                  3 a rule narrowed under --strict, 4 its own check
//!                                                  found fault with the plan
//!     astar SCENE.json PLAN.json [flags]           the same, the old way: exit 0 whenever a plan was written
//!     astar egress SCENE.json [flags]              the egress plan of a scene, before any copper is laid
//!     astar check SCENE.json PLAN.json             astar's own exact check of a plan: exit 0 clean, 1 findings
//!     astar explain                                not built yet: it says whose it is
//!     astar --passes                               the passes, in the order they run

use astar::passes::PASSES;
use astar::{exit, route, Cfg, Scene};
use std::time::Instant;

const USAGE: &str = "usage: astar route IN.kicad_pcb -o OUT.kicad_pcb [--plan PLAN.json] [--scene SCENE.json] [--seed N] [flags as below]
                                          a board in, the routed board out, its .kicad_pro, .kicad_dru and .kicad_prl copied beside it
       astar scene IN.kicad_pcb [-o SCENE.json]                the board as a scene (docs/scene.md)
       astar lay BOARD.kicad_pcb PLAN.json -o OUT.kicad_pcb    a plan laid on the board it was made for
       astar route SCENE.json PLAN.json [--pitch MM] [--weight W] [--weight-soft W] [--via MM] [--plane-via MM]
             [--turn MM] [--inner X] [--rip MM] [--rip-drop MM] [--fence MM] [--lane MM] [--toll X] [--pour PADS]
             [--one-way] [--rounds N] [--pressure X] [--radius MM] [--seconds S] [--skip PASS] [--strict] [-v]
             [--cross X] [--drops NET[,NET]]
         --drops NET: the net is joined off this board (a block laid out alone, its ground the host's pour):
                      its pads go down by vias (a pad may share one) and nothing joins the vias here
         exit: 0 every net joined, 1 nets open, 2 the input could not be taken, 3 a rule narrowed (with --strict),
               4 astar's own check found fault with the plan it wrote (a fault in astar: the findings are printed)
       astar SCENE.json PLAN.json [...]   the same; exit 0 whenever a plan was written
       astar check SCENE.json PLAN.json   astar's own exact check of a plan against its scene: exit 0 clean, 1 findings
       astar explain ...                  why a pad or a net could not be joined (not built yet: T12)
       astar egress SCENE.json [--pitch MM] [--skip PASS] [--part REF] [-v]
                                          the egress plan of a scene, before any copper; exit 1 on a deficit
       astar --passes                     the passes, in the order they run";

fn usage() -> ! {
    eprintln!("{}", USAGE);
    std::process::exit(exit::BAD_INPUT);
}

/// The input could not be taken: say why, exit 2.
fn refuse(what: &str) -> ! {
    eprintln!("astar: {}", what);
    std::process::exit(exit::BAD_INPUT);
}

fn main() {
    // a panic is a bug in astar, never the input's fault: say so, with where it came from
    std::panic::set_hook(Box::new(|info| {
        eprintln!("astar: BUG: {}\nThis is a fault in astar, not in the scene. The scene and this trace are what a fix needs:\n{}", info, std::backtrace::Backtrace::force_capture());
    }));
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|a| a.as_str()) {
        Some("route") => {
            args.remove(0);
            route_command(&args, false)
        }
        Some("scene") => std::process::exit(astar::io::cli::scene_command(&args[1..])),
        Some("lay") => std::process::exit(astar::io::cli::lay_command(&args[1..])),
        Some("check") => std::process::exit(astar::check::command(&args[1..])),
        Some("explain") => refuse("explain is not built yet. It is T12's: why a pad or a net could not be joined."),
        Some("egress") => std::process::exit(astar::passes::egress::command(&args[1..])),
        // the old form has no command and never exits 1
        _ => route_command(&args, true),
    }
}

/// `astar route`: flags into a `Cfg`, the scene read, routed, the plan written.
fn route_command(args: &[String], old_form: bool) -> ! {
    let t0 = Instant::now();
    let mut cfg = Cfg::default();
    let mut files: Vec<String> = Vec::new();
    let mut strict = false;
    // the board form: IN.kicad_pcb -o OUT.kicad_pcb, the plan and the scene written too when asked
    let (mut out_board, mut plan_out, mut scene_out, mut seed): (Option<String>, Option<String>, Option<String>, u64) = (None, None, None, 0);
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let val = |i: &mut usize| -> f64 {
            *i += 1;
            match args.get(*i).and_then(|s| s.parse::<f64>().ok()) {
                Some(v) => v,
                None => usage(),
            }
        };
        match a {
            "--pitch" => cfg.pitch = val(&mut i),
            "--weight" => cfg.weight = val(&mut i),
            "--weight-soft" => cfg.weight_soft = val(&mut i),
            "--fence" => cfg.fence_mm = val(&mut i),
            "--lane" => cfg.lane_mm = val(&mut i),
            "--toll" => cfg.lane_toll = val(&mut i),
            "--pour" => cfg.pour_pads = val(&mut i) as usize,
            "--one-way" => cfg.two_ways = false,
            "--drops" => {
                i += 1;
                match args.get(i) {
                    Some(names) if !names.starts_with("--") => cfg.drops.extend(names.split(',').filter(|n| !n.is_empty()).map(str::to_string)),
                    _ => refuse("--drops takes the name of a net (or several, with commas between)"),
                }
            }
            "--cross" => cfg.cross_mul = val(&mut i),
            "--rounds" => cfg.rounds = val(&mut i) as u32,
            "--pressure" => cfg.pressure = val(&mut i),
            "--via" => cfg.via_mm = val(&mut i),
            "--plane-via" => cfg.plane_via_mm = val(&mut i),
            "--turn" => cfg.turn_mm = val(&mut i),
            "--inner" => cfg.inner_mul = val(&mut i),
            "--rip" => cfg.rip_mm = val(&mut i),
            "--rip-drop" => cfg.rip_drop_mm = val(&mut i),
            "--radius" => cfg.radius_mm = val(&mut i),
            "--seconds" => cfg.budget_s = val(&mut i),
            "--skip" => {
                i += 1;
                match args.get(i) {
                    Some(name) if PASSES.iter().any(|p| p.name == name) => cfg.skip.push(name.clone()),
                    _ => refuse(&format!("--skip takes the name of a pass: {}", PASSES.iter().map(|p| p.name).collect::<Vec<_>>().join(", "))),
                }
            }
            "--passes" => {
                for p in PASSES {
                    println!("{:<7} {:<7} {}", p.name, p.step.stage(), p.about);
                }
                std::process::exit(exit::WHOLE);
            }
            "-o" | "--out" | "--plan" | "--scene" => {
                i += 1;
                let Some(v) = args.get(i) else { usage() };
                match a {
                    "--plan" => plan_out = Some(v.clone()),
                    "--scene" => scene_out = Some(v.clone()),
                    _ => out_board = Some(v.clone()),
                }
            }
            "--seed" => seed = val(&mut i) as u64,
            "--strict" => strict = true,
            "-v" | "--verbose" => cfg.verbose = true,
            "-h" | "--help" => usage(),
            _ if a.starts_with('-') => usage(),
            _ => files.push(a.to_string()),
        }
        i += 1;
    }
    if let ([board], Some(out)) = (files.as_slice(), &out_board) {
        let run = astar::io::cli::BoardRun { board, out, plan: plan_out.as_deref(), scene: scene_out.as_deref(), seed, strict };
        std::process::exit(astar::io::cli::route_board(&run, &cfg));
    }
    if files.len() != 2 || out_board.is_some() || plan_out.is_some() || scene_out.is_some() {
        usage();
    }
    let sc = Scene::load(&files[0]).unwrap_or_else(|e| refuse(&e.to_string()));
    let read_s = t0.elapsed().as_secs_f64();
    let asked = sc.clone();
    let mut out = route(sc, &cfg).unwrap_or_else(|e| refuse(&format!("{}: {}", files[0], e)));
    out.read_s = read_s;
    out.seconds = t0.elapsed().as_secs_f64() - read_s;
    let plan = out.plan();
    if let Err(e) = std::fs::write(&files[1], &plan) {
        refuse(&format!("cannot write {}: {}", files[1], e));
    }
    for line in out.lines(cfg.verbose) {
        eprintln!("{}", line);
    }
    // the run's word is checked before it is given: the plan against the scene, by the exact check
    let scene_json = std::fs::read_to_string(&files[0]).ok().and_then(|t| astar::json::parse(&t).ok()).unwrap_or(astar::json::J::Null);
    let (said, faults) = astar::check::of_run(&scene_json, &asked, &plan);
    for line in &said {
        eprintln!("{}", line);
    }
    let code = if faults > 0 { exit::FAULT } else { out.exit_code(strict) };
    std::process::exit(if old_form && code == exit::OPEN { exit::WHOLE } else { code });
}
