//! The library: a scene routed in a few lines, and what comes back.

use astar::{route, Cfg, Error, Scene};

fn fixture(name: &str) -> String {
    format!("{}/tests/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn a_scene_is_routed_through_the_library() {
    let scene = Scene::load(&fixture("every_field.json")).unwrap();
    let routed = route(scene, &Cfg::default()).unwrap();
    assert!(routed.open_nets().is_empty());
    assert_eq!(routed.exit_code(false), astar::exit::WHOLE);
    assert_eq!(routed.exit_code(true), astar::exit::NARROWED);
    let plan = astar::json::parse(&routed.plan()).unwrap();
    assert_eq!(plan.get("nets").arr().len(), 6);
    assert_eq!(plan.get("planes").arr().len(), 1);
    assert!(plan.get("notes").arr().iter().any(|n| n.get("kind").s() == Some("narrowed") && n.get("net").s() == Some("VCC")));
    assert!(routed.seconds > 0.0);
    // the same scene again: the same plan, but for its seconds
    let again = route(Scene::load(&fixture("every_field.json")).unwrap(), &Cfg::default()).unwrap();
    let strip = |p: String| p[..p.find("\"timing_s\"").unwrap()].to_string() + &p[p.find("\"summary\"").unwrap()..];
    assert_eq!(strip(routed.plan()), strip(again.plan()));
}

#[test]
fn what_cannot_be_taken_is_an_error_in_words() {
    let Err(Error::BadInput(e)) = Scene::load("/no/such/file.json") else { panic!("a missing file loaded") };
    assert!(e.contains("/no/such/file.json") && e.contains("cannot read"), "{}", e);
    let Err(Error::BadInput(e)) = Scene::parse("{\"version\": 2, \"colour\": 1}") else { panic!("an unknown key loaded") };
    assert!(e.contains("unknown key \"colour\""), "{}", e);
    let scene = Scene::load(&fixture("tiny11.json")).unwrap();
    let Err(e) = route(scene.clone(), &Cfg { pitch: 5.0, ..Cfg::default() }) else { panic!("a pitch of 5 mm was taken") };
    assert!(e.to_string().contains("the pitch is 5 mm"), "{}", e);
    let Err(e) = route(scene, &Cfg { max_cells: 1000, ..Cfg::default() }) else { panic!("a grid past the limit was built") };
    assert!(e.to_string().contains("a coarser --pitch"), "{}", e);
}
