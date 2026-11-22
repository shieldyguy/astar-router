//! The scene read from JSON.

use super::{Class, Copper, Floors, Keepout, Net, Pad, Rules, Scene, FORBID_COPPER};
use crate::geom::{arc_points, Shape};
use crate::json::J;
use std::collections::HashMap;

pub(super) fn shape_of(j: &J) -> Result<Shape, String> {
    if j.has("poly") {
        let pts: Vec<(f64, f64)> = j
            .get("poly")
            .arr()
            .iter()
            .map(|p| {
                let v = p.nums();
                (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0))
            })
            .collect();
        if pts.len() < 3 {
            return Err("a pad polygon with fewer than 3 points".into());
        }
        Ok(Shape::Poly { pts, r: j.get("r").f_or(0.0) })
    } else if j.has("circle") {
        let v = j.get("circle").nums();
        if v.len() < 3 {
            return Err("a pad circle needs [x, y, r]".into());
        }
        Ok(Shape::Circle { x: v[0], y: v[1], r: v[2] })
    } else if j.has("oval") {
        let v = j.get("oval").nums();
        if v.len() < 5 {
            return Err("a pad oval needs [ax, ay, bx, by, r]".into());
        }
        Ok(Shape::Capsule { ax: v[0], ay: v[1], bx: v[2], by: v[3], r: v[4] })
    } else {
        Err("a pad shape that is not poly, circle or oval".into())
    }
}

impl Scene {
    /// A scene from its JSON text, checked.
    pub fn parse(text: &str) -> Result<Scene, crate::Error> {
        crate::json::parse(text).and_then(|j| Scene::from_json(&j)).map_err(crate::Error::BadInput)
    }

    /// A scene from a file, checked. The error names the file.
    pub fn load(path: &str) -> Result<Scene, crate::Error> {
        let text = std::fs::read_to_string(path).map_err(|e| crate::Error::BadInput(format!("{}: cannot read: {}", path, e)))?;
        Scene::parse(&text).map_err(|crate::Error::BadInput(e)| crate::Error::BadInput(format!("{}: {}", path, e)))
    }

    /// A scene from its JSON, checked (`Scene::check`): format 2 when it says `"version": 2`, else 1.1.
    pub fn from_json(j: &J) -> Result<Scene, String> {
        let sc = if j.get("version").f() == Some(2.0) { Scene::read2(j)? } else { Scene::read1(j)? };
        sc.check()?;
        Ok(sc)
    }

