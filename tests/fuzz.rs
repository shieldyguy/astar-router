//! A mutation fuzzer for what takes a scene: the two fixtures in tests/scenes/ are bent out of shape
//! (values swapped for nasty ones, keys dropped, added and renamed, lists cut and doubled, pieces grafted
//! from elsewhere, the text itself torn) and given to the reader; what still is a scene is routed, and
//! its plan written. Every one must end as a plan or as an error in words; a panic is a failure, and
//! the seed that made it is printed.
//!
//!     cargo test --release --test fuzz                 300 scenes, every time
//!     cargo test --release --test fuzz -- --ignored    10,000 scenes, and every value of the fixtures
//!                                                      changed alone, each way it can be
//!
//! ASTAR_FUZZ_N and ASTAR_FUZZ_SEED change the count and the first seed.

use astar::json::{self, J};
use astar::route::{route, Cfg};
use astar::scene::Scene;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

fn count(j: &J) -> usize {
    1 + match j {
        J::Arr(v) => v.iter().map(count).sum(),
        J::Obj(v) => v.iter().map(|(_, x)| count(x)).sum(),
        _ => 0,
    }
}

/// The k-th value of the tree, counting depth first.
fn nth<'a>(j: &'a mut J, k: &mut usize) -> Option<&'a mut J> {
    if *k == 0 {
        return Some(j);
    }
    *k -= 1;
    match j {
        J::Arr(v) => v.iter_mut().find_map(|x| nth(x, k)),
        J::Obj(v) => v.iter_mut().find_map(|(_, x)| nth(x, k)),
        _ => None,
    }
}

const NUMBERS: &[f64] = &[0.0, -0.0, 1.0, -1.0, 0.5, 1e-9, -1e-9, 1e-300, 0.001, 0.01, 2.5, 99.0, 101.0, 1999.0, 2001.0, 65535.0, 65536.0, 99999.0, 100001.0, 1e9, -1e9, 1e15, 1e300, -1e300, 4294967296.0, 9007199254740993.0];
const STRINGS: &[&str] = &["", " ", "x", "F.Cu", "B.Cu", "In1.Cu", "In2.Cu", "In9.Cu", "GND", "VCC", "SIG", "SIG_P", "AUX", "Default", "Power", "signal", "rail", "ground", "F", "B", "mm", "U1", "J1", "copper", "core", "vias", "tracks", "thermal", "time", "\u{0}", "\u{fffd}\u{1f600}", "a\"b\\c\n"];
const KEYS: &[&str] = &["colour", "", "net", "layer", "at", "shape", "poly", "circle", "oval", "line", "arc", "r", "name", "class", "role", "width", "clearance", "via", "drill", "layers", "version", "nets", "pads", "copper", "locked", "fill", "holes", "outline", "tht", "side", "forbid", "scope", "parts"];

fn nasty(r: &mut Rng) -> J {
    match r.below(12) {
        0 => J::Null,
        1 => J::Bool(r.below(2) == 0),
        2 | 3 | 4 => J::Num(*r.pick(NUMBERS)),
        5 | 6 => J::Str(r.pick(STRINGS).to_string()),
        7 => J::Arr(vec![]),
        8 => J::Obj(vec![]),
        9 => J::Arr((0..r.below(7)).map(|_| J::Num(*r.pick(NUMBERS))).collect()),
        10 => J::Arr(vec![J::Arr(vec![J::Num(*r.pick(NUMBERS)), J::Num(*r.pick(NUMBERS))]); r.below(5)]),
        _ => J::Obj(vec![(r.pick(KEYS).to_string(), J::Num(*r.pick(NUMBERS)))]),
    }
}

