//! The search's queue.

#[derive(Clone, Copy)]
pub(crate) struct Entry {
    pub(crate) g: u32,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) l: u8,
}

/// A bucket queue whose scores may go down as well as up: with a weighted heuristic a step towards the
/// goal can score lower than the one before it. A bitmap over the buckets, and one over that, find the
/// lowest bucket in use in a few instructions however far back it lies.
pub(crate) struct Queue {
    buckets: Vec<Vec<Entry>>,
    occ: Vec<u64>,
    sum: Vec<u64>,
    cur: usize,
    shift: u32,
}

const MAX_BUCKETS: usize = 1 << 22;

impl Queue {
    pub(crate) fn new(shift: u32) -> Queue {
        Queue { buckets: Vec::new(), occ: Vec::new(), sum: Vec::new(), cur: 0, shift }
    }

    pub(crate) fn clear(&mut self) {
        for s in 0..self.sum.len() {
            let mut sb = self.sum[s];
            while sb != 0 {
                let w = s * 64 + sb.trailing_zeros() as usize;
                sb &= sb - 1;
                let mut bits = self.occ[w];
                while bits != 0 {
                    self.buckets[w * 64 + bits.trailing_zeros() as usize].clear();
                    bits &= bits - 1;
                }
                self.occ[w] = 0;
            }
            self.sum[s] = 0;
        }
        self.cur = usize::MAX;
    }

    #[inline(always)]
    pub(crate) fn push(&mut self, f: u64, e: Entry) {
        // a score past the queue's range is no path worth having (it is over a hundred metres of track,
        // in prices): it is dropped. Kept and squeezed into the last bucket, such nodes came out in the
        // order they went in and the search wandered: one net of five pads was laid with 5,461 vias.
        if (f >> self.shift) as usize >= MAX_BUCKETS - 1 {
            return;
        }
        let b = (f >> self.shift) as usize;
        if b >= self.buckets.len() {
            let n = (b + 1 + b / 4).min(MAX_BUCKETS).max(1024);
            self.buckets.resize_with(n, Vec::new);
            self.occ.resize(n / 64 + 1, 0);
            self.sum.resize(n / 4096 + 1, 0);
        }
        self.buckets[b].push(e);
        self.occ[b >> 6] |= 1 << (b & 63);
        self.sum[b >> 12] |= 1 << ((b >> 6) & 63);
        if b < self.cur {
            self.cur = b;
        }
    }

    #[inline(always)]
    pub(crate) fn pop(&mut self) -> Option<Entry> {
        loop {
            if self.cur >= self.buckets.len() {
                return None;
            }
            let mut w = self.cur >> 6;
            let mut bits = self.occ[w] & (!0u64 << (self.cur & 63));
            if bits == 0 {
                // the next word with a bucket in use
                let mut s = (w + 1) >> 6;
                if s >= self.sum.len() {
                    self.cur = usize::MAX;
                    return None;
                }
                let mut sb = self.sum[s] & (!0u64 << ((w + 1) & 63));
                loop {
                    if sb != 0 {
                        w = s * 64 + sb.trailing_zeros() as usize;
                        bits = self.occ[w];
                        break;
                    }
                    s += 1;
                    if s >= self.sum.len() {
                        self.cur = usize::MAX;
                        return None;
                    }
                    sb = self.sum[s];
                }
            }
            let b = w * 64 + bits.trailing_zeros() as usize;
            self.cur = b;
            let e = self.buckets[b].pop();
            if self.buckets[b].is_empty() {
                self.occ[w] &= !(1 << (b & 63));
                if self.occ[w] == 0 {
                    self.sum[w >> 6] &= !(1 << (w & 63));
                }
            }
            if e.is_some() {
                return e;
            }
        }
    }

    /// Take every entry out (they are scored again when a search changes phase).
    pub(crate) fn drain(&mut self, out: &mut Vec<Entry>) {
        for s in 0..self.sum.len() {
            let mut sb = self.sum[s];
            while sb != 0 {
                let w = s * 64 + sb.trailing_zeros() as usize;
                sb &= sb - 1;
                let mut bits = self.occ[w];
                while bits != 0 {
                    out.append(&mut self.buckets[w * 64 + bits.trailing_zeros() as usize]);
                    bits &= bits - 1;
                }
                self.occ[w] = 0;
            }
            self.sum[s] = 0;
        }
        self.cur = usize::MAX;
    }
}
