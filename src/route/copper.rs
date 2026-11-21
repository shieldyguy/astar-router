//! Copper the board itself draws with a net (a filled shape on an outer layer: an electrode, a heat
//! spreader, a contact) is part of that net: KiCad wants it joined like a pad. The router gets it as one.

use crate::geom::Shape;
use crate::scene::{Pad, Scene, ZonePads};

/// A point well inside a polygon: its middle when that is inside, else the middle of a chord.
fn inside_point(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    let shape = Shape::Poly { pts: pts.to_vec(), r: 0.0 };
    let n = pts.len() as f64;
    let mid = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
    let mut best: Option<(f64, (f64, f64))> = None;
    let mut look = |p: (f64, f64)| {
        let d = shape.depth(p.0, p.1);
        if d > 0.0 && best.map_or(true, |(bd, _)| d > bd) {
            best = Some((d, p));
        }
    };
    look(mid);
    for i in 0..pts.len() {
        for j in i + 2..pts.len() {
            look(((pts[i].0 + pts[j].0) / 2.0, (pts[i].1 + pts[j].1) / 2.0));
        }
        look(((pts[i].0 + mid.0) / 2.0, (pts[i].1 + mid.1) / 2.0));
    }
    best.map(|(_, p)| p)
}

/// Every shape the board draws in copper on an outer layer with a net, that no pad of the net stands
/// on, as a pad of that net. Returns how many.
pub(crate) fn copper_pads(sc: &mut Scene) -> usize {
    let last = sc.layers.len() - 1;
    let mut made = 0;
    for k in 0..sc.copper.zones.len() {
        let z = &sc.copper.zones[k];
        if !(z.locked && z.min_width == Some(0.0)) || z.net == 0 || z.outline.len() < 3 || (z.layer != 0 && z.layer != last) {
            continue;
        }
        let (net, back) = (z.net, z.layer == last);
        let shape = Shape::Poly { pts: z.outline.clone(), r: 0.0 };
        // a pad of the net on it already (a footprint's tab drawn round its pad): the shape is joined
        // through that pad
        let stood_on = sc.nets[net as usize].pads.iter().any(|&p| {
            let pad = &sc.pads[p];
            (pad.tht || pad.back == back) && (shape.dist(pad.x, pad.y) == 0.0 || z.outline.iter().any(|q| pad.shape.dist(q.0, q.1) == 0.0))
        });
        if stood_on {
            continue;
        }
        let Some(at) = inside_point(&z.outline) else { continue };
        let idx = sc.pads.len();
        sc.pads.push(Pad { ref_: format!("~copper{}", k), num: "1".into(), net, x: at.0, y: at.1, fx: at.0, fy: at.1, tht: false, back, shape, clearance: 0.0, drill: None, pour: Some(ZonePads::Solid), thermal_gap: None, spoke_angle: None });
        sc.nets[net as usize].pads.push(idx);
        made += 1;
    }
    made
}
