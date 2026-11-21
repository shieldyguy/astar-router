# astar: the plan to par with KRT, and past it on egress

Written 2026-11-16 from a read of KRT on the `krt-better` branch (its docs, the 94 flags of `route.py`, the
78 registered tools) and of astar as it stands (`../README.md`). Nothing here is built yet.

**The goal.** astar takes any KiCad board KRT can route and hands back a routed board to the same bar,
in seconds: every net landed, 0 new DRC errors by the house KiCad. And it plans egress before it lays
copper, which KRT does not.

## How this is meant to be run

    Chris
      one lead           keeps this file, the gates and the order; settles questions between tracks
        15 orchestrators one track each (T0 ... T14); each reads this file and its own track file only
          implementers   one package each (T4.3 and so on)

- A **track** is one file here. It names what the track owns, what it needs from other tracks, and its packages.
- A **package** is one implementer's job: one change, at least one fixture, one measured result.
  An orchestrator may split a package; it may not merge two into one change.
- A track's packages are in dependency order unless marked `(parallel)`.

## The order

| Wave | Tracks | Starts when | Ends at gate |
|------|--------|-------------|--------------|
| 0 | T0 Foundation, T13 Proof (starts, never stops) | now | G0: hooks frozen; the five bench boards give the same copper as before; the gate script runs |
| 1 | T1 Board I/O, T2 Rules, T3 Existing copper, T4 Egress plan, T12 Reporting | G0 | G1 Reach: corpus boards route from `.kicad_pcb` to `.kicad_pcb`; none crash; each is judged at its own rules |
| 2 | T5 Escapes, T6 Planes and power, T7 Impedance, T10 Looks, T11 Speed | G1 (T5 needs T4; T6 needs T2, T3) | G2: BGA boards, two-layer boards and plane boards land clean |
| 3 | T8 Pairs, T9 Matching, T14 Doors | G2 (T8 needs T7, T4; T9 needs T8; T14 needs T1, T3, T12) | G3 Features: every feature fixture passes |
| 4 | T13 full corpus beside KRT; every track fixes its tail; T14 switches the house chains | G3 | G4 Par: on every corpus board both routers finish, astar lands at least as many nets with no more new DRC errors; exceptions named one by one |

Two spikes run in wave 1 so waves 2 and 3 do not start blind: T5.0 (one BGA quadrant escaped by hand-written
rules, to learn what the egress plan must give it) and T8.0 (one pair routed as a wide virtual track on the
dock USB pair). Each is two days of one implementer and ends in a note, not in merged code.

## What every track builds on (T0 makes these; after G0 they change only through the lead)

Source layout after T0:

    src/main.rs        the command line, nothing else
    src/scene/         scene format 2: types, reader, validator          (T0, then T1 fills it)
    src/io/            KiCad reader and writer                           (T1)
    src/rules/         what applies between any two things               (T2)
    src/field/         the clearance field                               (T11 owns the hot path)
    src/search/        the A*, its queue, the cost hook                  (T11 owns the hot path)
    src/route/         jobs, negotiation, laying and lifting copper      (T0, then T3)
    src/passes/        one file per planning pass: egress, fences, drops, pours, pairs, match ...
    src/finish/        pull tight, arcs, cleanup, teardrops              (T10)
    src/report/        the report and its reasons                        (T12)
    tests/             fixtures and gates                                (T13)

The five hooks. A track adds behaviour through these and does not edit another track's files:

1. **Pass.** A planning or finishing step with a name, run in a fixed order, timed, switchable. Fences,
   drops, the pour, the route, pull tight and arcs become passes in T0.
