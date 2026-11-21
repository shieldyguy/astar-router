//! Debugging: what stands round a pad.

use super::Router;
use crate::field::{ddist, sdist};

impl Router {
    /// Debugging: how far can a pad (REF.NUM) get over fixed copper alone, and what stands round its access nodes?
    pub(crate) fn probe(&self, which: &str) {
        let Some(p) = (0..self.sc.pads.len()).find(|&i| format!("{}.{}", self.sc.pads[i].ref_, self.sc.pads[i].num) == which) else {
            eprintln!("probe: no pad {}", which);
            return;
        };
        let pad = &self.sc.pads[p];
        let net = pad.net;
        let t = self.t_min(net);
        let l = self.pad_layers(p)[0];
        eprintln!("probe {} net {} at ({:.3}, {:.3}) layer {} t_min {} access {:?}", which, self.sc.nets[net as usize].name, pad.x, pad.y, l,
            t, self.acc[p].iter().map(|a| (self.f.wx(a.x as usize), self.f.wy(a.y as usize), a.via.is_some())).collect::<Vec<_>>());
        for a in &self.acc[p] {
            for dy in -3i32..=3 {
                let mut line = String::new();
                for dx in -6i32..=6 {
                    let (x, y) = ((a.x as i32 + dx) as usize, (a.y as i32 + dy) as usize);
                    let c = &self.f.cells[self.f.idx(l, y, x)];
                    line.push_str(&format!(" {:>3}{}{:<3}", sdist(c, net), if c.sn1 == net && c.sd1 == 0 { '*' } else { '/' }, ddist(c, net)));
                }
                eprintln!("   {}", line);
            }
        }
        // flood over fixed copper only, tracks on this layer and vias
        let mut seen = vec![false; self.f.cells.len()];
        let mut stack: Vec<(usize, usize, usize)> = Vec::new();
        for a in &self.acc[p] {
            for &ll in &self.pad_layers(p) {
                stack.push((ll, a.y as usize, a.x as usize));
                seen[self.f.idx(ll, a.y as usize, a.x as usize)] = true;
            }
        }
        let mut count = 0usize;
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        let mut vias = 0;
        while let Some((ll, y, x)) = stack.pop() {
            count += 1;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            for d in 0..8 {
                let (nx, ny) = (x as i32 + crate::search::DX[d], y as i32 + crate::search::DY[d]);
                if nx < 0 || ny < 0 || nx >= self.f.w as i32 || ny >= self.f.h as i32 {
                    continue;
                }
                let i = self.f.idx(ll, ny as usize, nx as usize);
                if !seen[i] && sdist(&self.f.cells[i], net) >= t {
                    seen[i] = true;
                    stack.push((ll, ny as usize, nx as usize));
                }
            }
            let vi = y * self.f.w + x;
            let vl = self.costs.via_net[vi];
            if self.via_ok[vi >> 6] & (1 << (vi & 63)) != 0 && (vl == 0 || vl == net) {
                vias += 1;
                for &l2 in &self.routable {
                    let i = self.f.idx(l2, y, x);
                    if !seen[i] {
                        seen[i] = true;
                        stack.push((l2, y, x));
                    }
                }
            }
        }
        eprintln!("probe: {} nodes reachable over fixed copper, x {:.1}..{:.1}, y {:.1}..{:.1}, {} via sites", count, self.f.wx(x0), self.f.wx(x1), self.f.wy(y0), self.f.wy(y1), vias);
    }
}
