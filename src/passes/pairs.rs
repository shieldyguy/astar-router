//! Differential pairs: the two nets of a pair laid side by side, first.
//!
//! A pair runs between two parts that both of its nets have a pad on (a connector or its ESD array, the
//! chip). Between them the two tracks are one thing: a centre line found by the search for a net that
//! keeps half the pair's own breadth more from everything than a plain net does, and the two tracks
//! drawn off that line, one to each side, the pair's gap apart. They are laid before anything else and
//! are on the board from then on like copper it came with: the route joins each pad to its track's end,
//! and joins what else the nets have (a pull-up, a test point) anywhere on them; nothing moves them.
//!
//! What this is not: the pair stays on one layer (a pair that has no way on its pads' layer is routed as
//! two nets, and the plan says so), and its two halves are not made the same length (the plan gives the
//! difference).

use super::{Pass, Step, PASSES};
use crate::geom::hypot;
use crate::route::solve::Ask;
use crate::route::{Cfg, Router};
use crate::scene::{OldTrack, PathPiece, Scene};

pub const PASS: Pass = Pass {
    name: "pairs",
    about: "a differential pair is laid first, its two tracks side by side at the pair's gap on one layer, between the two parts both nets have a pad on; the route joins the pads to it",
    step: Step::Try(draw),
};

/// How far from the middle of a pair's two pads its side-by-side run may begin, mm.
const REACH: f64 = 2.5;

type Pt = (f64, f64);

/// One end of a pair: the pad of each net on the part they share.
struct End {
    p: usize,
    n: usize,
}

/// The board as the route is given it, up to where copper is laid: the field, the plan passes, the via
/// sites, the path passes.
fn planned(sc: Scene, cfg: &Cfg) -> Result<Router, String> {
    let on = |i: usize| !cfg.skip.iter().any(|s| s == PASSES[i].name);
    let mut r = Router::new(sc, cfg.clone(), Vec::new())?;
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Plan(f), true) = (p.step, on(i)) {
            if p.name != "egress" {
                f(&mut r);
            }
        }
    }
    r.find_via_sites();
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Path(f), true) = (p.step, on(i)) {
            r.shapers.push((i, f));
        }
    }
    Ok(r)
}

/// The two parts the pair runs between: the two farthest apart of the parts both nets have a pad on,
/// with the pad of each net there (the nearest two, where a part has more).
fn ends(sc: &Scene, p: usize, n: usize) -> Option<(End, End)> {
    let mut shared: Vec<End> = Vec::new();
    for &a in &sc.nets[p].pads {
        let ref_ = &sc.pads[a].ref_;
        if ref_.is_empty() || shared.iter().any(|e| sc.pads[e.p].ref_ == *ref_) {
            continue;
        }
        // the nearest two pads of the two nets on this part
        let mut best: Option<(f64, usize, usize)> = None;
        for &x in sc.nets[p].pads.iter().filter(|&&x| sc.pads[x].ref_ == *ref_) {
            for &y in sc.nets[n].pads.iter().filter(|&&y| sc.pads[y].ref_ == *ref_) {
                let d = hypot(sc.pads[x].x - sc.pads[y].x, sc.pads[x].y - sc.pads[y].y);
                if best.map_or(true, |b| d < b.0) {
                    best = Some((d, x, y));
                }
            }
        }
        if let Some((_, x, y)) = best {
            shared.push(End { p: x, n: y });
        }
    }
    if shared.len() < 2 {
        return None;
    }
    let mid = |e: &End| ((sc.pads[e.p].x + sc.pads[e.n].x) / 2.0, (sc.pads[e.p].y + sc.pads[e.n].y) / 2.0);
    let (mut bi, mut bj, mut bd) = (0, 1, -1.0);
    for i in 0..shared.len() {
        for j in i + 1..shared.len() {
            let (a, b) = (mid(&shared[i]), mid(&shared[j]));
            let d = hypot(a.0 - b.0, a.1 - b.1);
            if d > bd {
                (bi, bj, bd) = (i, j, d);
            }
        }
    }
    let b = shared.swap_remove(bj);
    let a = shared.swap_remove(bi);
    Some((a, b))
}

