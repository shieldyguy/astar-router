//! A net left open says why: the board itself shuts a pad in (a matter for the placement or the rules),
//! or the other nets left it no room.

use astar::json::J;
use astar::{route, Cfg, Scene};

fn scene(name: &str) -> String {
    format!("{}/tests/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn plan(name: &str) -> J {
    astar::json::parse(&route(Scene::load(&scene(name)).unwrap(), &Cfg::default()).unwrap().plan()).unwrap()
}

fn open_note(p: &J) -> String {
    let notes: Vec<&J> = p.get("notes").arr().iter().filter(|n| n.get("kind").s() == Some("open")).collect();
    assert_eq!(notes.len(), 1);
    notes[0].get("detail").s().unwrap().to_string()
}

/// B's second pad stands in a ring of keep-out. The run says so, and does not try it again: every net
/// is routed once, and the board is not routed a second time.
#[test]
fn a_pad_the_board_shuts_in_is_said_and_not_tried_again() {
    let p = plan("walled.json");
    let why = open_note(&p);
    assert!(why.contains("the board itself leaves no way to B2.1"), "{}", why);
    assert!(!why.contains("among the other nets"), "{}", why);
    let s = p.get("summary");
    assert_eq!((s.get("rounds").f(), s.get("routings").f()), (Some(0.0), Some(2.0)));
}

/// Two channels cross, each too narrow for a via: the net left out has a way but for the other. The
/// two want the same place every round: the seventh time the same jobs come up, the rounds end.
#[test]
fn a_net_left_out_among_the_others_is_said() {
    let p = plan("room.json");
    assert_eq!(p.get("summary").get("rounds").f(), Some(6.0));
    let why = open_note(&p);
    assert!(why.contains("no way was found to") && why.contains("among the other nets"), "{}", why);
    assert!(!why.contains("the board itself"), "{}", why);
}

/// The run's own lines name the open net and the reason, verbose or not.
#[test]
fn the_run_prints_each_open_net_and_why() {
    let dir = std::env::temp_dir().join(format!("astar-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_astar")).args(["route", &scene("walled.json"), dir.join("plan.json").to_str().unwrap()]).output().unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(out.status.code(), Some(1));
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(said.contains("astar: open: B: 1 of 2 pads joined; the board itself leaves no way to B2.1"), "{}", said);
}
