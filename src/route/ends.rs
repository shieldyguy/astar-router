//! True ends: a track ends where the copper it joins is, not at the grid node beside it.
//!
//! The search walks the grid, and pads do not stand on it: a path's first node is the node nearest the
//! pad's centre, or the end of a pin's fence, and so up to half a pitch to the side of the line the track
//! should run on. Drawn as found, that is a jog at every pad. Here the end leg of a path is drawn on its
//! true line instead: from the pad's centre in the leg's own heading (from a fence: along the pin's
//! centre line), and the corner where that leg ends slides along the next leg to meet it, so both legs
//! keep their headings. A leg with no corner to take up the difference runs on its true line to where
//! the search ended it: into the via, or the track of its net, that stands at its last node, a few
//! hundredths off that one's middle. Only a leg between two true ends (pad to pad, straight) leaves its
//! heading, by that little. A path that ends on a track drawn this way ends on the track itself.
//!
//! Every piece that is not where the search had it is checked against the exact shapes, and drawn at the
//! width that fits where it now lies. Where one is not clear, or would have to be narrower than the
//! search had it, the end is laid as found.

use super::Router;
use crate::geom::hypot;

type P = (f64, f64);

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}
fn dot(a: P, b: P) -> f64 {
    a.0 * b.0 + a.1 * b.1
}
fn cross(a: P, b: P) -> f64 {
    a.0 * b.1 - a.1 * b.0
}
fn unit(a: P, b: P) -> Option<P> {
    let d = sub(b, a);
    let l = hypot(d.0, d.1);
    (l > 1e-9).then(|| (d.0 / l, d.1 / l))
}
fn at(s: P, u: P, t: f64) -> P {
    (s.0 + u.0 * t, s.1 + u.1 * t)
}
/// Where the line through `a` along `u` meets the line through `b` along `v`.
fn meet(a: P, u: P, b: P, v: P) -> Option<P> {
    let den = cross(u, v);
    (den.abs() > 1e-6).then(|| at(a, u, cross(sub(b, a), v) / den))
}

/// A point of a run as it is drawn: where, whether the net's full width fits there, and which of the
/// run's nodes it stands for (NO_NODE: none, it is a true end).
pub(crate) type Pt = (P, bool, usize);
pub(crate) const NO_NODE: usize = usize::MAX;

/// A run's end redrawn: its points from the true end on, and the nodes that have no point any more
/// (which node, where its copper is now).
pub(crate) type TrueEnd = (Vec<Pt>, Vec<(usize, P)>);

