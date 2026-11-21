//! The egress plan against fixtures small enough to count by hand (tests/fixtures/egress/make.py says how
//! each number is counted).

use astar::json::J;
use astar::passes::egress::{analyse, prepare, Egress};
use astar::route::Router;
use astar::{Cfg, Scene};

fn load(name: &str) -> (Router, Egress, J) {
    let dir = format!("{}/tests/fixtures/egress", env!("CARGO_MANIFEST_DIR"));
    let scene = Scene::load(&format!("{}/{}.json", dir, name)).unwrap();
    let expect = astar::json::parse(&std::fs::read_to_string(format!("{}/{}.expect.json", dir, name)).unwrap()).unwrap();
    let cfg = Cfg { pitch: expect.get("pitch").f().unwrap(), ..Cfg::default() };
    let mut r = prepare(scene, &cfg).unwrap();
    let eg = analyse(&mut r);
    (r, eg, expect)
}

fn layer(r: &Router, x: &J) -> usize {
    let name = x.get("layer").s().unwrap();
    r.sc.layers.iter().position(|l| l == name).unwrap()
}

/// Every gap the fixture names has the width, the tracks and the lanes counted by hand, and there are no
/// gaps between pads but those.
fn gaps_match(name: &str) {
    let (r, eg, x) = load(name);
    let l = layer(&r, &x);
    for g in x.get("gaps").arr() {
        let (a, b) = (g.get("a").s().unwrap(), g.get("b").s().unwrap());
        let got = eg.gaps.between(&r, a, b, l).unwrap_or_else(|| panic!("{}: no gap between {} and {}", name, a, b));
        assert!((got.width - g.get("width").f().unwrap()).abs() < 1e-6, "{}: {} | {}: {} mm wide", name, a, b, got.width);
        assert_eq!(got.tracks as f64, g.get("tracks").f().unwrap(), "{}: {} | {}: tracks by the rules", name, a, b);
        assert_eq!(got.lanes as f64, g.get("lanes").f().unwrap(), "{}: {} | {}: lanes on the grid", name, a, b);
    }
    if let Some(n) = x.get("gap_count").f() {
        let is_pad = |o: u32| eg.gaps.name(&r, o).contains('.') && !eg.gaps.name(&r, o).starts_with("TP");
        let pads = eg.gaps.gaps.iter().filter(|g| g.layer as usize == l && is_pad(g.a) && is_pad(g.b)).count();
        assert_eq!(pads as f64, n, "{}: gaps between two pads", name);
    }
    for pair in x.get("no_gap").arr() {
        let (a, b) = (pair.arr()[0].s().unwrap(), pair.arr()[1].s().unwrap());
        assert!(eg.gaps.between(&r, a, b, l).is_none(), "{}: {} and {} do not face each other", name, a, b);
    }
    for p in x.get("pockets").arr() {
        let at = p.get("at").nums();
        let (nx, ny) = r.f.node_near(at[0], at[1]);
        let k = eg.gaps.region(l, ny, nx, r.f.w).unwrap_or_else(|| panic!("{}: no region at {:?}", name, at));
        assert!(!eg.gaps.regions[k].open, "{}: the region at {:?} is a pocket", name, at);
        assert_eq!(eg.gaps.regions[k].vias as f64, p.get("vias").f().unwrap(), "{}: via sites in the pocket at {:?}", name, at);
    }
}

#[test]
fn pad_rows_at_six_pitches_have_the_capacities_counted_by_hand() {
    gaps_match("rows");
}

#[test]
fn a_ball_array_has_one_track_a_gap_and_one_via_a_pocket() {
    gaps_match("bga");
}

#[test]
fn a_channel_in_front_of_a_row_holds_one_track() {
    gaps_match("channel");
}

/// One of the bench five, from the corpus's copy of its scene.
fn bench(name: &str) -> (Router, Egress) {
    let scene = Scene::load(&format!("{}/corpus/boards/{}/scene.json", env!("CARGO_MANIFEST_DIR"), name)).unwrap();
    let mut r = prepare(scene, &Cfg::default()).unwrap();
    let eg = analyse(&mut r);
    (r, eg)
}

#[test]
fn every_pad_of_the_bench_five_has_ways_out_and_is_given_one() {
    // the repo carries dock and g474; the other three are tried where a checkout has them
    for name in ["dock", "g474", "ccb", "tb150", "ub400"] {
        if !["dock", "g474"].contains(&name) && !std::path::Path::new(&format!("{}/corpus/boards/{}/scene.json", env!("CARGO_MANIFEST_DIR"), name)).exists() {
            eprintln!("{}: not in this checkout, skipped", name);
            continue;
        }
        let (r, eg) = bench(name);
        let none: Vec<String> = (0..eg.demands.len()).filter(|&di| eg.cands[di].is_empty()).map(|di| eg.pad_name(&r, di)).collect();
        assert!(none.is_empty(), "{}: pads with no way out: {:?}", name, none);
        assert!(eg.choice.iter().all(|c| c.is_some()), "{}: a pad was given no way out", name);
        assert!(eg.model.fits(&eg.cands, &eg.choice), "{}: the ways given do not stand together", name);
        assert_eq!(eg.exit_code(), 0);
    }
}

