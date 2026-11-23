//! The scene reader: 1.1 as it was, scene 2 with every field, and what it refuses.

use astar::json::{self, J};
use astar::scene::{Scene, FORBID_COPPER};

fn text(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)).expect("the fixture is there")
}

fn load(t: &str) -> Result<Scene, String> {
    Scene::from_json(&json::parse(t)?)
}

/// The fixture with one key set (or added) in the object at `path` ("" for the top).
fn with(t: &str, path: &[&str], key: &str, val: &str) -> String {
    fn go(j: &mut J, path: &[&str], key: &str, val: J) {
        if let Some((first, rest)) = path.split_first() {
            match j {
                J::Obj(v) => go(&mut v.iter_mut().find(|(k, _)| k == first).expect("the path is in the fixture").1, rest, key, val),
                J::Arr(v) => go(&mut v[first.parse::<usize>().unwrap()], rest, key, val),
                _ => panic!("the path leaves the fixture"),
            }
        } else if let J::Obj(v) = j {
            v.retain(|(k, _)| k != key);
            v.push((key.to_string(), val));
        }
    }
    let mut j = json::parse(t).unwrap();
    go(&mut j, path, key, json::parse(val).unwrap());
    json::write(&j)
}

#[test]
fn scene_1_1_loads_as_before() {
    let sc = load(&text("tiny11.json")).unwrap();
    assert_eq!(sc.version, 1);
    assert_eq!((sc.layers.len(), sc.nets.len(), sc.pads.len(), sc.outline.len()), (2, 2, 2, 4));
    assert!(sc.carried_unused().is_empty());
    // 1.1 passes over what it does not know, as it always did
    assert!(load(&with(&text("tiny11.json"), &[], "colour", "\"green\"")).is_ok());
}

