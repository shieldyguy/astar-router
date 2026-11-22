# The five hooks

A track adds behaviour to astar through these and does not edit another track's files. They are frozen
at G0: after that a hook changes only through the lead.

| hook | what it is | where it is defined |
|---|---|---|
| Pass | a named step of a run, in a fixed order, timed, switchable | `src/passes/mod.rs` |
| Job kind | what the negotiation routes | `src/route/job.rs` |
| Cost term | a named layer of prices the search adds to a step | `src/search/cost.rs` |
| Copper kind | what every object in the field is, whose, laid by which job, locked or not | `src/field/mod.rs` |
| Report entry | `note(kind, net, at, detail)`: what the run has to say | `src/report/mod.rs` |

The library is `astar` (`src/lib.rs`): `astar::route(scene, &cfg) -> Result<Routed, Error>` is the whole
run, `examples/route.rs` is a program that uses it, and the command (`src/main.rs`: `astar route`,
`astar check`, `astar egress`; `explain` is not built) is a command line round it and nothing more. A
track's command is a function in the library and one arm in `main.rs`.

How a run goes (`route::drive`): the board passes; the field, the exact index and the pads' access
nodes (`Router::new`); the plan passes; where a via may stand (`Router::find_via_sites`); the route
passes, which call the path passes for every path; the finish passes on the board that is kept.

## 1. Pass

    pub struct Pass { pub name: &'static str, pub about: &'static str, pub step: Step }

    pub enum Step {
        Board(fn(&mut Scene, &Cfg) -> Vec<(u16, usize)>),
        Plan(fn(&mut Router)),
        Route(fn(&mut Router)),
        Path(fn(&Router, u16, &[Node]) -> Vec<Node>),
        Finish(fn(&mut Router)),
    }

    pub const PASSES: &[Pass] = &[pour::PASS, fences::PASS, exits::PASS, cross::PASS, drops::PASS, route::PASS, tight::PASS, arcs::PASS];

A pass is one file with a `pub const PASS: Pass` and one entry in `PASSES` (`src/passes/mod.rs`, kept by
T0: ask for your line). The stage is the kind of `Step`; a run goes through the stages in the order
above, and within a stage in the order of `PASSES`.

- **Board**: before the field is built. May change the scene the router is given, and returns the planes
  it gave to nets as `(net, layer)`; those go to the plan's `planes`. (A net joined off the board,
  `--drops`, is in the plan's `planes` too, with `"host": true`: the writer draws no zone for it.) When a board pass changed the board
  it is routed both ways at once, changed and plain, and one is kept (below). `pour` is the one today.
- **Plan**: the field stands, nothing is routed. Add fixed copper (`Router::statics.add` and
  `Field::stamp_static`, both), cost terms, jobs (`Router::add_job`). `fences`, `exits`, `cross`, `drops`.
- **Route**: lays and lifts copper. `route` makes one job a net and runs the negotiation.
- **Path**: called with every path the route found, before it is laid: `(router, net, path)` gives the
  path to lay, same ends, same vias. A path through another net's copper is laid as found. `tight`.
- **Finish**: the copper stands; redraw it (`Job::tracks`). Runs once, on the board that is kept. `arcs`.

`astar --passes` lists them. `--skip NAME` (`Cfg::skip`) leaves one out; the plan's `summary.skipped`
and a line on stderr say so, and a pad whose pass was left out counts as not joined.

**Seconds.** `Router::clock` holds seconds per pass (`Clock::enter(i)`, `leave()`; `passes::index(name)`
gives `i`). The driver times every pass it calls. Work done for a pass inside another (a drop routed by
the negotiation, a path pulled tight) is on its own pass's clock, and the outer clock stops meanwhile:
the seconds add up to the run's. They are in the plan's `timing_s.passes` and, with `-v`, on stderr.
`field` is the entry for what no pass owns.

**Both ways.** The changed board and the plain one are routed on two threads. Which is kept depends on
their open nets and their search work (`Router::totals().2`, nodes expanded) alone, never on which
thread was done first: the changed one is kept when its work is at most the plain one's plus a quarter
plus a million, and it is whole or has fewer nets open. `--one-way` routes the changed board only.

## 2. Job kind

    pub struct JobKind {
        pub name: &'static str,
        pub pass: &'static str,                 // the pass on whose clock its routing runs
        pub route: fn(&mut Router, usize),      // route job j from nothing
    }

    Job::new(kind: JobKind, net: u16, pad: Option<usize>) -> Job
    Router::add_job(&mut self, job: Job) -> usize

