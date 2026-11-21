//! Rules a board sets that astar must keep, whatever its own defaults are: each has a scene that breaks
//! the rule when the rule is not modelled, and astar's exact check as the judge.

use astar::json::J;
use astar::{route, Cfg, Scene};

fn scene(name: &str) -> String {
    format!("{}/tests/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn plan(name: &str) -> J {
    let routed = route(Scene::load(&scene(name)).unwrap(), &Cfg::default()).unwrap();
    assert!(routed.open_nets().is_empty(), "{:?}", routed.open_nets());
    astar::json::parse(&routed.plan()).unwrap()
}

/// `astar route` then `astar check` on a scene: the check's exit code and what it said.
fn checked(name: &str) -> (Option<i32>, String) {
    let dir = std::env::temp_dir().join(format!("astar-rules-{}-{}", name, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let plan = dir.join("plan.json");
    let run = |args: &[&str]| std::process::Command::new(env!("CARGO_BIN_EXE_astar")).args(args).output().unwrap();
    let routed = run(&["route", &scene(name), plan.to_str().unwrap()]);
    assert_eq!(routed.status.code(), Some(0), "{}", String::from_utf8_lossy(&routed.stderr));
    let out = run(&["check", &scene(name), plan.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).ok();
    (out.status.code(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// Twelve nets, each from the front to the back, 0.9 mm apart. A row of vias 0.9 apart is clear by
/// copper (0.6 via, 0.15 clearance) and too near by the board's hole to hole rule (0.3 holes, 0.65
/// between them): the vias are staggered, whatever their nets.
#[test]
fn via_holes_keep_the_boards_distance_from_each_other() {
    let p = plan("holes.json");
    let vias: Vec<Vec<f64>> = p.get("nets").arr().iter().flat_map(|n| n.get("vias").arr().iter().map(|v| v.get("at").nums())).collect();
    assert_eq!(vias.len(), 12);
    for (i, a) in vias.iter().enumerate() {
        for b in &vias[i + 1..] {
            let d = (a[0] - b[0]).hypot(a[1] - b[1]);
            assert!(d >= 0.3 + 0.65 - 1e-9, "two vias {:.3} mm apart", d);
        }
    }
    let (code, said) = checked("holes.json");
    assert_eq!(code, Some(0), "{}", said);
    // and every via says what it is: the writer lays, and the checker checks, the via the router meant
    for n in p.get("nets").arr() {
        for v in n.get("vias").arr() {
            assert_eq!((v.get("size").f(), v.get("drill").f()), (Some(0.6), Some(0.3)));
        }
    }
}

/// A clearance that holds between two classes only (Power and Default: 0.2) and one that holds on one
/// layer only (In2.Cu: 0.18; Power there: 0.25), on a board whose own is 0.15: the exact check, which
/// applies each where it holds, finds the copper clear.
#[test]
fn a_clearance_for_two_classes_or_for_one_layer_is_kept() {
    let (code, said) = checked("every_field.json");
    assert_eq!(code, Some(0), "{}", said);
}

/// A rail 6 mm wide, and a net that asks 5 mm of clearance: both reach further than a signal's track
/// and clearance by a long way, and both are kept clear of the other net's pads.
#[test]
fn a_wide_rail_and_a_large_clearance_are_kept() {
    for name in ["wide_net.json", "far_net.json"] {
        let (code, said) = checked(name);
        assert_eq!(code, Some(0), "{}: {}", name, said);
    }
}

/// A row of pads 0.2 wide and 0.2 apart, on a board whose clearance and track are 0.2: a track leaves a
/// pad of the row only straight out, as wide as the pad, its edges at the clearance from both neighbours
/// exactly. Every check agrees that this fits (one tolerance, `geom::SLACK`): the nets are joined and
/// the plan is clean.
#[test]
fn a_track_that_fits_exactly_is_laid() {
    let p = plan("exact_fit.json");
    assert_eq!(p.get("nets").arr().len(), 6);
    let (code, said) = checked("exact_fit.json");
    assert_eq!(code, Some(0), "{}", said);
}

/// Seven nets straight across, pad to pad, the pads 0.2 wide on a 0.4 pitch, the board's track and
/// clearance 0.2: seven tracks side by side fit exactly, and that is how they are laid. (The grid alone
/// cannot tell a track that fits exactly from one a hundredth too near, and used to send every other net
/// round the back or down a via; a step it is not sure of is measured, piece against shape.)
#[test]
fn tracks_run_side_by_side_at_exactly_the_clearance() {
    let p = plan("lanes.json");
    let nets = p.get("nets").arr();
    assert_eq!(nets.len(), 7);
    for n in nets {
        assert!(n.get("vias").arr().is_empty(), "a via");
        let tracks = n.get("tracks").arr();
        assert_eq!(tracks.len(), 1);
        let path = tracks[0].get("path").arr();
        assert_eq!(path.len(), 1, "not one straight piece");
        let l = path[0].get("line").nums();
        assert!((l[0] - l[2]).abs() < 1e-9 && ((l[1] - l[3]).abs() - 8.0).abs() < 1e-9, "{:?}", l);
    }
    let (code, said) = checked("lanes.json");
    assert_eq!(code, Some(0), "{}", said);
}

/// A QFN-28 fanned out to a 2x10 header, 0.19 mm tracks at 0.19 mm. SCAN2 leaves the chip's right side
/// between two neighbours, and is routed again while one of them stands where it does not stay: found
/// then, its way steps aside for that neighbour and back. Once every net has its way each is drawn
/// again, so it ends as it would have been found on the finished board: out of the pin, one bend up
/// the diagonal, one bend up to the header.
#[test]
fn a_track_that_stepped_aside_for_a_neighbour_since_moved_ends_straight() {
    let p = plan("fan.json");
    let nets = p.get("nets").arr();
    let n = nets.iter().find(|n| n.get("name").s() == Some("SCAN2")).unwrap();
    let tracks = n.get("tracks").arr();
    assert_eq!(tracks.len(), 1);
    let lines = tracks[0].get("path").arr().iter().filter(|s| s.has("line")).count();
    assert_eq!(lines, 3, "{:?}", tracks[0].get("path").arr().len());
    let (code, said) = checked("fan.json");
    assert_eq!(code, Some(0), "{}", said);
}
