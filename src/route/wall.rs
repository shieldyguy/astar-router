//! What shuts a pad in, by name: the fixed things that bound all a pad can reach.

use super::Router;
use crate::field::{sdist, Kind};
use crate::search::{DX, DY};

impl Router {
    /// The fixed things round a pad the board itself leaves no way to (`Job::walled`): its nodes are
    /// flooded over what a track of its net may stand on, through every via site, and what stops the
    /// flood is named, nearest the pad first, four at most. Empty when the flood is not shut in after all
    /// (it grew past a pad's own neighbourhood).
    pub(crate) fn wall_of(&self, p: usize) -> Vec<String> {
        let f = &self.f;
        let (w, h, nl, per) = (f.w, f.h, f.nl, f.w * f.h);
        let pad = &self.sc.pads[p];
        let net = pad.net;
        let t = self.t_low(net);
        let site = |k: usize| self.kind(net).ok.get(k >> 6).map_or(false, |m| m & (1 << (k & 63)) != 0);
        let keep = self.keeps.get(&net);
        let off = |n: usize| keep.map_or(false, |k| k.off(n / per, f.wx((n % per) % w), f.wy((n % per) / w)));
        // where the flood was stopped: (how far from the pad, the node, by the net's own layer rule),
        // nearest the pad first
        let mut stops: Vec<(f64, usize, bool)> = Vec::new();
        let mut seen = vec![0u64; per * nl / 64 + 1];
        let mut todo: Vec<usize> = Vec::new();
        for a in &self.acc[p] {
            for l in self.pad_layers(p) {
                let i = f.idx(l, a.y as usize, a.x as usize);
                if seen[i >> 6] & (1 << (i & 63)) == 0 {
                    seen[i >> 6] |= 1 << (i & 63);
                    todo.push(i);
                }
            }
        }
        if todo.is_empty() {
            // no node to stand on at all: what is nearest the pad itself is what is in its way
            let (x, y) = f.node_near(pad.x, pad.y);
            for l in self.pad_layers(p) {
                stops.push((0.0, f.idx(l, y, x), false));
            }
        }
        let mut count = 0usize;
        while let Some(i) = todo.pop() {
            count += 1;
            if count > 400_000 {
                return Vec::new();
            }
            let (l, k) = (i / per, i % per);
            let (x, y) = (k % w, k / w);
            if x < 1 || y < 1 || x + 1 >= w || y + 1 >= h {
                continue;
            }
            if self.costs.step[l][0] != 0 {
                for nd in 0..8 {
                    let n = l * per + ((y as i32 + DY[nd]) as usize) * w + (x as i32 + DX[nd]) as usize;
                    if seen[n >> 6] & (1 << (n & 63)) != 0 {
                        continue;
                    }
                    seen[n >> 6] |= 1 << (n & 63);
                    let (nx, ny) = ((n % per) % w, (n % per) / w);
                    let far = crate::geom::hypot(f.wx(nx) - pad.x, f.wy(ny) - pad.y);
                    if sdist(&f.cells[n], net) < t {
                        stops.push((far, n, false));
                    } else if off(n) {
                        stops.push((far, n, true));
                    } else {
                        todo.push(n);
                    }
                }
            }
            if site(k) {
                for l2 in (0..nl).filter(|&l2| l2 != l && self.costs.step[l2][0] != 0) {
                    let n = l2 * per + k;
                    if seen[n >> 6] & (1 << (n & 63)) == 0 && !off(n) {
                        seen[n >> 6] |= 1 << (n & 63);
                        todo.push(n);
                    }
                }
            }
        }
        stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
        let mut names: Vec<String> = Vec::new();
        for &(_, n, rule) in &stops {
            let (l, k) = (n / per, n % per);
            let at = (f.wx(k % w), f.wy(k / w));
            let name = if rule { Some(format!("its own layer rule (it is kept to {})", self.kept_to(net))) } else { self.fixed_at(at, l, net) };
            if let Some(name) = name {
                if !names.contains(&name) {
                    names.push(name);
                    if names.len() == 4 {
                        break;
                    }
                }
            }
        }
        names
    }

    /// The layers a net is kept to, by name.
    pub(crate) fn kept_to(&self, net: u16) -> String {
        let keep = self.sc.nets[net as usize].keep;
        (0..self.sc.layers.len()).filter(|l| keep & (1 << l) != 0).map(|l| self.sc.layers[l].clone()).collect::<Vec<_>>().join(", ")
    }

    /// The fixed thing of another net (or of none) nearest a point on a layer, as a person would call it.
    fn fixed_at(&self, at: (f64, f64), l: usize, net: u16) -> Option<String> {
        let s = &self.statics;
        let (cx, cy) = s.bucket(at.0, at.1);
        let mut best: Option<(f64, usize)> = None;
        for &i in &s.buckets[cy * s.nx + cx] {
            let o = &s.objs[i as usize];
            if (o.net == net && o.net != 0) || o.lmask & (1 << l) == 0 {
                continue;
            }
            let d = o.shape.dist(at.0, at.1);
            if best.map_or(true, |b| d < b.0) {
                best = Some((d, i as usize));
            }
        }
        let o = &s.objs[best?.1];
        let of = |n: u16| if n == 0 { String::new() } else { format!(" ({})", self.sc.nets[n as usize].name) };
        Some(match o.kind {
            Kind::Edge => "the board's edge".into(),
            Kind::Hole => "a hole".into(),
            Kind::Keepout => "a keep-out".into(),
            Kind::Pad | Kind::Fence => {
                // the pad the copper is, or the pad a fence is the way out of
                let pad = self.sc.pads.iter().filter(|q| q.net == o.net).min_by(|a, b| o.shape.dist(a.x, a.y).partial_cmp(&o.shape.dist(b.x, b.y)).unwrap());
                match (pad, o.kind) {
                    (Some(q), Kind::Fence) => format!("the way out of {}.{}{}", q.ref_, q.num, of(o.net)),
                    (Some(q), _) => format!("pad {}.{}{}", q.ref_, q.num, of(o.net)),
                    _ => format!("a pad{}", of(o.net)),
                }
            }
            Kind::Zone => format!("a pour{}", of(o.net)),
            Kind::Track | Kind::Via | Kind::Existing => format!("copper the board came with{}", of(o.net)),
        })
    }
}