2. **Cost term.** A named cost layer the search adds per step (today's lane toll becomes one). Bus
   attraction, corridors, egress reservations and proximity penalties are cost terms. No track edits the
   search's inner loop except T11.
3. **Job kind.** What the negotiation routes: a pad-to-tree connection, a plane drop, later a pair, a tap.
4. **Copper kind.** Every piece of copper in the field says what it is (pad, fence, track, via, zone,
   existing), whose it is, which job laid it and whether it is locked.
5. **Report entry.** `note(kind, net, where, detail)`: open nets, narrowings, findings, timings.

**Scene 2** is the one contract between reading a board and routing it. Everything stays testable from
JSON. It adds to scene 1.1: net classes with pair clearance, per-layer rule overrides, pad clearance
overrides, via sizes per class, fab floors, the stackup with thickness and dielectric, copper already on
the board (tracks, arcs, vias, zones with their fill, locked flags), keep-outs by what they forbid, pairs,
match groups, impedance targets, groups of parts, which nets and parts are in scope. `docs/scene.md` (T0.2)
is its definition; a reader meeting a key it does not know says so and stops.

## The judges (in this order; the router's own count is never the grade)

1. The house `kicad-cli` (10.99): zones refilled, DRC run. Unconnected 0; errors new against the bare board 0.
2. KRT's checkers, run from the stock checkout, as a second opinion: `check_connected.py`, `check_drc.py` at
   the board's clearance, `check_weird.py`, `py_tools/check_orphan_stubs.py`, `check_complete.py`.
3. astar's own exact check (`astar check`, T13.7), for fuzzing where KiCad is too slow. Written by
   someone who did not write the router code it checks.

## Gates every package passes before it merges (`./gate.sh`, T0.8)

- **Same in, same out.** Two runs of one input give the same plan, byte for byte.
- **The bench five.** dock, g474, ccb, tb150, ub400: every net landed, 0 new DRC errors, each within 1.2x
  of its recorded time on the same machine.
- **The track's fixtures**, old and new.
- **Corpus smoke.** Twenty named boards: no crash, no board worse than its last row.
- **Only what worked.** A change that measured neutral or worse is not merged, not as an option either.
  A new flag needs a board it wins on and none it loses on, over at least three boards.

## Rules for orchestrators

- Read this file and your track file. Read `../README.md` for how astar works today.
- You own your track's files. A change you need in another track's file goes to that orchestrator as a
  request with the reason; a change to a hook goes to the lead.
- One branch a package (`t4/3-exit-candidates`), merged into the track branch when its gates pass; the
  track branch merges to main at the wave gate, or earlier when the lead agrees.
- Each package ends in a short note: what changed, the gate's numbers before and after, what is still open.
- Stop and ask the lead when: a package needs a decision that is Chris's (listed in the doc), a gate cannot
  be met for a reason outside the track, or two packages turn out to be one.
- Never push to `drandyhaas/KiCadRoutingTools`. Do not change the stock checkout at
  `~/tools/vendor/KiCadRoutingTools`. On Chris's machine: two heavy processes at most, niced.
- Tune on the corpus, never on the bench five alone.

## Rules for implementers

- Write the fixture first: the smallest scene or board that shows the thing missing, and what right looks like.
- No panics on anything an input can cause. No silent narrowing: what is narrowed is reported.
- Say what you measured, on which boards, with which command. "Should work" is not a result.
- Leave no knob behind that you did not prove.

## Left out, on purpose

Placement (KRT's `py_placer`), the SWIG GUI plugin (the house KiCad has no `pcbnew` module), KRT's AI
skills (they read boards, not routers, and still work), movies, schematic back-annotation (astar
reports a swap; it does not edit a schematic), blind and buried vias as something to route (read as fixed
copper only; KRT does not route them either), rigid-flex.

## The tracks

| File | Track | Packages |
|------|-------|---------:|
| `T00-foundation.md` | Foundation | 8 |
| `T01-board-io.md` | Board I/O | 10 |
| `T02-rules.md` | Rules | 9 |
| `T03-existing-copper.md` | Existing copper and scope | 8 |
| `T04-egress.md` | Egress plan | 11 |
| `T05-escapes.md` | Escapes | 9 |
| `T06-planes-power.md` | Planes and power | 10 |
| `T07-impedance.md` | Impedance | 5 |
| `T08-pairs.md` | Pairs | 9 |
| `T09-matching.md` | Matching | 7 |
| `T10-looks.md` | Looks | 9 |
| `T11-speed.md` | Speed and scale | 8 |
| `T12-reporting.md` | Reporting | 8 |
| `T13-proof.md` | Proof | 9 |
| `T14-doors.md` | Doors | 8 |