#[test]
fn scene_2_loads_with_every_field() {
    let sc = load(&text("every_field.json")).unwrap();
    assert_eq!(sc.version, 2);
    assert_eq!(sc.stackup.len(), 9);
    assert_eq!(sc.stackup[2].epsilon_r, Some(4.4));
    assert_eq!(sc.rules.hole_to_hole, Some(0.25));
    assert_eq!(sc.rules.floors.annular, Some(0.1));
    assert_eq!((sc.rules.layer_rules.len(), sc.rules.layer_rules[0].layer), (1, 2));
    assert_eq!(sc.classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Default", "Power", "Pair90"]);
    assert_eq!((sc.classes[1].via, sc.classes[1].layer_rules.len()), (Some(0.8), 1));
    assert!(sc.classes[2].impedance.as_ref().unwrap().differential);
    assert_eq!((sc.class_pairs.len(), sc.class_pairs[0].a, sc.class_pairs[0].clearance), (1, 1, 0.2));
    let net = |name: &str| sc.nets.iter().find(|n| n.name == name).unwrap();
    assert_eq!((net("VCC").netclass, net("VCC").width), (1, 0.4));
    assert_eq!((net("AUX").via, net("AUX").impedance.as_ref().unwrap().ohms), (Some(0.6), 50.0));
    assert_eq!(net("GND").plane, Some(1));
    assert!(sc.pads.iter().any(|p| p.clearance == 0.2) && sc.pads.iter().any(|p| p.drill == Some(1.0)) && sc.pads.iter().any(|p| p.back));
    assert!(sc.keepouts[0].forbid.vias && !sc.keepouts[0].forbid.tracks && sc.keepouts[0].forbid != FORBID_COPPER);
    assert_eq!((sc.copper.tracks.len(), sc.copper.vias.len(), sc.copper.zones.len()), (1, 2, 1));
    assert!(sc.copper.tracks[0].locked && sc.copper.tracks[0].path.len() == 2);
    assert_eq!(sc.copper.vias[1].span, Some((2, 3)));
    assert_eq!((sc.copper.zones[0].fill.len(), sc.copper.zones[0].fill[0].holes.len()), (1, 1));
    assert_eq!((sc.parts.len(), sc.pairs.len(), sc.matches.len(), sc.groups.len()), (7, 1, 2, 1));
    assert!(sc.parts.iter().any(|p| p.fixed) && sc.matches[1].by_time && sc.matches[1].target == Some(25.0));
    assert_eq!(sc.groups[0].parts, [0, 3, 4]);
    assert!(sc.nets.iter().skip(1).all(|n| n.in_scope) && sc.parts.iter().all(|p| p.in_scope));
    assert_eq!(sc.carried_unused().len(), 7);
}

#[test]
fn scene_2_scope_names_what_is_in() {
    let sc = load(&with(&text("every_field.json"), &["scope"], "nets", "[\"SIG\"]")).unwrap();
    assert_eq!(sc.nets.iter().filter(|n| n.in_scope).map(|n| n.name.as_str()).collect::<Vec<_>>(), ["", "SIG"]);
}

#[test]
fn scene_2_names_the_key_it_does_not_know() {
    let t = text("every_field.json");
    for (path, at) in [(&[][..], "scene"), (&["nets", "1"][..], "nets[1]"), (&["rules", "floors"][..], "rules.floors"), (&["copper", "zones", "0"][..], "copper.zones[0]"), (&["pads", "3", "shape"][..], "shape")] {
        let e = load(&with(&t, path, "colour", "1")).err().unwrap_or_else(|| panic!("an unknown key in {} went through", at));
        assert!(e.contains("unknown key \"colour\"") && e.contains(at), "{}", e);
    }
}

#[test]
fn scene_2_refuses_what_is_wrong() {
    let t = text("every_field.json");
    for (path, key, val, says) in [
        (&["rules"][..], "track", "-0.2", "rules.track"),
        (&["rules"][..], "via", "\"big\"", "must be a number"),
        (&["nets", "1"][..], "class", "\"signal\"", "is not in classes"),
        (&["nets", "1"][..], "name", "\"GND\"", "twice"),
        (&["pads", "0"][..], "net", "\"NOPE\"", "is not in nets"),
        (&["pads", "0"][..], "at", "[1]", "2 numbers"),
        (&["stack"][..], "copper", "[\"F.Cu\"]", "two layers"),
        (&["stack", "planes", "0"][..], "layer", "\"In9.Cu\"", "is not in stack.copper"),
        (&["pairs", "0"][..], "n", "\"SIG_P\"", "two nets"),
        (&["groups", "0"][..], "parts", "[\"U9\"]", "is not in parts"),
        (&[][..], "units", "\"mil\"", "only \"mm\""),
        (&[][..], "outline", "[{\"line\": [0, 0, 5000, 0]}, {\"line\": [5000, 0, 0, 10]}]", "at most 2000 mm"),
        (&["holes"][..], "0", "0", "not an object"),
    ] {
        if path == ["holes"] {
            let e = load(&t.replace("\"holes\": [[1.5, 1.5, 1.0]]", "\"holes\": [[1.5, 1.5]]")).err().expect("a hole of two numbers went through");
            assert!(e.contains("3 numbers"), "{}", e);
            continue;
        }
        let e = load(&with(&t, path, key, val)).err().unwrap_or_else(|| panic!("{} = {} went through", key, val));
        assert!(e.contains(says), "{} = {}: {}", key, val, e);
    }
}

#[test]
fn numbers_out_of_range_and_deep_nesting_are_errors() {
    assert!(json::parse("{\"a\": 1e999}").unwrap_err().contains("out of range"));
    assert!(json::parse(&("[".repeat(100_000) + &"]".repeat(100_000))).unwrap_err().contains("too deep"));
    let e = load(&with(&text("tiny11.json"), &["rules"], "track", "1e300")).err().unwrap();
    assert!(e.contains("rules.track"), "{}", e);
}
