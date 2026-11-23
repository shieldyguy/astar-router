//! Crossing: a track across another net's pour pays for it.

use super::{Pass, Step};
use crate::route::fill::{scan, set};
use crate::route::Router;
use crate::search::Price;

pub const PASS: Pass = Pass {
    name: "cross",
    about: "a step across another net's pour costs more than a step beside it: a pour is cut where the way round is dearer, and not for nothing",
    step: Step::Plan(Router::build_cross),
};

impl Router {
    /// Crossing: the board's pours are copper someone meant to be there, a ground under the signals, a
    /// wide path for a current. A track of another net that runs across one cuts it. So every node
    /// inside a pour costs every other net `Cfg::cross_mul` steps more than a node outside (the cost
    /// term "pour"). Where pours overlap, the node is the pour's that KiCad fills there: the one of the
    /// highest priority. A pour on a layer that is a net's whole plane is not priced: no track goes there.
    /// Nor is any other pour of a net that has a plane: the plane is what lies under the signals, and its
    /// pours on the other layers fill what the tracks leave.
    /// The pour of a net joined off this board is its host's, and is taken to lie under the whole board.
    /// Nor is a place where every layer is poured: the price says which layer to cross on, and there
    /// none is better.
    pub(crate) fn build_cross(&mut self) {
        let extra = (self.cfg.cross_mul * 1000.0 * self.f.pitch).round() as u32;
        if extra == 0 {
            return;
        }
        let per = self.f.w * self.f.h;
        let mut zones: Vec<usize> = (0..self.sc.copper.zones.len())
            .filter(|&k| {
                let z = &self.sc.copper.zones[k];
                z.net != 0 && z.outline.len() >= 3 && !(z.locked && z.min_width == Some(0.0)) && self.routable.contains(&z.layer) && self.sc.nets[z.net as usize].plane.is_none()
            })
            .collect();
        zones.sort_by_key(|&k| std::cmp::Reverse(self.sc.copper.zones[k].priority));
        let mut taken = vec![0u64; per * self.f.nl / 64 + 1];
        let mut charge: Vec<(usize, u16)> = Vec::new();
        for k in zones {
            let z = &self.sc.copper.zones[k];
            let mut inside = vec![0u64; per / 64 + 1];
            scan(&self.f, &z.outline, |y, xa, xb| (xa..=xb).for_each(|x| set(&mut inside, y * self.f.w + x)));
            for h in &z.holes {
                scan(&self.f, h, |y, xa, xb| (xa..=xb).for_each(|x| inside[(y * self.f.w + x) >> 6] &= !(1 << ((y * self.f.w + x) & 63))));
            }
            for i in 0..per {
                let ci = z.layer * per + i;
                if inside[i >> 6] & (1 << (i & 63)) != 0 && taken[ci >> 6] & (1 << (ci & 63)) == 0 {
                    set(&mut taken, ci);
                    charge.push((ci, z.net));
                }
            }
        }
        // a net joined off this board (`Cfg::drops`) has its host's pour under the whole of the layer its
        // vias go down to
        for &net in &self.hosted {
            let Some(l) = self.sc.nets[net as usize].plane else { continue };
            for ci in l * per..(l + 1) * per {
                if taken[ci >> 6] & (1 << (ci & 63)) == 0 {
                    set(&mut taken, ci);
                    charge.push((ci, net));
                }
            }
        }
        // where every layer a track may use is poured, none is the better one to cross on, and a price on
        // all of them would only make a via cheap beside a millimetre of track: nothing is charged there
        let poured = |i: usize| self.routable.iter().all(|&l| taken[(l * per + i) >> 6] & (1 << ((l * per + i) & 63)) != 0);
        let charge: Vec<(usize, u16)> = charge.into_iter().filter(|&(ci, _)| !poured(ci % per)).collect();
        let term = self.costs.term("pour");
        for (ci, net) in charge {
            self.costs.charge(&mut self.f, term, ci, net, Price::flat(extra));
        }
    }
}
