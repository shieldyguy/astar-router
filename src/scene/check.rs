//! The validator: a scene the router can take without falling over. Every limit is said in the message.

use super::*;
use crate::geom::Shape;

/// No coordinate lies farther from the origin than this, mm.
pub const MAX_COORD: f64 = 100_000.0;
/// The outline is no longer than this either way, mm.
pub const MAX_BOARD: f64 = 2_000.0;
/// No width, size or clearance is larger than this, mm.
pub const MAX_SIZE: f64 = 100.0;
pub const MAX_LAYERS: usize = 30;
pub const MAX_NETS: usize = 60_000;

fn size(what: &str, v: f64, zero_ok: bool) -> Result<(), String> {
    if !v.is_finite() || v < 0.0 || (v == 0.0 && !zero_ok) || v > MAX_SIZE {
        return Err(format!("{} is {} mm: it must be {} and at most {} mm", what, v, if zero_ok { "0 or more" } else { "more than 0" }, MAX_SIZE));
    }
    Ok(())
}

fn size_opt(what: &str, v: Option<f64>, zero_ok: bool) -> Result<(), String> {
    v.map_or(Ok(()), |v| size(what, v, zero_ok))
}

fn at(what: &str, x: f64, y: f64) -> Result<(), String> {
    if !x.is_finite() || !y.is_finite() || x.abs() > MAX_COORD || y.abs() > MAX_COORD {
        return Err(format!("{} is at ({}, {}): no coordinate may lie more than {} mm from the origin", what, x, y, MAX_COORD));
    }
    Ok(())
}

fn poly(what: &str, pts: &[(f64, f64)]) -> Result<(), String> {
    pts.iter().try_for_each(|&(x, y)| at(what, x, y))
}

