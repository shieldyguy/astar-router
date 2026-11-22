//! A net kept to layers runs there: `--route-on NET=LAYER`, or the net's own `route_on` in the scene. A pad of
//! the net on another layer is left by a stub to a via, no longer than `Cfg::stub_mm`, and as short as
//! there is room for.
//!
//! The scene is tests/scenes/tiny11.json: one net, two pads on the front, 6.1 mm apart.

use astar::json::J;
use astar::{route, Cfg, Scene};

fn scene() -> String {
    format!("{}/tests/scenes/tiny11.json", env!("CARGO_MANIFEST_DIR"))
}

/// The net's copper by layer: mm of track on each, and its vias.
fn copper(plan: &J) -> (f64, f64, usize) {
    let n = &plan.get("nets").arr()[0];
    let (mut front, mut back) = (0.0, 0.0);
    for t in n.get("tracks").arr() {
        let mm: f64 = t.get("path").arr().iter().filter(|s| s.has("line")).map(|s| s.get("line").nums()).map(|v| (v[2] - v[0]).hypot(v[3] - v[1])).sum();
        match t.get("layer").s() {
            Some("F.Cu") => front += mm,
            _ => back += mm,
        }
    }
    (front, back, n.get("vias").arr().len())
}

#[test]
fn a_net_kept_to_the_back_runs_there_and_leaves_its_pads_by_short_stubs() {
    // as it comes: straight across the front
    let plain = astar::json::parse(&route(Scene::load(&scene()).unwrap(), &Cfg::default()).unwrap().plan()).unwrap();
    assert_eq!((copper(&plain).1, copper(&plain).2), (0.0, 0));
    // kept to the back: a stub from each pad to a via, the rest on the back
    let cfg = Cfg { route_on: vec![("SIG".into(), vec!["B.Cu".into()])], ..Cfg::default() };
    let routed = route(Scene::load(&scene()).unwrap(), &cfg).unwrap();
    assert!(routed.open_nets().is_empty());
    let plan = astar::json::parse(&routed.plan()).unwrap();
    let (front, back, vias) = copper(&plan);
    assert_eq!(vias, 2);
    assert!(back > 3.0, "{} mm on the back", back);
    assert!(front < 2.0 * 1.3, "{} mm on the front: two stubs, each as short as a via beside the pad allows", front);
    let note = plan.get("notes").arr().iter().find(|n| n.get("kind").s() == Some("route on")).expect("the plan says how it came out");
    assert!(note.get("detail").s().unwrap().starts_with("kept to B.Cu: "), "{:?}", note.get("detail").s());
}

#[test]
fn the_scenes_own_word_does_the_same() {
    // the same board as a scene of its own, the net saying which layers it keeps to
    let text = r#"{"version": 2, "units": "mm", "stack": {"copper": ["F.Cu", "B.Cu"]},
        "outline": [{"line": [0, 0, 10, 0]}, {"line": [10, 0, 10, 6]}, {"line": [10, 6, 0, 6]}, {"line": [0, 6, 0, 0]}],
        "rules": {"track": 0.2, "clearance": 0.15, "via": 0.6, "drill": 0.3, "edge": 0.5, "hole_clearance": 0.25},
        "nets": [{"name": "SIG", "route_on": ["B.Cu"]}],
        "pads": [{"ref": "R1", "num": "1", "net": "SIG", "at": [2, 3], "shape": {"poly": [[1.6, 2.55], [2.4, 2.55], [2.4, 3.45], [1.6, 3.45]], "r": 0}},
                 {"ref": "R2", "num": "2", "net": "SIG", "at": [8, 4], "shape": {"poly": [[7.6, 3.55], [8.4, 3.55], [8.4, 4.45], [7.6, 4.45]], "r": 0}}]}"#;
    let dir = std::env::temp_dir().join(format!("astar-route-on-scene-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("scene.json");
    std::fs::write(&file, text).unwrap();
    let sc = Scene::load(file.to_str().unwrap());
    std::fs::remove_dir_all(&dir).ok();
    let plan = astar::json::parse(&route(sc.unwrap(), &Cfg::default()).unwrap().plan()).unwrap();
    assert_eq!(copper(&plan).2, 2);
}

/// A stub too short to reach any via: the net stays open, and says that its own rule is what shuts it in.
#[test]
fn a_rule_that_cannot_be_kept_is_named() {
    let cfg = Cfg { route_on: vec![("SIG".into(), vec!["B.Cu".into()])], stub_mm: 0.1, ..Cfg::default() };
    let routed = route(Scene::load(&scene()).unwrap(), &cfg).unwrap();
    assert_eq!(routed.open_nets().len(), 1);
    let plan = astar::json::parse(&routed.plan()).unwrap();
    let why = plan.get("notes").arr().iter().find(|n| n.get("kind").s() == Some("open")).unwrap().get("detail").s().unwrap().to_string();
    assert!(why.contains("is shut in by its own layer rule (it is kept to B.Cu)"), "{}", why);
    // a layer the board does not have is refused
    let cfg = Cfg { route_on: vec![("SIG".into(), vec!["In9.Cu".into()])], ..Cfg::default() };
    let e = route(Scene::load(&scene()).unwrap(), &cfg).err().expect("no such layer");
    assert!(e.to_string().contains("the board has no copper layer In9.Cu"), "{}", e);
}

/// astar's own check holds a plan to the rule the plan itself records (`route_on`): the plan as routed is
/// clean, and the same copper under a shorter stub than it used is a finding.
#[test]
fn the_check_holds_the_plan_to_its_rule() {
    let cfg = Cfg { route_on: vec![("SIG".into(), vec!["B.Cu".into()])], ..Cfg::default() };
    let plan = route(Scene::load(&scene()).unwrap(), &cfg).unwrap().plan();
    assert!(plan.contains("\"route_on\": [\n  {\"net\": \"SIG\", \"layers\": [\"B.Cu\"], \"stub\": 2.5}"), "{}", &plan[..plan.len().min(600)]);
    let dir = std::env::temp_dir().join(format!("astar-route-on-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (good, bad) = (dir.join("good.json"), dir.join("bad.json"));
    std::fs::write(&good, &plan).unwrap();
    std::fs::write(&bad, plan.replace("\"stub\": 2.5", "\"stub\": 0.2")).unwrap();
    let run = |file: &std::path::Path| std::process::Command::new(env!("CARGO_BIN_EXE_astar")).args(["check", &scene(), file.to_str().unwrap()]).output().unwrap();
    let (ok, found) = (run(&good), run(&bad));
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(ok.status.code(), Some(0), "{}", String::from_utf8_lossy(&ok.stdout));
    assert_eq!(found.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&found.stdout).contains("layer SIG"), "{}", String::from_utf8_lossy(&found.stdout));
}
