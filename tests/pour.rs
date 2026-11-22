//! How a pour is used: copper to go down into, crossed only where the way round is dearer; and what a
//! ground gets when the board draws none for it.
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

fn wired() -> Cfg {
    Cfg { ground_tracks: true, ..Cfg::default() }
}

#[test]
fn a_board_poured_on_every_layer_is_priced_as_one_with_no_pour() {
    // with GND poured on both layers no layer is the better one to cross on: SIG runs as it does on the
    // bare board
    let (both, none) = (plan("pour_both.json", &Cfg::default()), plan("pour_none.json", &wired()));
    for layer in ["F.Cu", "B.Cu"] {
        assert_eq!(copper(&both, "SIG", layer), copper(&none, "SIG", layer));
    }
}

#[test]
fn a_ground_the_board_draws_nothing_for_gets_a_pour_on_a_board_of_two_layers() {
    // the parts are on the front: the pour goes on the back, and the board is routed as the one whose
    // designer drew that pour
    let (p, drawn) = (plan("pour_none.json", &Cfg::default()), plan("pour_bottom.json", &Cfg::default()));
    let planes = p.get("planes").arr();
    assert_eq!(planes.len(), 1);
    assert_eq!((planes[0].get("net").s(), planes[0].get("layer").s(), planes[0].get("pour").b(), planes[0].get("pads").s()), (Some("GND"), Some("B.Cu"), true, Some("thermal")));
    let (front, vias) = copper(&p, "GND", "F.Cu");
    assert_eq!(vias, 4);
    assert!(front < 4.0 && copper(&p, "GND", "B.Cu").0 == 0.0, "{} mm on the front", front);
    assert!(copper(&p, "SIG", "B.Cu").0 < 2.5 && copper(&drawn, "SIG", "B.Cu").0 < 2.5);
    let n = note(&p, "planned").expect("the planned note");
    assert_eq!(n.get("net").s(), Some("GND"));
    assert!(note(&p, "ground").is_none());
    // astar's own check takes the pour as the board's
    let scene = Scene::load(&format!("{}/tests/scenes/pour_none.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let report = astar::check::check(&scene, &astar::check::plan::Plan::from_json(&p).unwrap(), &[]);
    assert!(report.findings.is_empty(), "{:?}", report.findings.iter().map(|f| (f.kind, f.net.clone())).collect::<Vec<_>>());
}

#[test]
fn a_ground_can_be_wired_as_a_net_and_that_is_said() {
    let p = plan("pour_none.json", &wired());
    assert!(p.get("planes").arr().is_empty() && note(&p, "planned").is_none());
    let n = note(&p, "ground").expect("the ground note");
    assert_eq!(n.get("net").s(), Some("GND"));
    assert!(copper(&p, "GND", "F.Cu").0 + copper(&p, "GND", "B.Cu").0 > 20.0);
}

/// every_field.json, four layers, with the ground plane its designer drew taken off.
fn four_layers_no_ground() -> Scene {
    let text = std::fs::read_to_string(format!("{}/tests/scenes/every_field.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut j = astar::json::parse(&text).unwrap();
    let empty = |j: &mut J, a: &str, b: &str| {
        let J::Obj(top) = j else { panic!() };
        let J::Obj(inner) = &mut top.iter_mut().find(|(k, _)| k == a).unwrap().1 else { panic!() };
        inner.iter_mut().find(|(k, _)| k == b).unwrap().1 = J::Arr(Vec::new());
    };
    empty(&mut j, "stack", "planes");
    empty(&mut j, "copper", "zones");
    Scene::from_json(&j).unwrap()
}

#[test]
fn a_ground_the_board_draws_nothing_for_gets_the_first_free_inner_layer() {
    let planes_of = |cfg: &Cfg| -> Vec<(String, String, bool)> {
        let routed = route(four_layers_no_ground(), cfg).unwrap();
        let p = astar::json::parse(&routed.plan()).unwrap();
        p.get("planes").arr().iter().map(|q| (q.get("net").s().unwrap().to_string(), q.get("layer").s().unwrap().to_string(), q.get("pour").b())).collect()
    };
    let one = |n: &str, l: &str, pour: bool| (n.to_string(), l.to_string(), pour);
    // the ground first, then the rail on the next
    let auto = planes_of(&Cfg { two_ways: false, ..Cfg::default() });
    assert_eq!(auto, vec![one("GND", "In1.Cu", false), one("VCC", "In2.Cu", false)]);
    // said outright: the ground on the other inner layer and a pour of it on the back besides
    let said = planes_of(&Cfg { planes: vec![("GND".into(), vec!["In2.Cu".into(), "B.Cu".into()])], pour_pads: 0, ..Cfg::default() });
    assert_eq!(said, vec![one("GND", "In2.Cu", false), one("GND", "B.Cu", true)]);
    // wired: the ground gets nothing, the rail the free layer as before
    let wired = planes_of(&Cfg { two_ways: false, ..wired() });
    assert_eq!(wired, vec![one("VCC", "In1.Cu", false)]);
    // what cannot be: a net the board does not have, a layer it does not have, two nets on one layer
    for bad in [vec![("NOPE", "In1.Cu")], vec![("GND", "In7.Cu")], vec![("GND", "In1.Cu"), ("VCC", "In1.Cu")]] {
        let cfg = Cfg { planes: bad.iter().map(|(n, l)| (n.to_string(), vec![l.to_string()])).collect(), ..Cfg::default() };
        assert!(route(four_layers_no_ground(), &cfg).is_err(), "{:?}", bad);
    }
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

/// A plane meets its net's through-hole pins by reliefs, and KiCad wants two spokes of a relief to
/// reach the fill. J1.2 is a ground pin whose spokes three neighbours' pins cut (KiCad: one left, a
/// starved relief). It gets short tracks of its own on the plane's layer, each a different way, which
/// KiCad counts as spokes; a pin with room all round gets none.
#[test]
fn a_plane_pin_with_too_few_spokes_gets_tracks_of_its_own_on_the_plane_layer() {
    let p = plan("plane_relief.json", &Cfg::default());
    let stubs: Vec<Vec<f64>> = p.get("nets").arr().iter().filter(|n| n.get("name").s() == Some("GND")).flat_map(|n| n.get("tracks").arr().to_vec()).filter(|t| t.get("layer").s() == Some("In1.Cu")).map(|t| t.get("path").arr()[0].get("line").arr().iter().map(|v| v.f().unwrap()).collect()).collect();
    assert_eq!(stubs.len(), 2, "{:?}", stubs);
    let way = |s: &Vec<f64>| {
        let (dx, dy) = (s[2] - s[0], s[3] - s[1]);
        let l = (dx * dx + dy * dy).sqrt();
        (dx / l, dy / l)
    };
    for s in &stubs {
        assert_eq!((s[0], s[1]), (10.0, 6.0), "from the pin's middle");
    }
    let (a, b) = (way(&stubs[0]), way(&stubs[1]));
    assert!(a.0 * b.0 + a.1 * b.1 < 0.7, "two ways, not one track twice: {:?}", stubs);
    // without the neighbours the relief has its four spokes and nothing is laid
    let text = std::fs::read_to_string(format!("{}/tests/scenes/plane_relief.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut j = astar::json::parse(&text).unwrap();
    let J::Obj(top) = &mut j else { panic!() };
    let J::Arr(pads) = &mut top.iter_mut().find(|(k, _)| k == "pads").unwrap().1 else { panic!() };
    pads.retain(|q| q.get("net").s() == Some("GND"));
    let routed = route(Scene::from_json(&j).unwrap(), &Cfg::default()).unwrap();
    let free = astar::json::parse(&routed.plan()).unwrap();
    assert!(free.get("nets").arr().iter().all(|n| n.get("tracks").arr().iter().all(|t| t.get("layer").s() != Some("In1.Cu"))));
}
