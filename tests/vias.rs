//! A net's via is its class's.
//!
//! The scene: 30 x 12 mm, two layers; PWR (class Power, via 1.0 on a 0.5 hole) and SIG (the board's 0.6 on
//! 0.3) each with one pad on the front at the left and one on the back at the right, so each needs a via.

use astar::json::J;
use astar::{route, Cfg, Scene};

fn scene() -> J {
    let pad = |r: &str, net: &str, x: f64, y: f64, side: &str| {
        format!(
            r#"{{"ref": "{r}", "num": "1", "net": "{net}", "at": [{x}, {y}], "fp": [{x}, {y}], "size": [1.0, 1.0], "tht": false, "side": "{side}", "shape": {{"poly": [[{}, {}], [{}, {}], [{}, {}], [{}, {}]], "r": 0}}}}"#,
            x - 0.5,
            y - 0.5,
            x + 0.5,
            y - 0.5,
            x + 0.5,
            y + 0.5,
            x - 0.5,
            y + 0.5
        )
    };
    let pads = [pad("J1", "PWR", 4.0, 8.0, "F"), pad("U1", "PWR", 26.0, 8.0, "B"), pad("J2", "SIG", 4.0, 4.0, "F"), pad("U2", "SIG", 26.0, 4.0, "B")];
    let text = format!(
        r#"{{"version": 2, "units": "mm", "source": {{"name": "vias"}}, "stack": {{"copper": ["F.Cu", "B.Cu"]}},
        "outline": [{{"line": [0, 0, 30, 0]}}, {{"line": [30, 0, 30, 12]}}, {{"line": [30, 12, 0, 12]}}, {{"line": [0, 12, 0, 0]}}],
        "rules": {{"track": 0.2, "clearance": 0.15, "via": 0.6, "drill": 0.3, "edge": 0.3, "hole_clearance": 0.25, "hole_to_hole": 0.25}},
        "classes": [{{"name": "Default", "track": 0.2, "clearance": 0.15}}, {{"name": "Power", "track": 0.4, "clearance": 0.15, "via": 1.0, "drill": 0.5}}],
        "nets": [{{"name": "PWR", "role": "signal", "class": "Power"}}, {{"name": "SIG", "role": "signal"}}],
        "pads": [{}]}}"#,
        pads.join(", ")
    );
    astar::json::parse(&text).unwrap()
}

#[test]
fn a_net_lays_the_via_of_its_class_and_the_board_s_where_its_class_says_none() {
    let j = scene();
    let routed = route(Scene::from_json(&j).unwrap(), &Cfg::default()).unwrap();
    assert!(routed.open_nets().is_empty());
    let plan = astar::json::parse(&routed.plan()).unwrap();
    let vias = |net: &str| -> Vec<(f64, f64)> {
        let n = plan.get("nets").arr().iter().find(|n| n.get("name").s() == Some(net)).unwrap();
        n.get("vias").arr().iter().map(|v| (v.get("size").f().unwrap(), v.get("drill").f().unwrap())).collect()
    };
    assert_eq!(vias("PWR"), vec![(1.0, 0.5)]);
    assert_eq!(vias("SIG"), vec![(0.6, 0.3)]);
    // astar's own check, which reads a via's size off the plan and its room off the scene, finds nothing
    let report = astar::check::check(&Scene::from_json(&j).unwrap(), &astar::check::plan::Plan::from_json(&plan).unwrap(), &[]);
    assert!(report.findings.is_empty(), "{:?}", report.findings.iter().map(|f| (f.kind, f.net.clone())).collect::<Vec<_>>());
}
