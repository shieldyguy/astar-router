//! How copper is drawn: a net keeps its width unless it has to give it up, and a long pad is left by an end.

use astar::{route, Cfg, Scene};

fn fixture(name: &str) -> String {
    format!("{}/tests/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

/// The plan's tracks: (net, width, the points of its path in order).
fn tracks(plan: &str) -> Vec<(String, f64, Vec<(f64, f64)>)> {
    let plan = astar::json::parse(plan).unwrap();
    let mut out = Vec::new();
    for n in plan.get("nets").arr() {
        for t in n.get("tracks").arr() {
            let mut pts = Vec::new();
            for s in t.get("path").arr() {
                let piece = if s.get("line").arr().is_empty() { s.get("arc") } else { s.get("line") };
                let v: Vec<f64> = piece.arr().iter().map(|x| x.f().unwrap()).collect();
                pts.extend(v.chunks(2).map(|c| (c[0], c[1])));
            }
            out.push((n.get("name").s().unwrap().to_string(), t.get("width").f().unwrap(), pts));
        }
    }
    out
}

#[test]
fn a_net_goes_round_rather_than_neck() {
    let routed = route(Scene::load(&fixture("neck_round.json")).unwrap(), &Cfg::default()).unwrap();
    assert!(routed.open_nets().is_empty());
    let plan = routed.plan();
    // the gap in the wall is straight ahead and a neck would fit through it: the net goes round the wall
    let t = tracks(&plan);
    assert!(t.iter().all(|t| (t.1 - 0.3).abs() < 1e-9), "{:?}", t);
    assert!(t.iter().any(|t| t.2.iter().any(|p| p.1 > 5.6)), "{:?}", t);
    assert_eq!(routed.exit_code(true), astar::exit::WHOLE);
}

#[test]
fn a_net_necks_where_nothing_else_joins_it() {
    let routed = route(Scene::load(&fixture("neck_must.json")).unwrap(), &Cfg::default()).unwrap();
    assert!(routed.open_nets().is_empty());
    let t = tracks(&routed.plan());
    // through the gap as wide as the gap leaves (0.5 mm between the walls, less the clearance of 0.15 on
    // both sides: 0.2, where the fab's floor is 0.127), and at its own width on both sides of it
    let neck: Vec<_> = t.iter().filter(|t| t.1 < 0.3 - 1e-9).collect();
    assert_eq!(neck.len(), 1, "{:?}", t);
    assert!((neck[0].1 - 0.2).abs() < 1e-9, "{:?}", neck);
    assert!(neck[0].2.iter().all(|p| (p.0 - 6.0).abs() < 1.0), "{:?}", neck);
    assert_eq!(t.iter().filter(|t| (t.1 - 0.3).abs() < 1e-9).count(), 2, "{:?}", t);
    assert_eq!(routed.exit_code(true), astar::exit::NARROWED);
}

#[test]
fn a_long_pad_is_left_by_an_end() {
    let routed = route(Scene::load(&fixture("exit_end.json")).unwrap(), &Cfg::default()).unwrap();
    assert!(routed.open_nets().is_empty());
    // J's three pads stand in a row, each 0.6 by 2 mm, long in y, centres at y = 4; every net's other pad
    // lies off to the side. No track crosses a long side: where a track is beside a pad of J (within its
    // length), it is on that pad's middle line
    for (net, _, pts) in tracks(&routed.plan()) {
        let x = match net.as_str() {
            "A" => 3.0,
            "SIG" => 4.0,
            _ => 5.0,
        };
        for w in pts.windows(2) {
            for k in 0..=20 {
                let p = (w[0].0 + (w[1].0 - w[0].0) * k as f64 / 20.0, w[0].1 + (w[1].1 - w[0].1) * k as f64 / 20.0);
                if (p.0 - x).abs() < 0.3 + 0.2 && (p.1 - 4.0).abs() < 0.95 {
                    assert!((p.0 - x).abs() < 0.051, "net {} is at ({:.2}, {:.2}), beside its pad at x = {}", net, p.0, p.1, x);
                }
            }
        }
    }
}

#[test]
fn a_track_ends_on_its_pads_line_whatever_the_grid() {
    // two rows of pins, 0.3 by 1.5 mm, long in y; the upper row's lie 0.03 mm off multiples of 0.1 in x,
    // the lower row's 0.07: the grid cannot stand on both. Every pin's track runs out along the pin's own
    // middle line all the same, and where it leaves that line it turns: it does not step over to a grid
    // line beside it
    let routed = route(Scene::load(&fixture("true_ends.json")).unwrap(), &Cfg { radius_mm: 0.0, ..Cfg::default() }).unwrap();
    assert!(routed.open_nets().is_empty());
    let plan = routed.plan();
    let notes = astar::json::parse(&plan).unwrap();
    assert!(notes.get("notes").arr().iter().all(|n| !matches!(n.get("kind").s(), Some("ends") | Some("narrowed"))), "{}", plan);
    let pins = [("A", 6.03, 3.0), ("B", 6.53, 3.0), ("C", 7.03, 3.0), ("D", 6.07, 9.0), ("E", 6.57, 9.0), ("F", 7.07, 9.0)];
    let t = tracks(&plan);
    for (net, x, y) in pins {
        let at_pin = |p: &(f64, f64)| (p.0 - x).abs() < 1e-9 && (p.1 - y).abs() < 1e-9;
        let (_, _, pts) = t.iter().find(|t| t.0 == net && (at_pin(&t.2[0]) || at_pin(&t.2[t.2.len() - 1]))).unwrap_or_else(|| panic!("no track of {} ends at its pin's centre: {:?}", net, t));
        // the track's points from the pin on, each once
        let mut from: Vec<(f64, f64)> = if at_pin(&pts[0]) { pts.clone() } else { pts.iter().rev().copied().collect() };
        from.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
        let k = from.iter().position(|p| (p.0 - x).abs() > 1e-9).unwrap_or_else(|| panic!("net {} never leaves its pin's line: {:?}", net, from));
        let (corner, next) = (from[k - 1], from[k]);
        assert!((corner.1 - y).abs() > 0.75, "net {} leaves its pin's line at y = {}, beside the pin", net, corner.1);
        let (dx, dy) = ((next.0 - corner.0).abs(), (next.1 - corner.1).abs());
        assert!(dx > 0.5 * dx.hypot(dy), "net {} steps from ({}, {}) to ({}, {}): a jog, not a turn", net, corner.0, corner.1, next.0, next.1);
    }
}