/// Every key of the tree with the places (depth first) of the values under it and the strings under it.
fn survey(j: &J, key: &str, k: &mut usize, keys: &mut Vec<(String, Vec<usize>, Vec<String>)>) {
    let i = match keys.iter().position(|e| e.0 == key) {
        Some(i) => i,
        None => {
            keys.push((key.to_string(), Vec::new(), Vec::new()));
            keys.len() - 1
        }
    };
    keys[i].1.push(*k);
    *k += 1;
    match j {
        J::Arr(v) => v.iter().for_each(|x| survey(x, key, k, keys)),
        J::Obj(v) => v.iter().for_each(|(kk, x)| survey(x, kk, k, keys)),
        J::Str(s) => keys[i].2.push(s.clone()),
        _ => {}
    }
}

fn mutate(j: &mut J, r: &mut Rng) {
    let n = count(j);
    let mut keys = Vec::new();
    survey(j, "", &mut 0, &mut keys);
    let graft = {
        let mut k = r.below(n);
        nth(j, &mut k).cloned()
    };
    // one time in three any value; else a key first (so a key that is there once gets its turn as
    // often as one that is there a thousand times) and then one of its values
    let mut k = if r.below(3) == 0 {
        r.below(n)
    } else {
        let places = &r.pick(&keys).1;
        *r.pick(places)
    };
    let Some(x) = nth(j, &mut k) else { return };
    // a name is most often swapped for a name of its own kind: one that stands under a key where
    // this one stands too (a layer for a layer of the stack, a net for a net)
    if let (J::Str(s), true) = (&mut *x, r.below(4) > 0) {
        let kinds: Vec<&Vec<String>> = keys.iter().filter(|e| e.2.contains(s)).map(|e| &e.2).collect();
        if !kinds.is_empty() {
            let kind = *r.pick(&kinds);
            *s = r.pick(kind).clone();
            return;
        }
    }
    match (r.below(10), &mut *x) {
        (0, _) => *x = nasty(r),
        (1, _) => *x = graft.unwrap_or(J::Null),
        (_, J::Num(v)) => {
            *v = match r.below(9) {
                0 => -*v,
                1 => *v * 1000.0,
                2 => *v / 1000.0,
                3 => *v * 1e6,
                4 => *v + 0.05,
                5 => *v - 0.05,
                6 => 0.0,
                7 => v.round(),
                _ => *r.pick(NUMBERS),
            }
        }
        (_, J::Str(s)) => *s = r.pick(STRINGS).to_string(),
        (_, J::Bool(b)) => *b = !*b,
        (_, J::Arr(v)) => match r.below(7) {
            0 => v.clear(),
            1 if !v.is_empty() => {
                v.remove(r.below(v.len()));
            }
            2 if !v.is_empty() => {
                let e = v[r.below(v.len())].clone();
                v.insert(r.below(v.len() + 1), e);
            }
            3 if !v.is_empty() => v.truncate(r.below(v.len())),
            4 if v.len() > 1 => {
                let (a, b) = (r.below(v.len()), r.below(v.len()));
                v.swap(a, b);
            }
            5 if !v.is_empty() => {
                let e = v[r.below(v.len())].clone();
                for _ in 0..r.below(40) {
                    v.push(e.clone());
                }
            }
            _ => v.push(nasty(r)),
        },
        (_, J::Obj(v)) => match r.below(5) {
            0 if !v.is_empty() => {
                v.remove(r.below(v.len()));
            }
            1 if !v.is_empty() => {
                let i = r.below(v.len());
                v[i].0 = r.pick(KEYS).to_string();
            }
            2 if !v.is_empty() => {
                let e = v[r.below(v.len())].clone();
                v.push(e);
            }
            3 if !v.is_empty() => {
                let i = r.below(v.len());
                v[i].1 = nasty(r);
            }
            _ => v.push((r.pick(KEYS).to_string(), nasty(r))),
        },
        (_, J::Null) => *x = nasty(r),
    }
}

