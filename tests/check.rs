//! astar check (src/check/): what it must call clean, and what it must catch, on a board small enough
//! to check by hand. 20 x 10 mm, two layers, track 0.2, clearance 0.2, edge 0.3, hole clearance 0.25:
//!
//!     A1 (2, 2) ......................... A2 (18, 2)     A2 goes through the board
//!     B1 (2, 6) ......................... B2 (18, 6)     a keep-out for tracks at x 8..12, y 4.5..5.2
//!     G1 (5, 8.6)          hole (10, 8.5) d 1             G has the back layer as its plane
//!                                         G2 (15, 8.6)   G2 goes through the board

use astar::check::plan::Plan;
use astar::check::{check, scene_planes, Report};
use astar::Scene;

fn scene_text(extra_net: &str) -> String {
    let sq = |x: f64, y: f64| format!("{{\"poly\": [[{}, {}], [{}, {}], [{}, {}], [{}, {}]]}}", x - 0.5, y - 0.5, x + 0.5, y - 0.5, x + 0.5, y + 0.5, x - 0.5, y + 0.5);
    let pad = |r: &str, n: &str, net: &str, x: f64, y: f64, tht: bool| format!("{{\"ref\": \"{}\", \"num\": \"{}\", \"net\": \"{}\", \"at\": [{}, {}], \"tht\": {}, \"shape\": {}}}", r, n, net, x, y, tht, sq(x, y));
    format!(
        "{{\"version\": 1, \"stack\": {{\"copper\": [\"F.Cu\", \"B.Cu\"], \"planes\": [{{\"net\": \"G\", \"layer\": \"B.Cu\"}}]}},
          \"rules\": {{\"track\": 0.2, \"clearance\": 0.2, \"via\": 0.6, \"drill\": 0.3, \"edge\": 0.3, \"hole_clearance\": 0.25}},
          \"outline\": [{{\"line\": [0, 0, 20, 0]}}, {{\"line\": [20, 0, 20, 10]}}, {{\"line\": [20, 10, 0, 10]}}, {{\"line\": [0, 10, 0, 0]}}],
          \"nets\": [{{\"name\": \"A\"}}, {{\"name\": \"B\"{}}}, {{\"name\": \"G\", \"class\": \"ground\"}}],
          \"pads\": [{}, {}, {}, {}, {}, {}],
          \"holes\": [[10, 8.5, 1.0]],
          \"keepouts\": [{{\"layers\": [\"F.Cu\"], \"poly\": [[8, 4.5], [12, 4.5], [12, 5.2], [8, 5.2]]}}]}}",
        extra_net,
        pad("RA", "1", "A", 2.0, 2.0, false),
        pad("JA", "2", "A", 18.0, 2.0, true),
        pad("RB", "1", "B", 2.0, 6.0, false),
        pad("RB", "2", "B", 18.0, 6.0, false),
        pad("CG", "1", "G", 5.0, 8.6, false),
        pad("JG", "2", "G", 15.0, 8.6, true)
    )
}

/// A track of 0.2 through these points.
fn track(layer: &str, pts: &[(f64, f64)]) -> String {
    let lines: Vec<String> = pts.windows(2).map(|w| format!("{{\"line\": [{}, {}, {}, {}]}}", w[0].0, w[0].1, w[1].0, w[1].1)).collect();
    format!("{{\"layer\": \"{}\", \"width\": 0.2, \"path\": [{}]}}", layer, lines.join(", "))
}

fn net(name: &str, tracks: &[String], vias: &[(f64, f64)]) -> String {
    let v: Vec<String> = vias.iter().map(|(x, y)| format!("{{\"at\": [{}, {}]}}", x, y)).collect();
    format!("{{\"name\": \"{}\", \"tracks\": [{}], \"vias\": [{}]}}", name, tracks.join(", "), v.join(", "))
}

const A: &[(f64, f64)] = &[(2.0, 2.0), (18.0, 2.0)];
const B: &[(f64, f64)] = &[(2.0, 6.0), (18.0, 6.0)];
/// G1 by a stub and a via into the plane; G2 goes through the board and is in the plane as it stands.
fn g() -> String {
    net("G", &[track("F.Cu", &[(5.0, 8.6), (6.5, 8.6)])], &[(6.5, 8.6)])
}

