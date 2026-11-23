//! A differential pair: two nets laid side by side between the two parts both have a pad on.
//!
//! The scene is one 30 x 12 mm board of two layers with a connector's two pads at the left and a chip's
//! two at the right, DP above DM at both.

use astar::json::J;
use astar::{route, Cfg, Scene};

fn scene() -> J {
    astar::json::parse(&std::fs::read_to_string(format!("{}/tests/scenes/pair.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
}

fn plan(j: &J, cfg: &Cfg) -> J {
    let routed = route(Scene::from_json(j).unwrap(), cfg).unwrap();
    assert!(routed.open_nets().is_empty());
    astar::json::parse(&routed.plan()).unwrap()
}

/// Every straight piece of a net's tracks: (x0, y0, x1, y1).
fn lines(plan: &J, net: &str) -> Vec<Vec<f64>> {
    let n = plan.get("nets").arr().iter().find(|n| n.get("name").s() == Some(net)).unwrap();
    n.get("tracks").arr().iter().flat_map(|t| t.get("path").arr().to_vec()).filter(|s| !s.get("line").arr().is_empty()).map(|s| s.get("line").nums()).collect()
}

fn said(plan: &J) -> String {
    plan.get("notes").arr().iter().find(|n| n.get("kind").s() == Some("pair")).map(|n| n.get("detail").s().unwrap().to_string()).unwrap_or_default()
}

#[test]
fn a_pair_runs_side_by_side_at_its_gap_and_both_nets_are_joined() {
    let p = plan(&scene(), &Cfg::default());
    assert!(said(&p).starts_with("laid as a pair on F.Cu"), "{}", said(&p));
    // the long piece of each net lies along the board, and the two are width + gap apart, middle to middle
    let long = |net: &str| lines(&p, net).into_iter().max_by(|a, b| (a[2] - a[0]).abs().total_cmp(&(b[2] - b[0]).abs())).unwrap();
    let (a, b) = (long("DP"), long("DM"));
    assert!((a[2] - a[0]).abs() > 15.0 && (b[2] - b[0]).abs() > 15.0, "{:?} {:?}", a, b);
    assert!(a[1] == a[3] && b[1] == b[3], "{:?} {:?}", a, b);
    assert!(((b[1] - a[1]) - 0.4).abs() < 1e-6, "DM runs 0.4 mm below DP: {:?} {:?}", a, b);
    // no via: the pair stays on its pads' layer
    assert!(p.get("nets").arr().iter().all(|n| n.get("vias").arr().is_empty()));
}

#[test]
fn a_pair_whose_pads_lie_the_other_way_round_at_one_end_is_routed_as_two_nets_and_the_plan_says_so() {
    let mut j = scene();
    let J::Obj(top) = &mut j else { panic!() };
    let J::Arr(pads) = &mut top.iter_mut().find(|(k, _)| k == "pads").unwrap().1 else { panic!() };
    for q in pads.iter_mut().filter(|q| q.get("ref").s() == Some("U1")) {
        let other = if q.get("net").s() == Some("DP") { "DM" } else { "DP" };
        let J::Obj(o) = q else { panic!() };
        o.iter_mut().find(|(k, _)| k == "net").unwrap().1 = J::Str(other.into());
    }
    let p = plan(&j, &Cfg::default());
    assert!(said(&p).starts_with("routed as two nets"), "{}", said(&p));
}

#[test]
fn skipping_the_pairs_routes_them_as_plain_nets_and_says_nothing() {
    let cfg = Cfg { skip: vec!["pairs".into()], ..Cfg::default() };
    let p = plan(&scene(), &cfg);
    assert_eq!(said(&p), "");
}

#[test]
fn a_pair_the_board_carries_track_of_is_not_laid_again() {
    // the board as a first run left it: the plan's tracks written back as the scene's old copper
    let j = scene();
    let first = plan(&j, &Cfg::default());
    let mut tracks = Vec::new();
    for n in first.get("nets").arr() {
        for t in n.get("tracks").arr() {
            tracks.push(format!(r#"{{"net": "{}", "layer": "{}", "width": {}, "path": {}}}"#, n.get("name").s().unwrap(), t.get("layer").s().unwrap(), t.get("width").f().unwrap(), astar::json::write(t.get("path"))));
        }
    }
    assert!(tracks.len() >= 2);
    let text = astar::json::write(&j);
    let text = format!("{}, \"copper\": {{\"tracks\": [{}]}}}}", text.trim_end().trim_end_matches('}'), tracks.join(", "));
    let again = plan(&astar::json::parse(&text).unwrap(), &Cfg::default());
    assert!(again.get("nets").arr().iter().all(|n| n.get("tracks").arr().is_empty()), "nothing is left to lay");
    assert!(said(&again).contains("carries track of it already"), "{}", said(&again));
}