impl Router {
    /// One end of a run of net `net` on layer `l`, on its true line.
    ///
    /// `pts`: the run's points from this end on. `stub`: the true end, and, for a pad with a fence, the
    /// point where the fence leaves the pad, each with whether the fence stands at the net's full width.
    /// None: the end is laid as found.
    pub(crate) fn true_end(&self, net: u16, l: usize, pts: &[Pt], stub: &[(P, bool)]) -> Option<TrueEnd> {
        if pts.len() < 2 || stub.is_empty() {
            return None;
        }
        // (a fence stands at the width it was built at; from a plain pad the width is the first node's)
        let (s, s_ok) = (stub[0].0, if stub.len() > 1 { stub[0].1 } else { pts[0].1 });
        let d0 = unit(pts[0].0, pts[1].0)?;
        // the first leg: points 0..=m
        let mut m = 1;
        while m + 1 < pts.len() {
            match unit(pts[m].0, pts[m + 1].0) {
                Some(d) if cross(d0, d).abs() < 1e-9 && dot(d0, d) > 0.0 => m += 1,
                _ => break,
            }
        }
        // the line the leg belongs on: the fence's, else its own heading through the true end
        let u = if stub.len() > 1 { unit(s, stub[1].0)? } else { d0 };
        let mut out: Vec<Pt> = vec![(s, s_ok, NO_NODE)];
        let mut gone: Vec<(usize, P)> = Vec::new();
        // how many of the run's points, at its far end, stay where they were
        let tail;
        if cross(u, d0).abs() < 1e-6 && dot(u, d0) > 0.0 {
            // the leg, on the line `w` from the true end as far as `reach`
            let (w, reach, corner) = if m < pts.len() - 1 {
                // to the corner, which slides along the next leg
                let d1 = unit(pts[m].0, pts[m + 1].0)?;
                let x = meet(s, u, pts[m].0, d1)?;
                if dot(sub(pts[m + 1].0, x), d1) <= 1e-6 {
                    return None;
                }
                (u, dot(sub(x, s), u), true)
            } else if pts[m].2 == NO_NODE {
                // to the run's other true end: straight
                (unit(s, pts[m].0)?, hypot(pts[m].0 .0 - s.0, pts[m].0 .1 - s.1), false)
            } else {
                // to the foot of the run's last node on this line
                (u, dot(sub(pts[m].0, s), u), true)
            };
            if reach <= 1e-6 {
                return None;
            }
            for &(p, ok, node) in &pts[..m] {
                let t = dot(sub(p, s), w);
                if t > 1e-6 && t < reach - 1e-6 {
                    out.push((at(s, w, t), ok, node));
                } else if node != NO_NODE {
                    gone.push((node, at(s, w, t.clamp(0.0, reach))));
                }
            }
            out.push((if corner { at(s, w, reach) } else { pts[m].0 }, pts[m].1, pts[m].2));
            out.extend_from_slice(&pts[m + 1..]);
            tail = (pts.len() - (m + 1)).max(1);
        } else {
            // the path turns where the fence ends: the corner is where the fence's line meets its first leg
            let x = meet(s, u, pts[0].0, d0)?;
            if dot(sub(x, s), u) <= 1e-6 || dot(sub(pts[1].0, x), d0) <= 1e-6 {
                return None;
            }
            out.push((x, pts[0].1, pts[0].2));
            out.extend_from_slice(&pts[1..]);
            tail = pts.len() - 1;
        }
        // a fence stands at its own width as far as it leaves the pad
        if stub.len() > 1 {
            if let Some(w) = unit(s, out[1].0) {
                let t = dot(sub(stub[1].0, s), w);
                if t > 1e-6 && t < hypot(out[1].0 .0 - s.0, out[1].0 .1 - s.1) - 1e-6 {
                    out.insert(1, (at(s, w, t), s_ok, NO_NODE));
                }
            }
        }
        // Every new piece clear by the exact shapes. Its width is what fits where it now lies, not what
        // fitted on the nodes it came from: the net's own width where that is clear, else the narrowest,
        // but never narrower than the search had it (a neck that the grid's path does not need is not
        // worth a straighter end).
        let (half_nom, half_min) = (self.sc.nets[net as usize].width / 2.0, self.w_min(net) / 2.0);
        let new = out.len() - tail;
        let had: Vec<bool> = out[..=new].iter().map(|p| p.1).collect();
        let mut full = vec![true; new];
        for k in 0..new {
            let (a, b) = (out[k].0, out[k + 1].0);
            if hypot(a.0 - b.0, a.1 - b.1) < 1e-7 {
                continue;
            }
            full[k] = self.piece_clear(a, b, half_nom, net, l);
            if !full[k] && ((had[k] && had[k + 1]) || !self.piece_clear(a, b, half_min, net, l)) {
                return None;
            }
        }
        for k in 0..new {
            out[k].1 = (k == 0 || full[k - 1]) && full[k];
        }
        // (the first point that stays where it was keeps the width it had, and is narrow if the piece
        // that now leads to it is)
        out[new].1 &= full[new - 1];
        Some((out, gone))
    }
}

#[cfg(test)]
mod tests {
    use crate::route::grid_offset;
    use crate::scene::Scene;

    #[test]
    fn the_grid_stands_where_the_narrow_pads_are() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scenes/true_ends.json")).unwrap();
        let mut sc = Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap();
        // three pins 0.03 mm off multiples of 0.1 in x and three 0.07 off: the grid takes the first row's
        // line; in y the pins say nothing (they are long that way), and the square pads sit on 0.1
        let (ox, oy) = grid_offset(&sc, 0.1);
        assert!((ox - 0.03).abs() < 1e-9 && oy.abs() < 1e-9, "({}, {})", ox, oy);
        // the lower row's pins narrowed to 0.2 mm: they need the grid more, and get it
        for p in sc.pads.iter_mut().filter(|p| p.ref_ == "V") {
            p.shape = crate::geom::Shape::Poly { pts: vec![(p.x - 0.1, p.y - 0.75), (p.x + 0.1, p.y - 0.75), (p.x + 0.1, p.y + 0.75), (p.x - 0.1, p.y + 0.75)], r: 0.0 };
        }
        let (ox, _) = grid_offset(&sc, 0.1);
        assert!((ox - 0.07).abs() < 1e-9, "{}", ox);
    }
}