fn run_with(extra_net: &str, nets: &[String], rest: &str) -> Report {
    let text = scene_text(extra_net);
    let sc = Scene::parse(&text).expect("the scene reads");
    let planes = scene_planes(&astar::json::parse(&text).unwrap(), &sc);
    let plan = Plan::from_json(&astar::json::parse(&format!("{{\"version\": 1, \"nets\": [{}]{}}}", nets.join(", "), rest)).unwrap()).expect("the plan reads");
    check(&sc, &plan, &planes)
}

fn run(nets: &[String]) -> Report {
    run_with("", nets, "")
}

fn kinds(r: &Report) -> Vec<&'static str> {
    let mut k: Vec<&'static str> = r.findings.iter().map(|f| f.kind).collect();
    k.dedup();
    k
}

fn said(r: &Report) -> String {
    r.findings.iter().map(|f| f.to_string()).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_right_plan_is_clean() {
    let r = run(&[net("A", &[track("F.Cu", A)], &[]), net("B", &[track("F.Cu", B)], &[]), g()]);
    assert!(r.clean(), "{}", said(&r));
    assert_eq!((r.nets, r.joined), (3, 3));
}

#[test]
fn a_net_without_copper_is_open_and_the_plan_is_held_to_its_word() {
    let nets = [net("A", &[track("F.Cu", A)], &[]), g()];
    let r = run(&nets);
    assert_eq!(kinds(&r), ["open-unsaid"], "{}", said(&r));
    assert!(r.findings[0].net == "B" && r.findings[0].detail.contains("[RB.1] [RB.2]"), "{}", said(&r));
    // the same copper with the plan's own word that B is left out: open, and said
    let r = run_with("", &nets, ", \"left_out\": [{\"net\": \"B\", \"reason\": \"1 of 2 pads joined\"}]");
    assert_eq!(kinds(&r), ["open"], "{}", said(&r));
}

#[test]
fn copper_must_overlap_to_join() {
    // two pieces that end half a millimetre apart
    let r = run(&[net("A", &[track("F.Cu", &[(2.0, 2.0), (9.0, 2.0)]), track("F.Cu", &[(9.5, 2.0), (18.0, 2.0)])], &[]), net("B", &[track("F.Cu", B)], &[]), g()]);
    assert_eq!(kinds(&r), ["open-unsaid"], "{}", said(&r));
    // end on end they are one
    let r = run(&[net("A", &[track("F.Cu", &[(2.0, 2.0), (9.0, 2.0)]), track("F.Cu", &[(9.0, 2.0), (18.0, 2.0)])], &[]), net("B", &[track("F.Cu", B)], &[]), g()]);
    assert!(r.clean(), "{}", said(&r));
    // a track on the back reaches a pad that goes through the board, but only through a via
    let back = [track("F.Cu", &[(2.0, 2.0), (10.0, 2.0)]), track("B.Cu", &[(10.0, 2.0), (18.0, 2.0)])];
    let r = run(&[net("A", &back, &[]), net("B", &[track("F.Cu", B)], &[]), g()]);
    assert_eq!(kinds(&r), ["open-unsaid"], "{}", said(&r));
    let r = run(&[net("A", &back, &[(10.0, 2.0)]), net("B", &[track("F.Cu", B)], &[]), g()]);
    assert!(r.clean(), "{}", said(&r));
}

#[test]
fn a_plane_joins_what_reaches_it() {
    // without its via G1 is not in the plane
    let r = run(&[net("A", &[track("F.Cu", A)], &[]), net("B", &[track("F.Cu", B)], &[]), net("G", &[track("F.Cu", &[(5.0, 8.6), (6.5, 8.6)])], &[])]);
    assert_eq!(kinds(&r), ["open-unsaid"], "{}", said(&r));
    assert_eq!(r.findings[0].net, "G");
}

#[test]
fn an_arc_is_copper() {
    // a quarter turn of radius 2 about (8, 4), from (8, 2) to (10, 4)
    let arc = "{\"layer\": \"F.Cu\", \"width\": 0.2, \"path\": [{\"line\": [2, 2, 8, 2]}, {\"arc\": [8, 2, 9.414214, 2.585786, 10, 4]}, {\"line\": [10, 4, 16, 4]}, {\"line\": [16, 4, 18, 2]}]}".to_string();
    let r = run(&[net("A", &[arc.clone()], &[]), net("B", &[track("F.Cu", B)], &[]), g()]);
    assert!(r.clean(), "{}", said(&r));
    // a stub of B along the outside of the bend, its centre line 2.3 from (8, 4): 0.1 of air to the arc
    let stub = track("F.Cu", &[(9.4263, 2.1737), (9.8263, 2.5737)]);
    let r = run(&[net("A", &[arc], &[]), net("B", &[track("F.Cu", B), stub], &[]), g()]);
    assert_eq!(kinds(&r), ["clearance"], "{}", said(&r));
    assert!(r.findings[0].detail.contains("0.1000 mm"), "{}", said(&r));
}

#[test]
fn a_short_and_a_clearance_error_are_told_apart() {
    // A run down over B1
    let r = run(&[net("A", &[track("F.Cu", A), track("F.Cu", &[(2.0, 2.0), (2.0, 6.0)])], &[]), net("B", &[track("F.Cu", B)], &[]), g()]);
    assert!(kinds(&r).contains(&"short"), "{}", said(&r));
    assert!(r.findings.iter().any(|f| f.kind == "short" && f.detail.contains("pad RB.1")), "{}", said(&r));
    // B beside A with 0.15 of air where 0.2 is asked
    let near = [(2.0, 6.0), (4.0, 6.0), (4.0, 2.35), (16.0, 2.35), (16.0, 6.0), (18.0, 6.0)];
    let r = run(&[net("A", &[track("F.Cu", A)], &[]), net("B", &[track("F.Cu", &near)], &[]), g()]);
    assert_eq!(kinds(&r), ["clearance"], "{}", said(&r));
    assert!(r.findings[0].detail.contains("0.1500 mm") && r.findings[0].detail.contains("asks 0.2000"), "{}", said(&r));
    // with 0.2 of air it is clean, and half a micron short of that still is (KiCad's own slack)
    for y in [2.4, 2.3996] {
        let ok = [(2.0, 6.0), (4.0, 6.0), (4.0, y), (16.0, y), (16.0, 6.0), (18.0, 6.0)];
        let r = run(&[net("A", &[track("F.Cu", A)], &[]), net("B", &[track("F.Cu", &ok)], &[]), g()]);
        assert!(r.clean(), "{}", said(&r));
    }
}

#[test]
fn the_larger_of_two_nets_clearances_holds() {
    // B asks 0.4 of every other net: 0.25 of air beside A is not enough, though the board's rule is 0.2
    let near = [(2.0, 6.0), (4.0, 6.0), (4.0, 2.45), (16.0, 2.45), (16.0, 6.0), (18.0, 6.0)];
    let nets = [net("A", &[track("F.Cu", A)], &[]), net("B", &[track("F.Cu", &near)], &[]), g()];
    assert!(run(&nets).clean());
    let r = run_with(", \"clearance\": 0.4", &nets, "");
    assert_eq!(kinds(&r), ["clearance"], "{}", said(&r));
    assert!(r.findings[0].detail.contains("asks 0.4000"), "{}", said(&r));
}

#[test]
fn holes_the_edge_keep_outs_and_the_outside() {
    let a = net("A", &[track("F.Cu", A)], &[]);
    // B round by the hole: 0.1 from it where 0.25 is asked
    let r = run(&[a.clone(), net("B", &[track("F.Cu", &[(2.0, 6.0), (2.0, 7.8), (18.0, 7.8), (18.0, 6.0)])], &[]), g()]);
    assert!(r.findings.iter().any(|f| f.kind == "hole" && f.detail.contains("0.1000 mm from a hole")), "{}", said(&r));
    // A along the edge with 0.2 to it where 0.3 is asked
    let r = run(&[net("A", &[track("F.Cu", &[(2.0, 2.0), (2.0, 0.3), (18.0, 0.3), (18.0, 2.0)])], &[]), net("B", &[track("F.Cu", B)], &[]), g()]);
    assert_eq!(kinds(&r), ["edge"], "{}", said(&r));
    // A through the keep-out
    let r = run(&[net("A", &[track("F.Cu", &[(2.0, 2.0), (6.0, 2.0), (6.0, 5.0), (14.0, 5.0), (14.0, 2.0), (18.0, 2.0)])], &[]), net("B", &[track("F.Cu", B)], &[]), g()]);
    assert_eq!(kinds(&r), ["keepout"], "{}", said(&r));
    // a via of G off the board
    let r = run(&[a.clone(), net("B", &[track("F.Cu", B)], &[]), net("G", &[track("F.Cu", &[(5.0, 8.6), (6.5, 8.6)])], &[(6.5, 8.6), (25.0, 5.0)])]);
    assert_eq!(kinds(&r), ["outside"], "{}", said(&r));
    // a via's hole too near another net's copper: hole 0.3, so 0.15 + 0.25 from its centre is asked
    let r = run(&[a, net("B", &[track("F.Cu", B)], &[]), net("G", &[track("F.Cu", &[(5.0, 8.6), (6.5, 8.6)])], &[(6.5, 8.6), (10.0, 6.4)])]);
    assert!(r.findings.iter().any(|f| f.kind == "hole" && f.detail.contains("the hole of a via of G")), "{}", said(&r));
}

#[test]
fn a_plan_that_does_not_fit_the_scene() {
    let r = run(&[net("A", &[track("In7.Cu", A)], &[]), net("B", &[track("F.Cu", B)], &[]), g(), net("NOPE", &[track("F.Cu", &[(4.0, 4.0), (5.0, 4.0)])], &[])]);
    let k = kinds(&r);
    assert!(k.contains(&"plan") && k.contains(&"open-unsaid"), "{}", said(&r));
    assert_eq!(r.findings.iter().filter(|f| f.kind == "plan").count(), 2, "{}", said(&r));
}

/// A run checks the plan it wrote before it reports. A net the plan itself says is open is no fault;
/// copper that breaks a rule is, and so is a net called joined that is not.
#[test]
fn a_run_finds_fault_with_its_own_plan() {
    let text = scene_text("");
    let sc = Scene::parse(&text).unwrap();
    let scene_json = astar::json::parse(&text).unwrap();
    let plan = |nets: &[String], rest: &str| format!("{{\"version\": 1, \"nets\": [{}]{}}}", nets.join(", "), rest);
    // right
    let (said, faults) = astar::check::of_run(&scene_json, &sc, &plan(&[net("A", &[track("F.Cu", A)], &[]), net("B", &[track("F.Cu", B)], &[]), g()], ""));
    assert_eq!((said.len(), faults), (0, 0), "{:?}", said);
    // B left out and said so: open, and no fault
    let (said, faults) = astar::check::of_run(&scene_json, &sc, &plan(&[net("A", &[track("F.Cu", A)], &[]), g()], ", \"left_out\": [{\"net\": \"B\"}]"));
    assert_eq!((said.len(), faults), (0, 0), "{:?}", said);
    // B left out and not said; and A laid across B
    let (_, faults) = astar::check::of_run(&scene_json, &sc, &plan(&[net("A", &[track("F.Cu", A)], &[]), g()], ""));
    assert_eq!(faults, 1);
    let (said, faults) = astar::check::of_run(&scene_json, &sc, &plan(&[net("A", &[track("F.Cu", &[(2.0, 2.0), (18.0, 6.0)])], &[]), net("B", &[track("F.Cu", B)], &[]), g()], ""));
    assert!(faults >= 1 && said[0].contains("ITS OWN CHECK FINDS FAULT") && said.iter().any(|l| l.contains("short")), "{:?}", said);
}

/// Plans kept as they were written, each with what the check must find in it: `NAME.scene.json`,
/// `NAME.plan.json` and `NAME.expect` (one kind a line) in tests/fixtures/check/.
#[test]
fn the_kept_plans() {
    let dir = format!("{}/tests/fixtures/check", env!("CARGO_MANIFEST_DIR"));
    let mut n = 0;
    let mut names: Vec<String> = std::fs::read_dir(&dir).expect("tests/fixtures/check").filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_str().and_then(|s| s.strip_suffix(".expect")).map(|s| s.to_string())).collect();
    names.sort();
    for name in names {
        let text = std::fs::read_to_string(format!("{}/{}.scene.json", dir, name)).unwrap();
        let sc = Scene::parse(&text).unwrap();
        let planes = scene_planes(&astar::json::parse(&text).unwrap(), &sc);
        let plan = Plan::load(&format!("{}/{}.plan.json", dir, name)).unwrap();
        let r = check(&sc, &plan, &planes);
        let mut got: Vec<&str> = r.findings.iter().map(|f| f.kind).collect();
        got.sort();
        got.dedup();
        let want = std::fs::read_to_string(format!("{}/{}.expect", dir, name)).unwrap();
        let mut want: Vec<&str> = want.lines().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
        want.sort();
        assert_eq!(got, want, "{}: {}", name, said(&r));
        n += 1;
    }
    assert!(n >= 2, "no kept plans in {}", dir);
}
