//! The plan read from JSON (router-bench's CONTRACT.md, "Plan"), for the check. The router writes a
//! plan and never reads one: this reader is the check's own.

use crate::json::J;

#[derive(Clone, Copy, Debug)]
pub enum Piece {
    Line([f64; 4]),
    /// start, a point on the arc, end
    Arc([f64; 6]),
}

pub struct Track {
    pub layer: String,
    pub width: f64,
    pub path: Vec<Piece>,
}

pub struct Via {
    pub x: f64,
    pub y: f64,
    pub size: Option<f64>,
    pub drill: Option<f64>,
}

pub struct PlanNet {
    pub name: String,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
}

/// A poured plane: a zone of `net` on `layer`. Without `outline` it covers the whole board; with it,
/// one zone per outer loop, each with its own holes.
pub struct Plane {
    pub net: String,
    pub layer: String,
    pub outline: Vec<Vec<(f64, f64)>>,
    pub holes: Vec<Vec<Vec<(f64, f64)>>>,
    /// the plane is not on this board: the net is joined by its host's pour (`astar route --drops`)
    pub host: bool,
}

pub struct Plan {
    pub nets: Vec<PlanNet>,
    pub planes: Vec<Plane>,
    /// the nets the plan itself says it did not join
    pub left_out: Vec<String>,
    /// the nets the run was not asked to route (`--nets`, `--except`, the scene's `scope`)
    pub not_asked: Vec<String>,
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn ring(j: &J, what: &str) -> Result<Vec<(f64, f64)>, String> {
    let mut out = Vec::new();
    for p in j.arr() {
        let v = p.nums();
        if v.len() < 2 || !finite(&v) {
            return Err(format!("{}: a point that is not [x, y]", what));
        }
        out.push((v[0], v[1]));
    }
    if out.len() < 3 {
        return Err(format!("{}: a loop of fewer than 3 points", what));
    }
    Ok(out)
}

impl Plan {
    pub fn load(path: &str) -> Result<Plan, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: cannot read: {}", path, e))?;
        let j = crate::json::parse(&text).map_err(|e| format!("{}: {}", path, e))?;
        Plan::from_json(&j).map_err(|e| format!("{}: {}", path, e))
    }

    pub fn from_json(j: &J) -> Result<Plan, String> {
        if !matches!(j, J::Obj(_)) {
            return Err("the plan is not a JSON object".into());
        }
        let mut nets = Vec::new();
        for n in j.get("nets").arr() {
            let name = n.get("name").s().ok_or("a net of the plan without a name")?.to_string();
            let mut tracks = Vec::new();
            for t in n.get("tracks").arr() {
                let what = format!("a track of {}", name);
                let layer = t.get("layer").s().ok_or(format!("{} without a layer", what))?.to_string();
                let width = t.get("width").f().filter(|w| w.is_finite() && *w > 0.0).ok_or(format!("{} without a width above 0", what))?;
                let mut path = Vec::new();
                for p in t.get("path").arr() {
                    if p.has("line") {
                        let v = p.get("line").nums();
                        if v.len() != 4 || !finite(&v) {
                            return Err(format!("{}: a line that is not [x0, y0, x1, y1]", what));
                        }
                        path.push(Piece::Line([v[0], v[1], v[2], v[3]]));
                    } else if p.has("arc") {
                        let v = p.get("arc").nums();
                        if v.len() != 6 || !finite(&v) {
                            return Err(format!("{}: an arc that is not [sx, sy, mx, my, ex, ey]", what));
                        }
                        path.push(Piece::Arc([v[0], v[1], v[2], v[3], v[4], v[5]]));
                    } else {
                        return Err(format!("{}: a piece of path that is neither line nor arc", what));
                    }
                }
                tracks.push(Track { layer, width, path });
            }
            let mut vias = Vec::new();
            for v in n.get("vias").arr() {
                let at = v.get("at").nums();
                if at.len() != 2 || !finite(&at) {
                    return Err(format!("a via of {} whose \"at\" is not [x, y]", name));
                }
                let pos = |k: &str| -> Result<Option<f64>, String> {
                    match v.get(k) {
                        J::Null => Ok(None),
                        x => x.f().filter(|d| d.is_finite() && *d > 0.0).map(Some).ok_or(format!("a via of {} whose {} is not a size above 0", name, k)),
                    }
                };
                vias.push(Via { x: at[0], y: at[1], size: pos("size")?, drill: pos("drill")? });
            }
            nets.push(PlanNet { name, tracks, vias });
        }
        let mut planes = Vec::new();
        for q in j.get("planes").arr() {
            let net = q.get("net").s().ok_or("a plane without a net")?.to_string();
            let layer = q.get("layer").s().ok_or(format!("the plane of {} without a layer", net))?.to_string();
            let what = format!("the plane of {} on {}", net, layer);
            let mut outline = Vec::new();
            for r in q.get("outline").arr() {
                outline.push(ring(r, &what)?);
            }
            let mut holes = Vec::new();
            for hs in q.get("holes").arr() {
                let mut one = Vec::new();
                for r in hs.arr() {
                    one.push(ring(r, &what)?);
                }
                holes.push(one);
            }
            planes.push(Plane { net, layer, outline, holes, host: q.get("host").b() });
        }
        let left_out = j.get("left_out").arr().iter().filter_map(|x| x.get("net").s().map(|s| s.to_string())).collect();
        let not_asked = j.get("not_asked").arr().iter().filter_map(|x| x.s().map(|s| s.to_string())).collect();
        Ok(Plan { nets, planes, left_out, not_asked })
    }
}