/// Tear the text itself: cut it short, change a byte, or drop some in.
fn tear(t: String, r: &mut Rng) -> String {
    let mut b = t.into_bytes();
    if b.is_empty() {
        return String::new();
    }
    match r.below(3) {
        0 => b.truncate(r.below(b.len())),
        1 => {
            let i = r.below(b.len());
            b[i] = *r.pick(b"{}[]\",:0-9eE.x \\\n\x00\xff");
        }
        _ => {
            let i = r.below(b.len());
            for _ in 0..r.below(6) + 1 {
                b.insert(i, *r.pick(b"{}[]\",:0-9eE.-+ \\u"));
            }
        }
    }
    String::from_utf8_lossy(&b).into_owned()
}

/// One bent scene from a seed.
fn bent(bases: &[J], seed: u64) -> String {
    let mut r = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    r.next();
    let mut j = r.pick(bases).clone();
    for _ in 0..1 + r.below(4) {
        mutate(&mut j, &mut r);
    }
    let t = json::write(&j);
    if r.below(5) == 0 {
        tear(t, &mut r)
    } else {
        t
    }
}

#[derive(Default)]
struct Tally {
    scenes: usize,
    errors: usize,
    /// of the scenes: routed (with how many left nets open), and refused by the router
    routed: usize,
    open: usize,
    refused: usize,
    panics: Vec<(u64, String)>,
}

/// What a panic said and where, kept by the hook (a routing thread's panic is seen here, not at the caller).
static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// What is done with every bent scene: read it, and what still is a scene goes through the whole
/// router and its plan is written. None of it may panic.
fn take(text: &str, seed: u64, t: &mut Tally) {
    let sc = match json::parse(text).and_then(|j| Scene::from_json(&j)) {
        Ok(sc) => sc,
        Err(e) => {
            assert!(!e.is_empty(), "an error without words");
            t.errors += 1;
            return;
        }
    };
    t.scenes += 1;
    // most at the pitch the router is tuned for, some coarser and finer; a small grid, a short leash
    let pitch = [0.1, 0.1, 0.1, 0.1, 0.2, 0.05, 0.5, 1.0][(seed % 8) as usize];
    let cfg = Cfg { pitch, max_cells: 2_000_000, budget_s: 3.0, two_ways: seed % 3 != 0, ..Cfg::default() };
    match route(sc, &cfg) {
        Ok(out) => {
            t.routed += 1;
            t.open += (out.exit_code(false) == astar::exit::OPEN) as usize;
            json::parse(&out.plan()).expect("the plan is JSON");
        }
        Err(e) => {
            assert!(!e.to_string().is_empty(), "an error without words");
            t.refused += 1;
        }
    }
}

/// The two fixtures; they must load themselves, or nothing bent from them is worth anything.
fn bases() -> Vec<J> {
    let bases: Vec<J> = ["every_field.json", "tiny11.json"]
        .iter()
        .map(|f| json::parse(&std::fs::read_to_string(format!("{}/tests/scenes/{}", env!("CARGO_MANIFEST_DIR"), f)).expect("the fixture is there")).expect("the fixture is JSON"))
        .collect();
    for b in &bases {
        Scene::from_json(b).expect("the fixture loads");
    }
    bases
}

