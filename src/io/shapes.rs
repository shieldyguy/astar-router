//! Drawn things: `gr_*` on the board and `fp_*` in a footprint. They are the outline (Edge.Cuts),
//! copper that is no track (a tab, an antenna, a logo) and the courtyards.

use super::board::{obj, poly, ptf, Board};
use super::num::{arc_pts, bezier_pts, mm, Frame, P};
use super::sexpr::Id;
use crate::json::J;

pub enum Geo {
    Line(P, P),
    /// start, a point on it, end
    Arc(P, P, P),
    /// centre, radius
    Circle(P, i64),
    /// closed
    Poly(Vec<(f64, f64)>),
    /// open
    Curve(Vec<(f64, f64)>),
}

pub struct Drawn {
    /// canonical
    pub layer: String,
    /// the stroke, nanometres
    pub width: i64,
    pub filled: bool,
    pub geo: Geo,
}

fn mmp(p: P) -> (f64, f64) {
    (mm(p.0), mm(p.1))
}

/// `(pts (xy x y) ... (arc (start) (mid) (end)) ...)` as points on the board, mm; arcs made of chords.
pub fn pts_of(b: &Board, id: Id, f: &Frame) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    let Some(pts) = b.doc.find(id, "pts") else { return out };
    for k in b.doc.lists(pts) {
        match b.doc.head(k) {
            "xy" => {
                if let (Some(x), Some(y)) = (b.doc.arg_num(k, 0), b.doc.arg_num(k, 1)) {
                    out.push(mmp(f.put((super::num::nm(x), super::num::nm(y)))));
                }
            }
            "arc" => {
                if let (Some(s), Some(m), Some(e)) = (b.xy(k, "start"), b.xy(k, "mid"), b.xy(k, "end")) {
                    let a = arc_pts(mmp(f.put(s)), mmp(f.put(m)), mmp(f.put(e)), 0.002);
                    let skip = usize::from(out.last() == a.first());
                    out.extend(a.into_iter().skip(skip));
                }
            }
            _ => {}
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// One `gr_*` or `fp_*` list as a drawn thing on the board; None when it is no shape (a text, a
/// dimension) or is not all there.
pub fn drawn(b: &Board, id: Id, f: &Frame) -> Option<Drawn> {
    let head = b.doc.head(id);
    let kind = head.strip_prefix("gr_").or_else(|| head.strip_prefix("fp_"))?;
    let geo = match kind {
        "line" => Geo::Line(f.put(b.xy(id, "start")?), f.put(b.xy(id, "end")?)),
        "arc" => match b.xy(id, "mid") {
            Some(m) => Geo::Arc(f.put(b.xy(id, "start")?), f.put(m), f.put(b.xy(id, "end")?)),
            None => {
                // KiCad 5: (start centre) (end the arc's first point) (angle sweep, clockwise on the screen)
                let (c, s) = (b.xy(id, "start")?, b.xy(id, "end")?);
                let sweep = b.doc.num_of(id, "angle")?;
                let turn = |deg: f64| {
                    let r = super::num::rot((s.0 - c.0, s.1 - c.1), -deg);
                    (c.0 + r.0, c.1 + r.1)
                };
                Geo::Arc(f.put(s), f.put(turn(sweep / 2.0)), f.put(turn(sweep)))
            }
        },
        "circle" => {
            let (c, e) = (b.xy(id, "center")?, b.xy(id, "end")?);
            Geo::Circle(f.put(c), (((e.0 - c.0) as f64).hypot((e.1 - c.1) as f64)).round() as i64)
        }
        "rect" => {
            let (a, c) = (b.xy(id, "start")?, b.xy(id, "end")?);
            Geo::Poly([(a.0, a.1), (c.0, a.1), (c.0, c.1), (a.0, c.1)].iter().map(|p| mmp(f.put(*p))).collect())
        }
        "poly" => Geo::Poly(pts_of(b, id, f)),
        "curve" => {
            let p = pts_of(b, id, f);
            if p.len() != 4 {
                return None;
            }
            Geo::Curve(bezier_pts([p[0], p[1], p[2], p[3]], 32))
        }
        _ => return None,
    };
    let filled = matches!(b.doc.str_of(id, "fill").as_deref(), Some("yes") | Some("solid"));
    Some(Drawn { layer: b.layer_of(id), width: b.stroke(id), filled, geo })
}

fn line(a: (f64, f64), c: (f64, f64)) -> J {
    obj(vec![("line", J::Arr(vec![J::Num(a.0), J::Num(a.1), J::Num(c.0), J::Num(c.1)]))])
}

fn arc(s: (f64, f64), m: (f64, f64), e: (f64, f64)) -> J {
    obj(vec![("arc", J::Arr([s, m, e].iter().flat_map(|p| [J::Num(p.0), J::Num(p.1)]).collect()))])
}

impl Drawn {
    /// The shape as pieces of a path: lines and arcs.
    pub fn path(&self) -> Vec<J> {
        let mut out = Vec::new();
        match &self.geo {
            Geo::Line(a, c) => {
                if a != c {
                    out.push(line(mmp(*a), mmp(*c)));
                }
            }
            Geo::Arc(s, m, e) => out.push(arc(mmp(*s), mmp(*m), mmp(*e))),
            Geo::Circle(c, r) => {
                let (c, r) = (mmp(*c), mm(*r));
                out.push(arc((c.0 + r, c.1), (c.0, c.1 + r), (c.0 - r, c.1)));
                out.push(arc((c.0 - r, c.1), (c.0, c.1 - r), (c.0 + r, c.1)));
            }
            Geo::Poly(p) => {
                for k in 0..p.len() {
                    if p[k] != p[(k + 1) % p.len()] {
                        out.push(line(p[k], p[(k + 1) % p.len()]));
                    }
                }
            }
            Geo::Curve(p) => {
                for w in p.windows(2) {
                    if w[0] != w[1] {
                        out.push(line(w[0], w[1]));
                    }
                }
            }
        }
        out
    }

    /// The shape's points, mm (an arc and a circle as chords): for boxes and hulls.
    pub fn points(&self) -> Vec<(f64, f64)> {
        match &self.geo {
            Geo::Line(a, c) => vec![mmp(*a), mmp(*c)],
            Geo::Arc(s, m, e) => arc_pts(mmp(*s), mmp(*m), mmp(*e), 0.002),
            Geo::Circle(c, r) => {
                // (a micron more: the many-sided shape holds the circle, its sides never inside the rim)
                let (c, r) = (mmp(*c), mm(*r) + 0.001);
                let mut p = arc_pts((c.0 + r, c.1), (c.0, c.1 + r), (c.0 - r, c.1), 0.001);
                p.pop();
                p.extend(arc_pts((c.0 - r, c.1), (c.0, c.1 - r), (c.0 + r, c.1), 0.001));
                p.pop();
                p
            }
            Geo::Poly(p) | Geo::Curve(p) => p.clone(),
        }
    }

    /// The ground a drawing covers, as polygons: a closed shape is itself, a stroke is a box a piece.
    pub fn areas(&self) -> Vec<Vec<(f64, f64)>> {
        let p = self.points();
        if matches!(self.geo, Geo::Poly(_) | Geo::Circle(..)) && p.len() >= 3 {
            return vec![p];
        }
        boxes(&p, (mm(self.width) / 2.0).max(0.05))
    }

    /// The ground within `more` mm of the drawing's stroke, a box a piece (a closed shape's sides too).
    pub fn band(&self, more: f64) -> Vec<Vec<(f64, f64)>> {
        let mut p = self.points();
        if matches!(self.geo, Geo::Poly(_) | Geo::Circle(..)) && p.len() >= 3 {
            p.push(p[0]);
        }
        boxes(&p, mm(self.width) / 2.0 + more)
    }

    /// The stroke round a closed shape, a box a side: the shape reaches that much further than its points.
    pub fn rim(&self) -> Vec<Vec<(f64, f64)>> {
        let mut p = self.points();
        if !matches!(self.geo, Geo::Poly(_) | Geo::Circle(..)) || p.len() < 3 || self.width <= 0 {
            return Vec::new();
        }
        p.push(p[0]);
        boxes(&p, mm(self.width) / 2.0)
    }

    /// Copper that is no track, for the scene's `copper`: a filled shape is a zone with its fill, a
    /// stroke is a track. Both are locked: they are the footprint's or the board's own drawing.
    /// `lent`: the net is not the drawing's own but that of the pad it lies round (a footprint's
    /// drawing carries none). To KiCad such copper joins nothing, so its stroke is marked `drawn`: it is
    /// in every other net's way, and a track of its own net still has to reach the pad itself.
    pub fn copper(&self, net: &str, lent: bool, tracks: &mut Vec<J>, zones: &mut Vec<J>) {
        let closed = matches!(self.geo, Geo::Poly(_) | Geo::Circle(..));
        if closed && self.filled {
            let p = self.points();
            if p.len() >= 3 {
                zones.push(obj(vec![
                    ("net", J::Str(net.to_string())),
                    ("layer", J::Str(self.layer.clone())),
                    ("outline", poly(&p)),
                    ("fill", J::Arr(vec![obj(vec![("outline", poly(&p))])])),
                    ("min_width", J::Num(0.0)),
                    ("locked", J::Bool(true)),
                ]));
            }
        }
        // the stroke round a filled shape is copper too
        if self.width > 0 {
            let path = self.path();
            if !path.is_empty() {
                let mut kv = vec![("net", J::Str(net.to_string())), ("layer", J::Str(self.layer.clone())), ("width", J::Num(mm(self.width))), ("path", J::Arr(path)), ("locked", J::Bool(true))];
                if lent {
                    kv.push(("drawn", J::Bool(true)));
                }
                tracks.push(obj(kv));
            }
        }
    }
}

/// A box round each piece of a path, r each side and r past each end.
fn boxes(p: &[(f64, f64)], r: f64) -> Vec<Vec<(f64, f64)>> {
    p.windows(2)
        .filter_map(|w| {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let l = (dx * dx + dy * dy).sqrt();
            if l < 1e-9 {
                return None;
            }
            let (ux, uy) = (dx / l * r, dy / l * r);
            Some(vec![(w[0].0 - ux + uy, w[0].1 - uy - ux), (w[1].0 + ux + uy, w[1].1 + uy - ux), (w[1].0 + ux - uy, w[1].1 + uy + ux), (w[0].0 - ux - uy, w[0].1 - uy + ux)])
        })
        .collect()
}

/// The box round points, mm: (x0, y0, x1, y1).
pub fn bbox(pts: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let mut it = pts.iter();
    let &(x, y) = it.next()?;
    Some(it.fold((x, y, x, y), |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1))))
}

pub fn box_poly(b: (f64, f64, f64, f64)) -> J {
    J::Arr(vec![ptf((b.0, b.1)), ptf((b.2, b.1)), ptf((b.2, b.3)), ptf((b.0, b.3))])
}