/// A line of points drawn `d` to its left (negative: to its right). On the inside of a turn the corner
/// is mitred; on the outside it is cut flat, so that no point of the drawn line is further than `d`
/// from the line it is drawn beside. None where a run is too short for its corners at that distance.
fn beside(pts: &[Pt], d: f64) -> Option<Vec<Pt>> {
    let k = pts.len();
    let dir = |i: usize| {
        let (a, b) = (pts[i], pts[i + 1]);
        let l = hypot(b.0 - a.0, b.1 - a.1);
        ((b.0 - a.0) / l, (b.1 - a.1) / l)
    };
    let left = |u: Pt| (-u.1, u.0);
    let mut out = Vec::with_capacity(k + 2);
    // (the run each point of `out` starts, to see below that it still points the way it did)
    let mut run_of: Vec<usize> = Vec::with_capacity(k + 2);
    for j in 0..k {
        if j == 0 || j == k - 1 {
            let m = left(dir(if j == 0 { 0 } else { k - 2 }));
            out.push((pts[j].0 + m.0 * d, pts[j].1 + m.1 * d));
            run_of.push(j);
            continue;
        }
        let (u, v) = (dir(j - 1), dir(j));
        let (a, b) = (left(u), left(v));
        let s = 1.0 + a.0 * b.0 + a.1 * b.1;
        if s < 0.3 {
            return None; // a turn too sharp
        }
        // a turn to the left has its inside on the left
        let turn = u.0 * v.1 - u.1 * v.0;
        if turn * d >= 0.0 {
            out.push((pts[j].0 + (a.0 + b.0) / s * d, pts[j].1 + (a.1 + b.1) / s * d));
            run_of.push(j);
        } else {
            out.push((pts[j].0 + a.0 * d, pts[j].1 + a.1 * d));
            run_of.push(usize::MAX);
            out.push((pts[j].0 + b.0 * d, pts[j].1 + b.1 * d));
            run_of.push(j);
        }
    }
    for i in 0..out.len() - 1 {
        if run_of[i] == usize::MAX {
            continue;
        }
        let (u, v) = (dir(run_of[i]), (out[i + 1].0 - out[i].0, out[i + 1].1 - out[i].1));
        if u.0 * v.0 + u.1 * v.1 <= 1e-6 {
            return None;
        }
    }
    Some(out)
}

fn length(pts: &[Pt]) -> f64 {
    pts.windows(2).map(|w| hypot(w[1].0 - w[0].0, w[1].1 - w[0].1)).sum()
}

/// The line from `a` mm past its start to `b` mm before its end.
fn cut(pts: &[Pt], a: f64, b: f64) -> Vec<Pt> {
    let total = length(pts);
    let at = |s: f64| -> Pt {
        let mut left = s;
        for w in pts.windows(2) {
            let l = hypot(w[1].0 - w[0].0, w[1].1 - w[0].1);
            if left <= l {
                return (w[0].0 + (w[1].0 - w[0].0) * left / l, w[0].1 + (w[1].1 - w[0].1) * left / l);
            }
            left -= l;
        }
        *pts.last().unwrap()
    };
    let mut out = vec![at(a)];
    let mut run = 0.0;
    for w in pts.windows(2) {
        run += hypot(w[1].0 - w[0].0, w[1].1 - w[0].1);
        if run > a + 1e-9 && run < total - b - 1e-9 {
            out.push(w[1]);
        }
    }
    out.push(at(total - b));
    out
}