impl Scene {
    /// Is this a scene the router can take? The first thing wrong is the error.
    pub fn check(&self) -> Result<(), String> {
        if self.layers.len() < 2 || self.layers.len() > MAX_LAYERS {
            return Err(format!("{} copper layers: the router takes 2 to {}", self.layers.len(), MAX_LAYERS));
        }
        if self.nets.len() > MAX_NETS {
            return Err(format!("{} nets: the router takes at most {}", self.nets.len(), MAX_NETS));
        }
        let r = &self.rules;
        size("rules.track", r.track, false)?;
        size("rules.clearance", r.clearance, true)?;
        size("rules.via", r.via, false)?;
        size("rules.drill", r.drill, true)?;
        size("rules.edge", r.edge, true)?;
        size("rules.hole_clearance", r.hole_clearance, true)?;
        size_opt("rules.hole_to_hole", r.hole_to_hole, true)?;
        let f = &r.floors;
        for (k, v) in [("track", f.track), ("clearance", f.clearance), ("via", f.via), ("drill", f.drill), ("annular", f.annular), ("hole_to_hole", f.hole_to_hole), ("edge", f.edge)] {
            size_opt(&format!("rules.floors.{}", k), v, true)?;
        }
        let layer_rules = |what: &str, list: &[LayerRule]| -> Result<(), String> {
            for l in list {
                size_opt(&format!("{}: a layer's track", what), l.track, false)?;
                size_opt(&format!("{}: a layer's clearance", what), l.clearance, true)?;
            }
            Ok(())
        };
        layer_rules("rules", &r.layer_rules)?;
        let imp = |what: &str, i: &Option<Impedance>| -> Result<(), String> {
            let Some(i) = i else { return Ok(()) };
            if !(i.ohms > 0.0 && i.ohms <= 1000.0) || i.tolerance.map_or(false, |t| !(0.0..=100.0).contains(&t)) {
                return Err(format!("{}: an impedance must be more than 0 and at most 1000 ohms, its tolerance 0 to 100 percent", what));
            }
            size_opt(&format!("{}: coplanar_gap", what), i.coplanar_gap, false)
        };
        for c in &self.classes {
            let what = format!("class {}", c.name);
            size_opt(&format!("{}: track", what), c.track, false)?;
            size_opt(&format!("{}: clearance", what), c.clearance, true)?;
            size_opt(&format!("{}: via", what), c.via, false)?;
            size_opt(&format!("{}: drill", what), c.drill, true)?;
            size_opt(&format!("{}: pair_width", what), c.pair_width, false)?;
            size_opt(&format!("{}: pair_gap", what), c.pair_gap, false)?;
            layer_rules(&what, &c.layer_rules)?;
            imp(&what, &c.impedance)?;
        }
        for p in &self.class_pairs {
            size("class_pairs: clearance", p.clearance, true)?;
        }
        for l in &self.stackup {
            size(&format!("stack.layers: the thickness of {}", l.name), l.thickness, true)?;
            if l.epsilon_r.map_or(false, |e| !(1.0..=100.0).contains(&e)) || l.loss_tangent.map_or(false, |t| !(0.0..=1.0).contains(&t)) {
                return Err(format!("stack.layers: {}: epsilon_r must be 1 to 100 and loss_tangent 0 to 1", l.name));
            }
        }
        for n in self.nets.iter().skip(1) {
            let what = format!("net {}", n.name);
            size(&format!("{}: width", what), n.width, false)?;
            size(&format!("{}: clearance", what), n.clearance, true)?;
            size_opt(&format!("{}: via", what), n.via, false)?;
            size_opt(&format!("{}: drill", what), n.drill, true)?;
            imp(&what, &n.impedance)?;
            if n.plane.map_or(false, |l| l >= self.layers.len()) || n.netclass >= self.classes.len().max(1) {
                return Err(format!("{}: its plane or its class points nowhere", what));
            }
        }

        // where things are
        if self.outline.is_empty() {
            return Err("the scene has no outline".into());
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(ax, ay, bx, by) in &self.outline {
            at("the outline", ax, ay)?;
            at("the outline", bx, by)?;
            x0 = x0.min(ax).min(bx);
            y0 = y0.min(ay).min(by);
            x1 = x1.max(ax).max(bx);
            y1 = y1.max(ay).max(by);
        }
        if !(x1 - x0 > 0.0 && y1 - y0 > 0.0) || x1 - x0 > MAX_BOARD || y1 - y0 > MAX_BOARD {
            return Err(format!("the outline is {} by {} mm: a board is more than 0 and at most {} mm each way", x1 - x0, y1 - y0, MAX_BOARD));
        }
        for p in &self.pads {
            let what = format!("pad {}.{}", p.ref_, p.num);
            at(&what, p.x, p.y)?;
            at(&what, p.fx, p.fy)?;
            size(&format!("{}: clearance", what), p.clearance, true)?;
            size_opt(&format!("{}: drill", what), p.drill, true)?;
            if p.net as usize >= self.nets.len() {
                return Err(format!("{}: its net points nowhere", what));
            }
            match &p.shape {
                Shape::Poly { pts, r } => {
                    poly(&what, pts)?;
                    size(&format!("{}: its shape's r", what), *r, true)?;
                }
                Shape::Circle { x, y, r } => {
                    at(&what, *x, *y)?;
                    size(&format!("{}: its shape's r", what), *r, true)?;
                }
                Shape::Capsule { ax, ay, bx, by, r } => {
                    at(&what, *ax, *ay)?;
                    at(&what, *bx, *by)?;
                    size(&format!("{}: its shape's r", what), *r, true)?;
                }
            }
        }
        for &(x, y, d) in &self.holes {
            at("a hole", x, y)?;
            size("a hole's diameter", d, true)?;
        }
        for k in &self.keepouts {
            poly("a keep-out", &k.poly)?;
            if k.layers.iter().any(|&l| l >= self.layers.len()) {
                return Err("a keep-out names a layer that is not there".into());
            }
        }
        for t in &self.copper.tracks {
            size("copper.tracks: width", t.width, false)?;
            for q in &t.path {
                let v: &[f64] = match q {
                    PathPiece::Line(v) => v,
                    PathPiece::Arc(v) => v,
                };
                v.chunks(2).try_for_each(|c| at("a track on the board", c[0], c[1]))?;
            }
        }
        for v in &self.copper.vias {
            at("a via on the board", v.x, v.y)?;
            size("copper.vias: size", v.size, false)?;
            size("copper.vias: drill", v.drill, true)?;
        }
        for z in &self.copper.zones {
            poly("a zone", &z.outline)?;
            z.holes.iter().try_for_each(|h| poly("a zone", h))?;
            for i in &z.fill {
                poly("a zone's fill", &i.outline)?;
                i.holes.iter().try_for_each(|h| poly("a zone's fill", h))?;
            }
            size_opt("copper.zones: clearance", z.clearance, true)?;
            size_opt("copper.zones: min_width", z.min_width, true)?;
        }
        for p in &self.parts {
            poly(&format!("part {}", p.ref_), &p.courtyard)?;
        }
        for p in &self.pairs {
            let what = format!("pair {}", p.name);
            size_opt(&format!("{}: width", what), p.width, false)?;
            size_opt(&format!("{}: gap", what), p.gap, false)?;
            imp(&what, &p.impedance)?;
            for (k, v) in [("max_skew", p.max_skew), ("max_uncoupled", p.max_uncoupled)] {
                if v.map_or(false, |v| !(0.0..=MAX_COORD).contains(&v)) {
                    return Err(format!("{}: {} must be 0 to {} mm", what, k, MAX_COORD));
                }
            }
        }
        for m in &self.matches {
            if !(0.0..=MAX_COORD).contains(&m.tolerance) || m.target.map_or(false, |t| !(0.0..=MAX_COORD).contains(&t)) {
                return Err(format!("match group {}: tolerance and target must be 0 to {}", m.name, MAX_COORD));
            }
        }
        Ok(())
    }
}
