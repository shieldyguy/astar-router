# T11 Speed and scale

**Wave 2.** Mission: seconds stay seconds as features land and boards grow.

Owns: the hot paths of `src/field/` and `src/search/`. Every other track reaches them through the hooks.
Today: 130 to 150 ns a search step, bound by memory; ub400 3.6 s alone.

### T11.1 Where the time goes
Build: a profile of ub400 and three big corpus boards; a budget per pass that the gate checks.
Done when: the budget table is in `docs/speed.md` and the gate fails on 20% over.

### T11.2 Memory layout
Build: one record per search node (cost, parent, steps since a via); a per-column summary so a via check
is one read; a smaller field cell.
Done when: ub400's search is at least 1.5x faster, plans identical.

### T11.3 Threads
Build: nets that cannot touch (by their boxes) routed at the same time, merged in a fixed order so the
result does not depend on timing.
Done when: 4 cores give at least 2x on the big boards; same in, same out still holds.

### T11.4 Two grids
Build: a coarse grid for open board, the fine one only where a part needs it. Measured first: at 0.2 mm
ub400 routes in 1.3 s but dock and ccb leave fine-pitch pins open.
Done when: the bench five land whole in less total time than on one grid.

### T11.5 Big boards
Build: 6 to 12 layers, 2,000 nets, 300 mm a side: a memory cap, a time budget, a clean stop with the best
board so far.
Done when: the largest corpus boards finish or stop cleanly inside the budget.

### T11.6 The small change in a blink
Build: the field kept between runs, keyed by the board, so re-routing one net does not rebuild it.
Done when: T3.6's case runs under 0.2 s.

### T11.7 Tuning by data
Build: the negotiation's numbers (pressure, wear, prices) tuned on the corpus. A default changes only on
evidence from at least three boards and none worse.
Done when: each changed default has its table.

### T11.8 A global plan, if the data asks for it
Build: for boards the negotiation cannot settle, layers and corridors chosen on the coarse graph of T4.8
before the fine search.
Done when: it is built only if T13's scoreboard shows boards that need it, and it lands them.
