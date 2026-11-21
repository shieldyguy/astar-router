//! Copper already on the board is part of its net: what it joins is joined, and new copper starts and
//! ends on it.
//!
//! The scene (tests/scenes/old_copper.json), 20 x 10 mm, tracks on the front only (the back is G's plane):
//!
//!     A1 ====== A2            A3 ==== (a stub out of A3, to x = 14)        y = 3.03, off the grid
//!     B1                                              B2                   y = 6
//!                    ----  a piece of B no pad stands on, x 9..11          y = 7.537, off the grid
//!     G1 === via                 G3 ========= G2                           y = 9

use astar::json::J;
use astar::{route, Cfg, Scene};

fn scene() -> String {
    format!("{}/tests/scenes/old_copper.json", env!("CARGO_MANIFEST_DIR"))
}

fn plan() -> J {
    let routed = route(Scene::load(&scene()).unwrap(), &Cfg::default()).unwrap();
    assert!(routed.open_nets().is_empty());
    astar::json::parse(&routed.plan()).unwrap()
}

/// A net's new copper: every piece as (x0, y0, x1, y1), and its vias.
fn copper(plan: &J, net: &str) -> (Vec<[f64; 4]>, usize) {
    let n = plan.get("nets").arr().iter().find(|n| n.get("name").s() == Some(net)).unwrap();
    let mut pieces = Vec::new();
    for t in n.get("tracks").arr() {
        for s in t.get("path").arr() {
            let v = if s.get("line").arr().is_empty() { s.get("arc").nums() } else { s.get("line").nums() };
            pieces.push([v[0], v[1], v[v.len() - 2], v[v.len() - 1]]);
        }
    }
    (pieces, n.get("vias").arr().len())
}

fn length(pieces: &[[f64; 4]]) -> f64 {
    pieces.iter().map(|p| (p[2] - p[0]).hypot(p[3] - p[1])).sum()
}

#[test]
fn pads_that_old_copper_joins_get_no_track_and_a_stub_is_gone_on_from() {
    let p = plan();
    // A1 and A2 are joined already; A3's stub ends at x = 14. All that is laid is the 6 mm between the
    // stub's end and A2, on the old copper's own line (which is off the grid)
    let (a, vias) = copper(&p, "A");
    assert_eq!(vias, 0);
    assert!((length(&a) - 6.0).abs() < 0.3, "{:?}", a);
    assert!(a.iter().all(|s| s[0] > 7.6 && s[2] > 7.6 && s[0] < 14.1 && s[2] < 14.1), "{:?}", a);
    assert!(a.iter().all(|s| (s[1] - 3.03).abs() < 1e-6 && (s[3] - 3.03).abs() < 1e-6), "{:?}", a);
    let kept = p.get("notes").arr().iter().find(|n| n.get("kind").s() == Some("kept")).expect("the kept note");
    assert!(kept.get("detail").s().unwrap().contains("5 tracks and 1 vias"));
}

#[test]
fn old_copper_no_pad_stands_on_is_joined_to_its_net() {
    let p = plan();
    // B's own two pads take 16 mm; the piece at y = 7.5 is reached by a branch that ends on it
    let (b, _) = copper(&p, "B");
    // (on its own middle line, which no node stands on: y = 7.537)
    assert!(b.iter().any(|s| [(s[0], s[1]), (s[2], s[3])].iter().any(|e| (e.1 - 7.537).abs() < 1e-6 && e.0 > 8.9 && e.0 < 11.2)), "{:?}", b);
    assert!(length(&b) < 19.0, "{:?}", b);
    assert!(p.get("notes").arr().iter().all(|n| n.get("kind").s() != Some("loose")));
}

#[test]
fn a_plane_pad_old_copper_takes_down_gets_no_via_of_its_own() {
    let p = plan();
    // G1 has a track to a via in the plane: nothing is laid for it. G2 and G3 are joined by a track:
    // one via takes both down, from a point on that track
    let (g, vias) = copper(&p, "G");
    assert_eq!(vias, 1);
    assert!(length(&g) < 1.5 && g.iter().all(|s| s[0] > 11.9 && s[0] < 16.1), "{:?}", g);
}

/// astar's own exact check takes the board's copper with the plan's: the nets are whole and clear.
#[test]
fn the_board_with_its_old_copper_checks_clean() {
    let dir = std::env::temp_dir().join(format!("astar-old-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let plan = dir.join("plan.json");
    let run = |args: &[&str]| std::process::Command::new(env!("CARGO_BIN_EXE_astar")).args(args).output().unwrap();
    let routed = run(&["route", &scene(), plan.to_str().unwrap()]);
    assert_eq!(routed.status.code(), Some(0), "{}", String::from_utf8_lossy(&routed.stderr));
    let checked = run(&["check", &scene(), plan.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(checked.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&checked.stdout), String::from_utf8_lossy(&checked.stderr));
}