/// Every group of pads small enough to try every choice: the assignment leaves as few pads without a way
/// as any choice does, and costs no more than the cheapest such.
fn best_by_brute_force(name: &str) -> usize {
    let (_, eg, _) = load(name);
    let mut tried = 0;
    for group in &eg.model.groups {
        let options: f64 = group.iter().map(|&di| eg.cands[di].len() as f64 + 1.0).product();
        if group.len() < 2 || options > 300_000.0 {
            continue;
        }
        tried += 1;
        let mut choice: Vec<Option<usize>> = vec![None; eg.demands.len()];
        let mut best = u64::MAX;
        let mut counter = vec![0usize; group.len()];
        loop {
            for (k, &di) in group.iter().enumerate() {
                choice[di] = if counter[k] == 0 { None } else { Some(counter[k] - 1) };
            }
            if eg.model.fits(&eg.cands, &choice) {
                best = best.min(group.iter().map(|&di| choice[di].map_or(astar::passes::egress::assign::NONE, |c| eg.cands[di][c].cost as u64)).sum());
            }
            let mut k = 0;
            while k < group.len() {
                counter[k] += 1;
                if counter[k] <= eg.cands[group[k]].len() {
                    break;
                }
                counter[k] = 0;
                k += 1;
            }
            if k == group.len() {
                break;
            }
        }
        let got: u64 = group.iter().map(|&di| eg.choice[di].map_or(astar::passes::egress::assign::NONE, |c| eg.cands[di][c].cost as u64)).sum();
        assert_eq!(got, best, "{}: the group of {:?}", name, group);
    }
    tried
}

#[test]
fn the_assignment_is_the_best_there_is_wherever_every_choice_can_be_tried() {
    assert!(best_by_brute_force("channel") >= 1);
    assert!(best_by_brute_force("rows") >= 1);
}

#[test]
fn every_pin_of_docks_u1_gets_a_way_out_and_no_two_cross() {
    let (r, eg) = bench("dock");
    let u1: Vec<usize> = (0..eg.demands.len()).filter(|&di| eg.parts[eg.demands[di].part].ref_ == "U1").collect();
    assert_eq!(u1.len(), 11);
    let reach = r.sc.rules.track + r.sc.rules.clearance - 1e-6;
    for (k, &a) in u1.iter().enumerate() {
        let ca = eg.exit(a).unwrap_or_else(|| panic!("{} has no way out", eg.pad_name(&r, a)));
        for &b in &u1[k + 1..] {
            let cb = eg.exit(b).unwrap();
            if r.sc.pads[eg.demands[a].pad].net == r.sc.pads[eg.demands[b].pad].net || ca.layer != cb.layer {
                continue;
            }
            let d = astar::passes::egress::geo::line_line(&ca.points(&r), &cb.points(&r));
            assert!(d >= reach, "{} and {} come within {:.3} mm", eg.pad_name(&r, a), eg.pad_name(&r, b), d);
        }
    }
}

#[test]
fn one_track_too_many_is_reported_with_the_part_the_side_and_the_count() {
    let (r, eg, x) = load("channel");
    let d = eg.deficits(&r);
    assert_eq!(d.len(), 1);
    assert_eq!(eg.parts[d[0].part].ref_, x.get("part").s().unwrap());
    assert_eq!(astar::passes::egress::geo::COMPASS[d[0].side as usize], x.get("side").s().unwrap());
    assert_eq!((d[0].need as f64, d[0].have as f64, d[0].short.len() as f64), (x.get("demand").f().unwrap(), x.get("assigned").f().unwrap(), x.get("short").f().unwrap()));
    assert_eq!(eg.pad_name(&r, d[0].short[0]), x.get("short_pads").arr()[0].s().unwrap());
    assert!(d[0].remedies.iter().any(|l| l.starts_with("more room") && l.contains("keepout")), "{:?}", d[0].remedies);
    assert_eq!(eg.exit_code(), 1);
    // the rows have room for every pad
    let (r, eg, _) = load("rows");
    assert!(eg.deficits(&r).is_empty());
    assert_eq!(eg.exit_code(), 0);
}

#[test]
fn a_ball_array_without_vias_has_the_ways_out_counted_by_hand() {
    for name in ["bga_novia", "bga"] {
        let (_, eg, x) = load(name);
        let balls: Vec<usize> = (0..eg.demands.len()).filter(|&di| eg.parts[eg.demands[di].part].ref_ == "U1").collect();
        assert_eq!(balls.len() as f64, x.get("demand").f().unwrap());
        let given = balls.iter().filter(|&&di| eg.choice[di].is_some()).count() as f64;
        assert!(given <= x.get("assigned").f().unwrap() && given >= x.get("assigned_at_least").f().unwrap(), "{}: {} balls have a way out", name, given);
        assert!(eg.model.fits(&eg.cands, &eg.choice));
    }
}