A job is what the negotiation routes, lifts and routes again. Its kind says how. Today: `NET`
(`src/route/net.rs`: a net's pads joined one at a time into a tree) and `DROP` (`src/passes/drops.rs`:
one surface pad into its net's plane). A pair or a tap is a new `const JobKind` in its own file and a
plan pass that adds jobs of it.

`route(r, j)` finds the job's connections and lays them:

- ask the search: fill a `route::Ask` (sources, target, prices, limits) and call `r.solve(&ask)`, which
  gives the path and whether it may run through other nets' copper;
- when it may: `r.note_clashes(j, &path)` before laying, so the negotiation knows whom it clashes with;
- when it did not clash: `let path = r.shape(net, path)` (the path passes);
- lay it: `r.commit(j, &path, from_pad, to_pad)`, which writes tracks and vias to the field and to
  `r.jobs[j]`;
- say how it ended: `r.jobs[j].open` (pads not joined), `r.jobs[j].done = true`.

`Job::pad` is the one pad a job is about, when it is about one. A job whose track ends on another job's
copper says so with `r.deps[other].push(j)`: lifting `other` then lifts `j` too.

## 3. Cost term

    costs.term(name: &'static str) -> TermId                     // made the first time it is asked for
    costs.charge(&mut field, term, ci: usize, owner: u16, price: Price)
    costs.reserve_via(term, vi: usize, owner: u16)
    costs.terms[term.0 as usize].via_shared = PRICE

    Price::flat(v)         // v for every net
    Price::per_width(v)    // v for every default track width the paying net is wide

`costs` is `Router::costs` (`search::Costs`); `ci` is a cell's field index (`Field::idx(l, y, x)`), `vi` a
node's (`y * w + x`). Prices are the search's units: 1000 for a millimetre of track on an outer layer.

A cost term is a named layer of per-node prices. A plan pass makes one and writes it; the search adds
it. Nobody but T11 edits the search's inner loop.

- **`charge`**: every net but `owner` pays `price` more for a step onto that cell (`owner` 0: every net
  pays). Prices add up: two terms on one cell, or one term twice, are both paid, and a net is spared
  exactly what it owns there. A term that wants "the first claim wins" keeps its own record of what it
  has claimed, as the lanes do.
- **`reserve_via`**: the via site is reserved for `owner`: no other net may put a via there. A site
  reserved for several nets is open to each of them at `via_shared`, the price the term names, once for
  every term under which another net shares it; to every other net it stays closed.
- `-v` lists the terms with how many nodes each priced and how many via sites it reserved.

Today's terms: `lane` (the way out in front of a fine-pitch pin costs another net `--toll` a millimetre,
by the width) and `mouth` (no foreign via in a lane's mouth; where two mouths overlap, a via of either
net costs `mouth_mm` more), both written by the `fences` pass; and `pour` (a step inside another net's
pour costs `--cross` steps more), written by the `cross` pass.

How it is kept (T11's to change, not the calls above): all terms are summed as they are written into one
byte of the field's cell, `Cell::toll`, a price class, with the spared net in `Cell::toll_net`; a cell
that owes to more than one owner is marked `SEVERAL` and kept in a side table. So the search reads what
it read before: ub400's route took 2.825 s before the hook and 2.825 s with it.

What a term cannot say yet: a price that only some nets pay (a corridor: "net N pays outside it"), and
a via site that others may use at a price. Both need an addition to the hook: ask the lead.

## 4. Copper kind

    pub enum Kind { Edge, Hole, Keepout, Pad, Fence, Track, Via, Zone, Existing }     // field::Kind

    pub struct DynObj    { kind, net, job, locked, layer, ax, ay, bx, by, r, alive }   // routed copper
    pub struct StaticObj { kind, shape, lmask, net, extra, job, locked }              // fixed copper
    pub const NO_JOB: u32 = u32::MAX;

Every object says what it is (`kind`), whose it is (`net`, 0 for none), which job laid it (`job`, or
`NO_JOB`) and whether it is locked. There are two stores:

- **The field's routed copper**, `Field::objs` (`DynObj`): what jobs lay and the negotiation may lift.
  `Router::commit` lays tracks (`Track`) and vias (`Via`, on `ALL_LAYERS`) with the job's number.
  Lay your own with `Field::add_obj`; keep the id in `Job::objs` so that lifting the job lifts it.
- **The exact index**, `Router::statics.objs` (`StaticObj`): the fixed copper as shapes, for what the
  grid cannot answer. The board's own (`Edge`, `Hole`, `Keepout`, `Pad`) and the fences (`Fence`), made
  with `StaticObj::fixed(kind, shape, lmask, net, extra)`: no job, locked. Fixed copper is added to both
  the index and the field: `statics.add(obj)` and `f.stamp_static(&shape, lmask, net, extra)`.

`Existing` is the copper the board came with (tracks, vias, a zone's outline); nothing makes `Zone` yet.

**Locked copper is never lifted.** `Field::remove_objs(ids)` leaves locked objects where they are and
returns how many it refused. `Router::lock_job(j)` locks a job and all its copper; `Router::lift(j)`
then refuses it: the job is not lifted, nothing is lifted on its account, and the negotiation never
routes it again. The job that clashes with a locked job is the one that moves. Lifting another job
still lifts what leans on it (`Router::deps`), except what is locked.

## 5. Report entry

    Router::note(&mut self, kind: &'static str, net: u16, at: Option<(f64, f64)>, detail: String)

    pub struct Note { pub kind: &'static str, pub net: u16, pub at: Option<(f64, f64)>, pub detail: String }

A note says something about a net (0: none) or a place (mm). Notes are `Router::notes`, in the order they
were made; they go to the plan's `notes` (`{"kind", "net"?, "at"?, "detail"}`) and, with `-v`, to stderr.

Kinds today, all written at the end of a run by `route::route` (`Router::close_report`):

| kind | what | who reads it |
|---|---|---|
| `open` | a net with a pad not joined, at an open pad: `2 of 5 pads joined`, then why, pad by pad: `the board itself leaves no way to U1.2: U1.2 is shut in by pad U1.3 (SCK) and a keep-out` (what stays round the pad shuts it in, named: a matter for the placement or the rules) or `no way was found to U1.2 among the other nets: the nearest way runs through NRST at (24.10, 18.30)` (whose copper is in the way, and where) | exit 1; the plan's `left_out`; stdout, always (the first dozen) |
| `narrowed` | a net drawn narrower than it asks where its width did not fit: how many stretches, how long, down to what | exit 3 under `--strict` |
| `ends` | track ends at pads laid as the grid found them, a step beside the pad's own line, where that line was not clear: how many of how many | nobody yet |
| `kept` | the tracks and vias the board came with: how many, how many pads of how many nets stand on them | nobody yet |
| `touching` | pads that lie on another pad of their net, with no track between: how many, in how many places; they are joined as they are | nobody yet |
| `layers` | a net kept to layers (`--layers`, the net's `layers`): how much of it runs there, and how much on other layers from its pads to their vias | nobody yet |
| `not asked` | the nets the run was not asked to route (`--nets`, `--except`, the scene's `scope`): how many, the first eight by name | the plan's `not_asked`; the summary line |
| `loose` | a piece of old copper no pad stands on that could not be joined to the rest of its net, with where | the net counts as open: exit 1 |
| `ground` | a ground of three pads or more joined by track: the board has no pour and no plane for it | nobody yet |
| `drops` | a net joined off this board (`--drops`): how many pads go down by how many vias, which nothing joins here | nobody yet |
| `choice` | what the run chose between two boards (the pour) | the plan's `summary.note` |
| `unused` | something the scene carries that the router does not use yet | stderr, always |

A track adds its own kinds (a finding, a rule it could not keep). Three rules:

- **Nothing is narrowed in silence.** Whatever a pass gives up or bends (a width, a clearance, a pair's
  gap, a length it could not match) is a note. `narrowed` is the kind that `--strict` turns into exit 3.
- **A note is the same for the same input.** No seconds, no thread, nothing of the machine: two runs of
  one input give the same plan, byte for byte, but for `timing_s`. Seconds go on the clock (hook 1).
- **A note about copper is written when the copper stands.** The negotiation lifts and re-routes jobs: a
  note made while routing a job describes copper that may be gone. Write it in a finish pass, or look at
  the jobs at the end as `close_report` does.

## Errors, and what the command exits with

| exit | when | `astar::exit` |
|---|---|---|
| 0 | every net joined | `WHOLE` |
| 1 | a plan was written and nets are open | `OPEN` |
| 2 | the input could not be taken (the file, the JSON, the scene, a flag, the grid): no plan, the reason on stderr | `BAD_INPUT` |
| 3 | every net joined, a rule was narrowed, and `--strict` was given | `NARROWED` |
| 4 | a plan was written and astar's own check (`astar check`) finds fault with it: copper that breaks a rule, or a net the plan calls joined that is not. A fault in astar; the findings are on stderr | `FAULT` |
| 101 | a panic: a bug in astar, with a backtrace. Never the input's fault | |

`astar route SCENE PLAN` exits with these. The old form `astar SCENE PLAN` is the same run but exits 0
where the new one exits 1 (router-bench's adapter reads any other exit as "no plan").

- Whatever an input can cause is an `Err`, not a panic: `Scene::from_json` and `Scene::check` (the
  scene), `Cfg::check` (the settings), `route::check(&scene, &cfg)` (the two together: the grid is at
  most 65,000 nodes across and `Cfg::max_cells` cells in all, 120 million unless told otherwise; one
  copper layer at least is no net's plane). `route::route` runs them all and returns
  `Err(astar::Error::BadInput(reason))`.
- Past those checks code may `expect` what the checks made sure of. A pass that finds its own way to be
  given nonsense adds its check to `route::check`, or returns nothing and writes a note.
- `tests/fuzz.rs` holds every pass to it (the full gate runs it): 10,000 bent scenes and every value of two fixtures changed
  alone go through the whole router; a panic fails the test and prints what made it.
