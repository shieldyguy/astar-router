# T0 Foundation

**Wave 0. One orchestrator, packages in order. Nothing else starts until G0.**

Mission: make astar a thing fifteen teams can work on at once without stepping on each other, and change
nothing about what it routes.

Owns: `src/main.rs`, `src/scene/`, `src/route/`, the pass list, the hook definitions, `gate.sh`.
Needs: nothing. Gives: the hooks and scene 2 to every track.
Gate G0: the five bench plans are byte-identical to the plans of commit `3ef9069`; the hooks are written
down in `docs/hooks.md`; `./gate.sh` passes.

### T0.1 The split
Build: `router.rs` (1,750 lines) into the layout in `README.md`. Moves only.
Done when: five bench plans byte-identical; no file over 600 lines; no warning.

### T0.2 Scene 2
Build: the types, the JSON reader, the validator, `docs/scene.md`. Reads 1.1 and 2. An unknown key is an
error that names it. Every new field is carried but unused until its track uses it.
Done when: the five scenes load as before; a scene with each new field loads; 10,000 mutated scenes give
errors, never a crash.

### T0.3 Passes
Build: the pass hook. Fences, drops, the pour choice, the route, pull tight and arcs each become a pass with
a name, an order, a timing line. `--passes` lists them; `--skip NAME` leaves one out.
Done when: plans identical; the report shows seconds per pass.

### T0.4 Cost terms
Build: the cost hook: named per-node cost layers summed by the search. The lane toll and the mouth rule
move onto it.
Done when: plans identical; ub400's search within 3% of its time before.

### T0.5 Copper kinds
Build: kind, owner job and locked flag on every object in the field and the exact index. Lifting refuses
locked copper.
Done when: plans identical; unit tests for lift and lock.

### T0.6 Errors, not panics
Build: every input-caused failure is a message and an exit code: 0 whole, 1 nets open, 2 bad input,
3 rules narrowed under `--strict`. `panic = abort` goes; a panic is a bug with a backtrace.
Done when: the fuzz of T0.2 run through the whole router: no crash in 10,000.

### T0.7 Library and commands
Build: astar as a library (`route(scene, cfg) -> plan`) with a thin command over it: `astar route`,
`astar check`, `astar explain`, `astar egress`. The old `astar SCENE PLAN` form keeps working.
Done when: the bench adapter runs unchanged; a ten-line program routes a scene through the library.

### T0.8 The gate script
Build: `./gate.sh`: same-in-same-out, the bench five through router-bench, unit tests; `--smoke` adds the
corpus smoke when T13.2 exists. One line a check, exit 1 on any failure.
Done when: it passes on main and fails on a planted regression of each kind.
