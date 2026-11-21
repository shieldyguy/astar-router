//! How a pour is used: copper to go down into, crossed only where the way round is dearer; and what is
//! said when a board has none for its ground.
//!
//! The scenes are one 20 x 10 mm board of two layers: a wall down the middle of the front that SIG can
//! pass only on the back, and GND's four pads in the corners of the front.

use astar::json::J;
use astar::{route, Cfg, Scene};

fn plan(name: &str, cfg: &Cfg) -> J {
    let routed = route(Scene::load(&format!("{}/tests/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)).unwrap(), cfg).unwrap();
    assert!(routed.open_nets().is_empty());
    astar::json::parse(&routed.plan()).unwrap()
}

/// A net's track length on one layer, mm, and how many vias it has.
fn copper(plan: &J, net: &str, layer: &str) -> (f64, usize) {
    let n = plan.get("nets").arr().iter().find(|n| n.get("name").s() == Some(net)).unwrap();
    let mut mm = 0.0;
    for t in n.get("tracks").arr().iter().filter(|t| t.get("layer").s() == Some(layer)) {
        for s in t.get("path").arr() {
            let v = if s.get("line").arr().is_empty() { s.get("arc").nums() } else { s.get("line").nums() };
            mm += (v[v.len() - 2] - v[0]).hypot(v[v.len() - 1] - v[1]);
        }
    }
    (mm, n.get("vias").arr().len())
}

fn note<'a>(plan: &'a J, kind: &str) -> Option<&'a J> {
    plan.get("notes").arr().iter().find(|n| n.get("kind").s() == Some(kind))
}

#[test]
fn a_pour_is_gone_down_into_and_crossed_no_further_than_it_must_be() {
    let p = plan("pour_bottom.json", &Cfg::default());
    // GND is poured on the back: each of its pads goes down to it by a via beside itself, and no pad is
    // wired to another
    let (front, vias) = copper(&p, "GND", "F.Cu");
    assert_eq!(vias, 4);
    assert!(front < 4.0 && copper(&p, "GND", "B.Cu").0 == 0.0, "{} mm on the front", front);
    // SIG crosses the wall on the back and comes straight up again: it does not run on across the pour
    let (back, _) = copper(&p, "SIG", "B.Cu");
    assert!(back < 2.5, "{} mm of SIG on the back", back);
    assert!(note(&p, "ground").is_none());
}

#[test]
fn a_board_poured_on_every_layer_is_priced_as_one_with_no_pour() {
    // with GND poured on both layers no layer is the better one to cross on: SIG runs as it does on the
    // bare board
    let (both, none) = (plan("pour_both.json", &Cfg::default()), plan("pour_none.json", &Cfg::default()));
    for layer in ["F.Cu", "B.Cu"] {
        assert_eq!(copper(&both, "SIG", layer), copper(&none, "SIG", layer));
    }
}

#[test]
fn a_ground_with_no_pour_is_wired_and_said() {
    let p = plan("pour_none.json", &Cfg::default());
    let n = note(&p, "ground").expect("the ground note");
    assert_eq!(n.get("net").s(), Some("GND"));
    assert!(copper(&p, "GND", "F.Cu").0 + copper(&p, "GND", "B.Cu").0 > 20.0);
}

#[test]
fn a_net_joined_off_the_board_gets_a_via_a_pad_and_nothing_else() {
    let cfg = Cfg { drops: vec!["GND".to_string()], ..Cfg::default() };
    let p = plan("pour_none.json", &cfg);
    let planes = p.get("planes").arr();
    assert_eq!(planes.len(), 1);
    assert_eq!((planes[0].get("net").s(), planes[0].get("layer").s(), planes[0].get("host").b()), (Some("GND"), Some("B.Cu"), true));
    let (front, vias) = copper(&p, "GND", "F.Cu");
    assert_eq!(vias, 4);
    assert!(front < 4.0 && copper(&p, "GND", "B.Cu").0 == 0.0);
    assert!(note(&p, "drops").is_some() && note(&p, "ground").is_none());
    // the host's pour is under the whole board: SIG crosses it no further than it must, as it does a
    // pour drawn on the board
    assert!(copper(&p, "SIG", "B.Cu").0 < 2.5);
    // a net the board does not have is refused
    let bad = Cfg { drops: vec!["NOPE".to_string()], ..Cfg::default() };
    assert!(route(Scene::load(&format!("{}/tests/scenes/pour_none.json", env!("CARGO_MANIFEST_DIR"))).unwrap(), &bad).is_err());
}

/// The vias of a net joined off the board stand on a layer that carries other nets' tracks (a plane's own
/// layer carries none): astar's exact check finds them legal.
#[test]
fn the_drops_of_a_hosted_net_are_legal() {
    let dir = std::env::temp_dir().join(format!("astar-pour-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (scene, plan) = (format!("{}/tests/scenes/pour_none.json", env!("CARGO_MANIFEST_DIR")), dir.join("plan.json"));
    let run = |args: &[&str]| std::process::Command::new(env!("CARGO_BIN_EXE_astar")).args(args).output().unwrap();
    let routed = run(&["route", &scene, plan.to_str().unwrap(), "--drops", "GND"]);
    assert_eq!(routed.status.code(), Some(0), "{}", String::from_utf8_lossy(&routed.stderr));
    let checked = run(&["check", &scene, plan.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(checked.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&checked.stdout), String::from_utf8_lossy(&checked.stderr));
}