fn fuzz(n: u64) -> Tally {
    let n = std::env::var("ASTAR_FUZZ_N").ok().and_then(|s| s.parse().ok()).unwrap_or(n);
    let first: u64 = std::env::var("ASTAR_FUZZ_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let bases = bases();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| SAID.lock().unwrap_or_else(|e| e.into_inner()).push(info.to_string())));
    let mut t = Tally::default();
    for seed in first..first + n {
        let text = bent(&bases, seed);
        if catch_unwind(AssertUnwindSafe(|| take(&text, seed, &mut t))).is_err() {
            let said = std::mem::take(&mut *SAID.lock().unwrap_or_else(|e| e.into_inner()));
            t.panics.push((seed, said.join(" | ")));
        }
    }
    std::panic::set_hook(hook);
    println!(
        "fuzz: {} bent scenes from seed {}: {} read errors, {} still scenes ({} routed, {} of them with nets open; {} refused by the router), {} panics",
        n,
        first,
        t.errors,
        t.scenes,
        t.routed,
        t.open,
        t.refused,
        t.panics.len()
    );
    for (seed, what) in t.panics.iter().take(10) {
        println!("  PANIC at ASTAR_FUZZ_SEED={} ASTAR_FUZZ_N=1: {}", seed, what);
    }
    t
}

/// Every value of both fixtures changed alone, each way it can be: a name for every name of its own
/// kind, a number for nasty numbers, a flag flipped, a list emptied, anything taken out. Where the random
/// fuzz may never hit the one key that matters, this hits every key.
fn sweep() -> Tally {
    let bases = bases();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| SAID.lock().unwrap_or_else(|e| e.into_inner()).push(info.to_string())));
    let mut t = Tally::default();
    let mut cases = 0u64;
    for (bi, base) in bases.iter().enumerate() {
        let mut keys = Vec::new();
        survey(base, "", &mut 0, &mut keys);
        for k in 0..count(base) {
            let mut probe = base.clone();
            let mut kk = k;
            let Some(x) = nth(&mut probe, &mut kk) else { continue };
            let mut swaps: Vec<J> = vec![J::Null];
            match x {
                J::Str(s) => {
                    let mut names: Vec<&String> = keys.iter().filter(|e| e.2.contains(s)).flat_map(|e| e.2.iter()).collect();
                    names.sort();
                    names.dedup();
                    swaps.extend(names.into_iter().filter(|n| *n != s).map(|n| J::Str(n.clone())));
                    swaps.push(J::Str(String::new()));
                }
                J::Num(_) => swaps.extend(NUMBERS.iter().skip(k % 3).step_by(3).map(|&v| J::Num(v))),
                J::Bool(b) => swaps.push(J::Bool(!*b)),
                J::Arr(_) => swaps.push(J::Arr(vec![])),
                _ => {}
            }
            for (vi, v) in swaps.into_iter().enumerate() {
                let mut j = base.clone();
                let mut kk = k;
                *nth(&mut j, &mut kk).unwrap() = v;
                let text = json::write(&j);
                cases += 1;
                if catch_unwind(AssertUnwindSafe(|| take(&text, cases, &mut t))).is_err() {
                    let said = std::mem::take(&mut *SAID.lock().unwrap_or_else(|e| e.into_inner()));
                    t.panics.push((cases, format!("fixture {}, value {}, change {}: {}", bi, k, vi, said.join(" | "))));
                }
            }
        }
    }
    std::panic::set_hook(hook);
    println!(
        "sweep: {} single changes: {} read errors, {} still scenes ({} routed, {} of them with nets open; {} refused by the router), {} panics",
        cases,
        t.errors,
        t.scenes,
        t.routed,
        t.open,
        t.refused,
        t.panics.len()
    );
    for (_, what) in t.panics.iter().take(10) {
        println!("  PANIC at {}", what);
    }
    t
}

#[test]
#[ignore]
fn every_value_changed_alone() {
    let t = sweep();
    assert!(t.panics.is_empty(), "{} panics", t.panics.len());
    assert!(t.routed > 0 && t.errors > 0 && t.open > 0);
}

#[test]
fn bent_scenes_are_errors_never_crashes() {
    let t = fuzz(300);
    assert!(t.panics.is_empty(), "{} panics", t.panics.len());
    // the fuzz must reach both sides: scenes that still load and scenes that are refused
    assert!(t.routed > 0 && t.errors > 0, "{} routed, {} errors: the fuzz is not bending", t.routed, t.errors);
}

#[test]
#[ignore]
fn ten_thousand_bent_scenes() {
    let t = fuzz(10_000);
    assert!(t.panics.is_empty(), "{} panics", t.panics.len());
    assert!(t.routed > 0 && t.errors > 0 && t.refused > 0 && t.open > 0);
}
