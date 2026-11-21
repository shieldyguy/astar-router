//! The reader and the writer on two small boards: the house format (nets by name, `uuid`, KiCad 8's
//! indented form, a footprint placed by `transform`) and KiCad 7's (nets by number, `tstamp`, one
//! item a line). Each is read, routed, written and read again.

use super::project::Project;
use super::scene::from_text;
use super::{lay, read, Read};
use crate::json::{self, J};
use crate::Cfg;

fn fixture(name: &str) -> String {
    format!("{}/src/io/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

/// Read, route, lay: the board read, the plan, the written text.
fn through(name: &str) -> (Read, J, String) {
    let rd = read(&fixture(name)).unwrap();
    let routed = crate::route(rd.to_scene().unwrap(), &Cfg::default()).unwrap();
    assert!(routed.open_nets().is_empty(), "{}: nets left open: {:?}", name, routed.open_nets());
    let plan = json::parse(&routed.plan()).unwrap();
    let (text, laid) = lay(&rd, &plan, 0).unwrap();
    assert!(laid.tracks > 0);
    (rd, plan, text)
}

/// The scene of a board's text, through a temp file (the reader takes a path).
fn reread(text: &str, tag: &str) -> J {
    let dir = std::env::temp_dir().join(format!("astar-io-test-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("x.kicad_pcb");
    std::fs::write(&p, text).unwrap();
    let rd = read(p.to_str().unwrap()).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    rd.scene
}

#[test]
fn the_house_format_is_read() {
    let rd = read(&fixture("tiny.kicad_pcb")).unwrap();
    let s = &rd.scene;
    assert_eq!(s.get("stack").get("copper").arr().len(), 2);
    assert_eq!(s.get("stack").get("layers").arr().len(), 3);
    // a zone over less than half the board is no plane; it is copper, by the layer's own name ("Bottom" is
    // B.Cu), with its hole
    assert!(s.get("stack").get("planes").arr().is_empty());
    let zone = &s.get("copper").get("zones").arr()[0];
    assert_eq!((zone.get("net").s(), zone.get("layer").s(), zone.get("holes").arr().len()), (Some("GND"), Some("B.Cu"), 1));
    let pads = s.get("pads").arr();
    assert_eq!(pads.len(), 4);
    // a footprint placed by (transform (translate 5 5) (rotate 90)): its pad 1 at (-0.8, 0) is below it
    assert_eq!(pads[0].get("at").nums(), vec![5.0, 5.8]);
    // a rounded rectangle is its box drawn in by the radius, grown by it
    assert_eq!(pads[0].get("shape").get("r").f(), Some(0.2));
    assert_eq!(pads[0].get("net").s(), Some("SIG \"odd\" (name)"));
    assert_eq!(pads[1].get("clearance").f(), Some(0.3));
    // the second R1 is R1~2; it is on the back; its through pad's copper is offset from its hole
    assert_eq!((pads[2].get("ref").s(), pads[2].get("side").s()), (Some("R1~2"), Some("B")));
    assert_eq!(pads[3].get("at").nums(), vec![15.8, 5.2]);
    assert!(pads[3].get("tht").b() && pads[3].get("drill").f() == Some(0.6));
    assert_eq!(s.get("holes").arr()[0].nums(), vec![10.0, 2.0, 1.0]);
    let refs: Vec<&str> = s.get("parts").arr().iter().filter_map(|p| p.get("ref").s()).collect();
    assert_eq!(refs, vec!["R1", "R1~2", "#00000000-0000-4000-8000-000000000007"]);
    assert_eq!(s.get("copper").get("vias").arr().len(), 1);
    assert_eq!(rd.notes.len(), 3);
}

/// An opening drawn in the mask reaches half its stroke past its points: no other net's track may stand
/// in that rim either (KiCad calls copper of two nets under one opening a bridge).
#[test]
fn a_mask_opening_is_as_wide_as_its_stroke() {
    let text = std::fs::read_to_string(fixture("tiny.kicad_pcb")).unwrap();
    let opening = "\t(gr_poly (pts (xy 8 7) (xy 9 7) (xy 9 8) (xy 8 8)) (stroke (width 0.1) (type solid)) (fill solid) (layer \"F.Mask\"))\n\t(embedded_fonts no)";
    let scene = reread(&text.replace("\t(embedded_fonts no)", opening), "mask");
    let areas: Vec<Vec<Vec<f64>>> = scene.get("keepouts").arr().iter().filter(|k| k.get("name").s() == Some("an opening drawn in the solder mask")).map(|k| k.get("poly").arr().iter().map(|p| p.nums()).collect()).collect();
    // (every area here is a box square to the axes)
    let covers = |x: f64, y: f64| {
        areas.iter().any(|a| {
            let b = a.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, p| (b.0.min(p[0]), b.1.min(p[1]), b.2.max(p[0]), b.3.max(p[1])));
            x >= b.0 - 1e-9 && x <= b.2 + 1e-9 && y >= b.1 - 1e-9 && y <= b.3 + 1e-9
        })
    };
    assert!(covers(8.5, 7.5), "the opening itself");
    assert!(covers(9.04, 7.5) && covers(8.5, 6.96) && covers(9.04, 8.04), "the rim, its corner too");
    assert!(!covers(9.06, 7.5), "and no further");
}

/// A line on the Margin layer is an edge to KiCad's edge rule: no copper within the edge clearance of it,
/// on any layer. Here it stands across the way both nets would go, and they go round its ends.
#[test]
fn copper_keeps_the_edge_clearance_from_a_margin_line() {
    let text = std::fs::read_to_string(fixture("tiny.kicad_pcb")).unwrap();
    let margin = "\t(gr_line (start 10 3) (end 10 8.5) (stroke (width 0.1) (type solid)) (layer \"Margin\"))\n\t(embedded_fonts no)";
    let scene = reread(&text.replace("\t(embedded_fonts no)", margin), "margin");
    let edge = scene.get("rules").get("edge").f().unwrap();
    assert!(scene.get("keepouts").arr().iter().any(|k| k.get("name").s().map_or(false, |n| n.starts_with("the margin line"))));
    let routed = crate::route(crate::Scene::from_json(&scene).unwrap(), &Cfg::default()).unwrap();
    assert!(routed.open_nets().is_empty());
    let plan = json::parse(&routed.plan()).unwrap();
    let mut pieces = 0;
    for net in plan.get("nets").arr() {
        for t in net.get("tracks").arr() {
            let half = t.get("width").f().unwrap() / 2.0;
            for piece in t.get("path").arr() {
                let v = if piece.get("line").arr().is_empty() { piece.get("arc").nums() } else { piece.get("line").nums() };
                // (points along the piece, an arc by its three)
                for k in 0..=20 {
                    let u = k as f64 / 20.0;
                    let (x, y) = (v[0] + (v[v.len() - 2] - v[0]) * u, v[1] + (v[v.len() - 1] - v[1]) * u);
                    let d = (x - 10.0).hypot(y - y.clamp(3.0, 8.5));
                    assert!(d >= edge + 0.05 + half - 1e-6, "copper {:.3} mm from the margin line at ({:.2}, {:.2})", d - half - 0.05, x, y);
                }
                pieces += 1;
            }
        }
    }
    assert!(pieces > 0);
}

#[test]
fn the_house_format_is_written() {
    let (rd, plan, text) = through("tiny.kicad_pcb");
    let before = rd.board.doc.text();
    // everything the board had is there byte for byte: the new items stand before its last bracket
    let cut = before.rfind(')').unwrap();
    assert!(text.starts_with(&before[..cut]) && text.ends_with(")\n"));
    let new = &text[cut..];
    assert!(new.contains("\t(segment\n\t\t(start ") && new.contains("(net \"SIG \\\"odd\\\" (name)\")") && new.contains("\t\t(uuid \""));
    // a new via takes the board's protection (no token); the via that was there keeps its own
    assert!(!new.contains("tenting") && text.matches("(tenting").count() == 1);
    // the same plan writes the same file; a seed changes the ids and nothing else
    assert!(lay(&rd, &plan, 0).unwrap().0 == text);
    let other = lay(&rd, &plan, 7).unwrap().0;
    assert!(other != text && other.len() == text.len());
    // read again: the copper that was there and the copper laid
    let again = reread(&text, "house");
    let laid: usize = plan.get("nets").arr().iter().map(|n| n.get("tracks").arr().iter().map(|t| t.get("path").arr().len()).sum::<usize>()).sum();
    assert_eq!(again.get("copper").get("tracks").arr().len(), 1 + laid);
    assert_eq!(again.get("pads").arr().len(), 4);
}

#[test]
fn kicad_7_is_read_and_written_in_its_own_dialect() {
    let (rd, plan, text) = through("tiny7.kicad_pcb");
    assert!(rd.board.nets_by_number());
    assert_eq!(rd.scene.get("pads").arr()[0].get("at").nums(), vec![5.0, 5.8]);
    let before = rd.board.doc.text();
    let new = &text[before.rfind(')').unwrap()..];
    assert!(new.contains("  (segment (start ") && new.contains("(net 1) (tstamp ") && !new.contains("uuid") && !new.contains("(net \""));
    let again = reread(&text, "k7");
    assert!(again.get("copper").get("tracks").arr().len() > 1);
    assert!(again.get("copper").get("tracks").arr().iter().all(|t| matches!(t.get("net").s(), Some("SIG") | Some("GND"))));
    let _ = plan;
}

#[test]
fn copper_is_lifted_by_its_id_and_a_plane_takes_the_zones_place() {
    let rd = read(&fixture("tiny.kicad_pcb")).unwrap();
    let plan = json::parse(r#"{"nets": [], "planes": [{"net": "GND", "layer": "B.Cu", "pads": "thermal_tht"}], "lift": ["00000000-0000-4000-8000-00000000000a"]}"#).unwrap();
    let (text, laid) = lay(&rd, &plan, 0).unwrap();
    assert_eq!((laid.lifted, laid.zones), (2, 1));
    let again = reread(&text, "lift");
    assert_eq!(again.get("copper").get("tracks").arr().len(), 0);
    let zones = again.get("copper").get("zones").arr();
    assert_eq!(zones.len(), 1);
    assert_eq!((zones[0].get("pads").s(), zones[0].get("holes").arr().len()), (Some("thermal_tht"), 0));
    // nothing else moved: the text less the two items and the new zone is the board's
    assert!(text.contains("(uuid \"00000000-0000-4000-8000-00000000000b\")") && !text.contains("00000000-0000-4000-8000-00000000000a"));
    // a plane that is the net's host's (`--drops`) is not this board's to draw
    let hosted = json::parse(r#"{"nets": [], "planes": [{"net": "GND", "layer": "B.Cu", "host": true}]}"#).unwrap();
    assert_eq!(lay(&rd, &hosted, 0).unwrap().1.zones, 0);
    // a plan for another board is refused, not written
    let bad = json::parse(r#"{"nets": [{"name": "NOPE", "tracks": [{"layer": "In9.Cu", "width": 0.2, "path": [{"line": [0, 0, 1, 1]}]}]}]}"#).unwrap();
    assert!(lay(&rd, &bad, 0).unwrap_err().contains("In9.Cu"));
}

/// A bent board file is an error or a scene, never a panic: both fixtures cut, doubled and given wild
/// numbers a few thousand ways, each read and, when it reads, checked as a scene and laid on.
#[test]
fn a_bent_board_is_an_error_not_a_panic() {
    let mut seed: u64 = 0x2545f4914f6cdd1d;
    let mut rnd = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n.max(1) as u64) as usize
    };
    let wild = ["1e300", "-1e300", "0", "-0.0000001", "1e-300", "nan", "inf", "99999999999999999999", "\"", "(", ")", "((((", "|", "\\"];
    let plan = json::parse(r#"{"nets": [{"name": "GND", "tracks": [{"layer": "F.Cu", "width": 0.2, "path": [{"line": [1, 1, 2, 2]}]}], "vias": [{"at": [3, 3]}]}], "planes": [{"net": "GND", "layer": "B.Cu"}]}"#).unwrap();
    let (mut read_ok, mut scenes) = (0, 0);
    for name in ["tiny.kicad_pcb", "tiny7.kicad_pcb"] {
        let text = std::fs::read_to_string(fixture(name)).unwrap();
        for round in 0..2500 {
            let mut t = text.clone().into_bytes();
            for _ in 0..1 + rnd(3) {
                let at = rnd(t.len());
                match rnd(4) {
                    // a span cut out
                    0 => {
                        let end = (at + rnd(60)).min(t.len());
                        t.drain(at..end);
                    }
                    // a span doubled
                    1 => {
                        let end = (at + rnd(200)).min(t.len());
                        let piece = t[at..end].to_vec();
                        t.splice(at..at, piece);
                    }
                    // a number made wild
                    2 => {
                        let Some(start) = (at..t.len()).find(|&i| t[i].is_ascii_digit()) else { continue };
                        let end = (start..t.len()).find(|&i| !(t[i].is_ascii_digit() || t[i] == b'.')).unwrap_or(t.len());
                        t.splice(start..end, wild[rnd(wild.len())].bytes());
                    }
                    // the file cut short
                    _ => t.truncate(at),
                }
            }
            let Ok(t) = String::from_utf8(t) else { continue };
            if let Ok(rd) = from_text(t, Project::defaults(), name) {
                read_ok += 1;
                scenes += usize::from(rd.to_scene().is_ok());
                let _ = lay(&rd, &plan, round as u64);
            }
        }
    }
    eprintln!("bent boards: 5000, {} read, {} of them scenes the router takes, 0 panics", read_ok, scenes);
    assert!(read_ok > 500 && scenes > 100);
}
