# T13 Proof

**Starts in wave 0 and never stops.** Mission: "route anything" as a number. Five boards proved astar can
work; this track finds where it does not.

Owns: `tests/`, `corpus/`, the scoreboard. Works for every track: each track's fixtures live here.
Reference: KRT `tests/stress/` (RUNBOOK.md, `fetch_*.py`, `ab_replay_grade.py`, the cloud recipes),
router-bench (`bench.py`, CONTRACT.md).

### T13.1 The corpus
Build: one list of boards with what each holds (layers, nets, area arrays, pairs, planes, copper already
there): router-bench's five; KRT's `kicad_files/` (about twenty boards); the house boards in `~/tools`
(114 files: fixtures, library blocks); KRT's stress sets, fetched by its own scripts (hundreds of open
boards; they live on Chris's Mac under `~/Documents/kicad_stress_test`).
Done when: `corpus/manifest.json` lists every board with its facts and a class: must route, hard, known bad.

### T13.2 The runner
Build: `corpus run astar|krt`: a row per board (landed, KiCad's unconnected, new DRC, seconds, mm, vias,
narrowed), the artifacts kept, a time budget, resumable, two heavy jobs at most on the Mac.
Done when: both routers run the whole list unattended.

### T13.3 The judges
Build: the three judges of `README.md` wired in; the bare board's own DRC as each board's baseline.
Done when: a planted short, a planted open and a planted clearance error are each caught by judge 1.

### T13.4 The scoreboard
Build: astar beside KRT per board and per feature class; every board astar is worse on, by name.
Done when: it is one page Chris can read on a phone.

### T13.5 Feature fixtures
Build: with each track, the small boards its packages name. One feature a board, each with what right
looks like.
Done when: every package in every track points at a fixture that exists.

### T13.6 Fuzzing
Build: random scenes (pads on and off the grid, random rules, random copper already there) and the
properties that must hold: no crash, same in same out, every net called joined is joined, the exact check
finds no clearance error.
Done when: a night of fuzzing finds nothing new.

### T13.7 The independent check
Build: `astar check`: connectivity and clearance of a plan against its scene, exact, sharing no code with
the router beyond the geometry primitives, written by someone else.
Done when: it catches every planted error of T13.3 and agrees with KiCad on the corpus.

### T13.8 The speed ledger
Build: seconds per board per commit on the bench five and ten big boards.
Done when: the gate reads it.

### T13.9 Scale
Build: the corpus on rented cores, as KRT's cloud recipes do, for whole-corpus runs in minutes.
Done when: a full A/B of two commits costs about a dollar and under half an hour.