/// One pair laid: its two tracks, and what the plan says of it. Err: why it is left to be two nets.
fn lay(sc: &Scene, cfg: &Cfg, truth: &Router, pi: usize) -> Result<(Vec<OldTrack>, String), String> {
    let pair = &sc.pairs[pi];
    let (p, n) = (pair.p as usize, pair.n as usize);
    let Some((ea, eb)) = ends(sc, p, n) else { return Err("its two nets share fewer than two parts".into()) };
    let nl = sc.layers.len();
    let side = |q: usize| if sc.pads[q].tht { None } else { Some(if sc.pads[q].back { nl - 1 } else { 0 }) };
    let sides: Vec<usize> = [ea.p, ea.n, eb.p, eb.n].iter().filter_map(|&q| side(q)).collect();
    let layer = sides.first().copied().unwrap_or(0);
    if sides.iter().any(|&l| l != layer) {
        return Err("its pads are on both sides of the board, and a pair is kept to one layer".into());
    }
    if sc.nets.iter().any(|x| x.plane == Some(layer)) || [p, n].iter().any(|&x| sc.nets[x].keep != 0 && sc.nets[x].keep & (1 << layer) == 0) {
        return Err(format!("its pads' layer, {}, is not open to its tracks", sc.layers[layer]));
    }
    // the pair's own sizes: its width, and its gap, which is no less than the clearance between its nets
    let clr = sc.rules.clearance.max(sc.nets[p].clearance).max(sc.nets[n].clearance);
    let w = pair.width.unwrap_or(sc.nets[p].width.max(sc.nets[n].width));
    let gap = pair.gap.unwrap_or(clr).max(clr);
    let d = (w + gap) / 2.0;
    // the centre line's net: a net of its own with no pad, as wide as one track, that keeps from
    // everything what the outer edge of either track must: half the pair's breadth more. From the pair's
    // own pads too: the two tracks end short of them, and the route joins each pad to its track's end.
    // (No neck: the centre line is searched at the pair's width, or at the board's plain width when that
    // is less, and then keeps the difference too.)
    let mut wide = sc.clone();
    wide.rules.floors.track = None;
    let net = wide.nets.len() as u16;
    wide.nets.push(crate::scene::Net::plain(format!("{} (the pair's centre line)", pair.name), crate::scene::Class::Signal, w, clr + d + 0.01 + (w - w.min(sc.rules.track)) / 2.0));
    let mut r = planned(wide, cfg)?;
    let f = &r.f;
    let t_min = r.t_min(net);
    let mid = |e: &End| ((sc.pads[e.p].x + sc.pads[e.n].x) / 2.0, (sc.pads[e.p].y + sc.pads[e.n].y) / 2.0);
    let (ma, mb) = (mid(&ea), mid(&eb));
    // where the centre line may stand near an end: every node within reach of the middle of the two pads
    let near = |m: Pt| -> Vec<(usize, usize, f64)> {
        let ((x0, y0), (x1, y1)) = (f.node_near(m.0 - REACH, m.1 - REACH), f.node_near(m.0 + REACH, m.1 + REACH));
        let mut out = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dm = hypot(f.wx(x) - m.0, f.wy(y) - m.1);
                if dm <= REACH && crate::field::sdist(&f.cells[f.idx(layer, y, x)], net) >= t_min {
                    out.push((x, y, dm));
                }
            }
        }
        out
    };
    let (from, to) = (near(ma), near(mb));
    if from.is_empty() || to.is_empty() {
        return Err("there is no room for the two tracks side by side near its pads".into());
    }
    // (a start further from the pads costs three times its distance: the run begins as near as it can)
    let sources: Vec<(u8, u16, u16, u32)> = from.iter().map(|&(x, y, dm)| (layer as u8, y as u16, x as u16, (dm * 3000.0) as u32)).collect();
    let mut tbox = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    let best = to.iter().fold(f64::MAX, |b, t| b.min(t.2));
    for &(x, y, dm) in &to {
        // (the far end: the nodes nearest the middle of its pads)
        if dm <= best + 2.0 * f.pitch {
            let i = f.idx(layer, y, x);
            r.search.mark_target_at(i);
            tbox = (tbox.0.min(x as i32), tbox.1.min(y as i32), tbox.2.max(x as i32), tbox.3.max(y as i32));
        }
    }
    let cells = (r.f.w * r.f.h) as u64;
    let ask = Ask { net, sources, tbox, tlayers: 1 << layer, plane_goal: None, own_vias: Vec::new(), own_pads: Vec::new(), via_mm: 1.0e4, rip_mm: cfg.rip_mm, hard: 30_000 + cells / 40, cap: 400_000 + cells / 4, shut: false, what: String::new() };
    r.strict = true;
    let found = r.solve(&ask);
    r.search.clear_targets();
    let Some((path, _)) = found else { return Err(format!("the two tracks side by side find no way on {}", sc.layers[layer])) };
    if path.iter().any(|q| q.0 as usize != layer) {
        return Err(format!("the two tracks side by side find no way that stays on {}", sc.layers[layer]));
    }
    let path = r.shape(net, path);
    let mut centre: Vec<Pt> = Vec::new();
    for q in &path {
        let pt = (r.f.wx(q.2 as usize), r.f.wy(q.1 as usize));
        if centre.last().map_or(true, |l| hypot(l.0 - pt.0, l.1 - pt.1) > 1e-9) {
            // (a point on the line of the two before it only lengthens that run)
            if centre.len() >= 2 {
                let (a, b) = (centre[centre.len() - 2], centre[centre.len() - 1]);
                if ((b.0 - a.0) * (pt.1 - a.1) - (b.1 - a.1) * (pt.0 - a.0)).abs() < 1e-9 {
                    centre.pop();
                }
            }
            centre.push(pt);
        }
    }
    if centre.len() < 2 || length(&centre) < 4.0 * d {
        return Err("its pads are too near each other for a run side by side".into());
    }
    // which side each net is on, by where its pads lie at each end: the same at both, or the pair is crossed
    let cross = |u: Pt, e: &End| u.0 * (sc.pads[e.p].y - sc.pads[e.n].y) - u.1 * (sc.pads[e.p].x - sc.pads[e.n].x);
    let u0 = (centre[1].0 - centre[0].0, centre[1].1 - centre[0].1);
    let k = centre.len();
    let u1 = (centre[k - 1].0 - centre[k - 2].0, centre[k - 1].1 - centre[k - 2].1);
    let (sa, sb) = (cross(u0, &ea), cross(u1, &eb));
    if sa * sb < 0.0 {
        return Err("it is crossed: one net is on the left at one end and on the right at the other, and the two tracks may not cross".into());
    }
    let p_left = if sa.abs() >= sb.abs() { sa > 0.0 } else { sb > 0.0 };
    // the two tracks, cut back from each end until both are clear of everything else by the board's own
    // measure: the fixed copper as the route will see it, each track against the other net's pads too
    let half = w / 2.0;
    let clear = |pts: &[Pt], own: usize| pts.windows(2).all(|s| truth.statics.track_clear(s[0], s[1], half, truth.net_clr(own as u16), 1 << layer, own as u16));
    let step = r.f.pitch;
    let total = length(&centre);
    let most = ((REACH / step) as usize).min(((total - 4.0 * d) / step / 2.0) as usize);
    let mut laid: Option<(Vec<Pt>, Vec<Pt>, f64)> = None;
    'cut: for sum in 0..=2 * most {
        for ca in 0..=sum.min(most) {
            let cb = sum - ca;
            if cb > most {
                continue;
            }
            let c = cut(&centre, ca as f64 * step, cb as f64 * step);
            let (Some(l), Some(rt)) = (beside(&c, d), beside(&c, -d)) else { continue };
            let (pp, nn) = if p_left { (l, rt) } else { (rt, l) };
            if clear(&pp, p) && clear(&nn, n) {
                laid = Some((pp, nn, length(&c)));
                break 'cut;
            }
        }
    }
    if cfg.verbose {
        eprintln!("  pair {}: middles {:?} {:?}; {} nodes to start on, {} to end on (nearest {:.2} mm); centre {:?}", pair.name, ma, mb, from.len(), to.len(), best, centre);
    }
    let Some((pp, nn, run)) = laid else { return Err("its two tracks side by side do not fit among the copper near its pads".into()) };
    let track = |own: usize, pts: &[Pt]| OldTrack { net: own as u16, layer, width: w, path: pts.windows(2).map(|s| PathPiece::Line([s[0].0, s[0].1, s[1].0, s[1].1])).collect(), locked: false, laid: true };
    let (lp, ln) = (length(&pp), length(&nn));
    let said = format!(
        "laid as a pair on {}: {:.1} mm side by side, {} mm wide and {} mm apart, from {} to {}; {} is {:.2} mm the longer there (the halves are not made the same length)",
        sc.layers[layer],
        run,
        crate::json::num(w),
        crate::json::num(gap),
        sc.pads[ea.p].ref_,
        sc.pads[eb.p].ref_,
        if lp >= ln { &sc.nets[p].name } else { &sc.nets[n].name },
        (lp - ln).abs()
    );
    Ok((vec![track(p, &pp), track(n, &nn)], said))
}