    /// Scene 1.1, read as it always was: what is missing takes a default, what is not known is passed over.
    fn read1(j: &J) -> Result<Scene, String> {
        let stack = j.get("stack");
        let layers: Vec<String> = stack.get("copper").arr().iter().filter_map(|x| x.s().map(|s| s.to_string())).collect();
        if layers.len() < 2 {
            return Err("the scene's stack.copper needs at least two layers".into());
        }
        let layer_ix: HashMap<&str, usize> = layers.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
        let r = j.get("rules");
        let rules = Rules {
            track: r.get("track").f_or(0.2),
            clearance: r.get("clearance").f_or(0.15),
            via: r.get("via").f_or(0.45),
            drill: r.get("drill").f_or(0.2),
            edge: r.get("edge").f_or(0.3),
            hole_clearance: r.get("hole_clearance").f_or(0.2),
            hole_to_hole: None,
            min_spokes: None,
            floors: Floors::default(),
            layer_rules: Vec::new(),
        };

        let mut nets: Vec<Net> = vec![Net::plain(String::new(), Class::Signal, rules.track, 0.0)];
        let mut net_ix: HashMap<String, u16> = HashMap::new();
        for n in j.get("nets").arr() {
            let name = n.get("name").s().unwrap_or("").to_string();
            if name.is_empty() || net_ix.contains_key(&name) {
                continue;
            }
            let class = match n.get("class").s().unwrap_or("signal") {
                "rail" => Class::Rail,
                "ground" => Class::Ground,
                _ => Class::Signal,
            };
            net_ix.insert(name.clone(), nets.len() as u16);
            nets.push(Net::plain(name, class, n.get("width").f_or(rules.track), n.get("clearance").f_or(0.0)));
        }
        if nets.len() > 60000 {
            return Err("more than 60000 nets".into());
        }

        let mut pads = Vec::new();
        for p in j.get("pads").arr() {
            let name = p.get("net").s().unwrap_or("");
            let net = if name.is_empty() {
                0
            } else if let Some(&i) = net_ix.get(name) {
                i
            } else {
                let i = nets.len() as u16;
                net_ix.insert(name.to_string(), i);
                nets.push(Net::plain(name.to_string(), Class::Signal, rules.track, 0.0));
                i
            };
            let at = p.get("at").nums();
            if at.len() < 2 {
                return Err("a pad without \"at\"".into());
            }
            let shape = shape_of(p.get("shape"))?;
            let fp = p.get("fp").nums();
            let idx = pads.len();
            if net != 0 {
                nets[net as usize].pads.push(idx);
            }
            pads.push(Pad {
                ref_: p.get("ref").s().unwrap_or("").to_string(),
                num: p.get("num").s().unwrap_or("").to_string(),
                net,
                x: at[0],
                y: at[1],
                fx: fp.first().copied().unwrap_or(at[0]),
                fy: fp.get(1).copied().unwrap_or(at[1]),
                tht: p.get("tht").b(),
                back: p.get("side").s() == Some("B"),
                shape,
                clearance: p.get("clearance").f_or(0.0),
                drill: None,
                pour: None,
                thermal_gap: None,
                spoke_angle: None,
            });
        }

        for pl in stack.get("planes").arr() {
            let (Some(n), Some(l)) = (pl.get("net").s(), pl.get("layer").s()) else { continue };
            if let (Some(&ni), Some(&li)) = (net_ix.get(n), layer_ix.get(l)) {
                nets[ni as usize].plane = Some(li);
            }
        }

        let holes: Vec<(f64, f64, f64)> = j
            .get("holes")
            .arr()
            .iter()
            .filter_map(|h| {
                let v = h.nums();
                if v.len() >= 3 {
                    Some((v[0], v[1], v[2]))
                } else {
                    None
                }
            })
            .collect();

        let mut outline = Vec::new();
        for q in j.get("outline").arr() {
            if q.has("line") {
                let v = q.get("line").nums();
                if v.len() >= 4 {
                    outline.push((v[0], v[1], v[2], v[3]));
                }
            } else if q.has("arc") {
                let v = q.get("arc").nums();
                if v.len() >= 6 {
                    let pts = arc_points((v[0], v[1]), (v[2], v[3]), (v[4], v[5]), 0.005);
                    for w in pts.windows(2) {
                        outline.push((w[0].0, w[0].1, w[1].0, w[1].1));
                    }
                }
            }
        }
        if outline.is_empty() {
            return Err("the scene has no outline".into());
        }

        let mut keepouts = Vec::new();
        for k in j.get("keepouts").arr() {
            let ls: Vec<usize> = k.get("layers").arr().iter().filter_map(|x| x.s().and_then(|s| layer_ix.get(s).copied())).collect();
            let poly: Vec<(f64, f64)> = k
                .get("poly")
                .arr()
                .iter()
                .filter_map(|p| {
                    let v = p.nums();
                    if v.len() >= 2 {
                        Some((v[0], v[1]))
                    } else {
                        None
                    }
                })
                .collect();
            if poly.len() >= 3 {
                keepouts.push(Keepout { layers: if ls.is_empty() { (0..layers.len()).collect() } else { ls }, poly, forbid: FORBID_COPPER, name: String::new(), net: 0 });
            }
        }

        Ok(Scene {
            version: 1,
            layers,
            rules,
            nets,
            pads,
            holes,
            outline,
            keepouts,
            existing_copper: j.get("copper").arr().len(),
            stackup: Vec::new(),
            classes: Vec::new(),
            class_pairs: Vec::new(),
            copper: Copper::default(),
            parts: Vec::new(),
            pairs: Vec::new(),
            matches: Vec::new(),
            groups: Vec::new(),
            said: Vec::new(),
        })
    }
}