/// Every pair of the scene whose two nets are to be routed here, laid or left, and said.
fn draw(sc: &mut Scene, cfg: &Cfg) {
    let todo: Vec<usize> = (0..sc.pairs.len()).filter(|&i| {
        let (p, n) = (&sc.nets[sc.pairs[i].p as usize], &sc.nets[sc.pairs[i].n as usize]);
        p.wanted() && n.wanted() && p.plane.is_none() && n.plane.is_none()
    }).collect();
    if todo.is_empty() {
        return;
    }
    // the board as it is, for the exact measure of what is laid
    let Ok(truth) = planned(sc.clone(), cfg) else { return };
    for i in todo {
        let p = sc.pairs[i].p;
        match lay(sc, cfg, &truth, i) {
            Ok((tracks, said)) => {
                sc.copper.tracks.extend(tracks);
                sc.said.push((p, "pair", said));
            }
            Err(why) => sc.said.push((p, "pair", format!("routed as two nets, not as a pair: {}", why))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_drawn_beside_another_keeps_its_distance_round_a_corner() {
        let c = [(0.0, 0.0), (4.0, 0.0), (6.0, 2.0), (6.0, 6.0)];
        let (l, r) = (beside(&c, 0.2).unwrap(), beside(&c, -0.2).unwrap());
        for (a, b) in [(&l, &r), (&r, &l)] {
            for w in a.windows(2) {
                for v in b.windows(2) {
                    assert!(crate::geom::seg_seg_dist2(w[0], w[1], v[0], v[1]).sqrt() >= 0.4 - 1e-9);
                }
            }
        }
        // and neither is further than its distance from the line it is drawn beside
        for q in l.iter().chain(&r) {
            let far = c.windows(2).map(|w| crate::geom::seg_seg_dist2(w[0], w[1], *q, *q).sqrt()).fold(f64::MAX, f64::min);
            assert!(far <= 0.2 + 1e-9, "{:?} is {} from the line", q, far);
        }
        // the inner one is the shorter by what the two 45 degree corners take
        assert!((length(&l) - length(&r)).abs() > 0.3);
        // a run too short for its corners at that distance is refused
        assert!(beside(&[(0.0, 0.0), (1.0, 0.0), (1.1, 0.1), (1.1, 2.0)], 0.5).is_none());
        assert_eq!(cut(&c, 1.0, 1.0), vec![(1.0, 0.0), (4.0, 0.0), (6.0, 2.0), (6.0, 5.0)]);
    }
}
