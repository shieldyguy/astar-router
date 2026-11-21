# T14 Doors

**Wave 3. Needs T1, T3, T12.** Mission: astar is reachable from everywhere KRT is reachable today in the
house: the bench, `pcb`, the chains, the editor.

Owns: the adapters and scripts outside the router. Reference: `~/tools/pcb/docs/routing.md` (the rulings,
both chains, steps 13 to 21), `~/tools` `pcb krt`, router-bench CONTRACT.md, `~/kicad` (the Braid build:
no `pcbnew` module; Python reaches KiCad through kipy and the API server).

### T14.1 The bench
Build: the board-to-board adapter on the scoreboard, run on the Mac.
Done when: astar's rows sit beside krt_better's with Mac seconds.

### T14.2 The `pcb` door
Build: `pcb astar BOARD ...`: sizes from the circuit file's class lines, floors from `fab-overrides.txt`,
runs in `build/pcb/route-N/`, the `pcb keep` manners.
Done when: a house block routes through it with no flag typed by hand.

### T14.3 The chains
Build: the 4-layer and 2-layer chains with astar doing steps 3 to 12 in one run, and the same stops with
the same words (`chain FAILED at NAME`, `placement:` lines).
Done when: each stop of `routing.md` can be provoked and reads as it does today.

### T14.4 The grade
Build: step 15's graders pass on astar's boards: `check_connected`, `pcb drc`, `check_drc`, `check_weird`,
`check_orphan_stubs`, `board_score`, `check_complete`. Where one cannot, a ruled exception with its reason.
Done when: the house fixtures grade done.

### T14.5 The editor
Build: as Chris rules (see the doc's decisions): a kipy plugin that routes the open board and writes the
tracks back as one undo step, or a `kicad-cli pcb route-astar` verb in the Braid build.
Done when: a board open in KiCad is routed from a menu and undone with one Ctrl-Z.

### T14.6 Releases
Build: built binaries for macOS arm64 and Linux x86_64 on GitHub Releases; an install script; the version
in `--capabilities`.
Done when: a fresh machine has a working astar in one command.

### T14.7 The words
Build: the robbie skill's map, a routing card for astar beside KRT's, astar's README, `docs/scene.md`.
Done when: a fresh session routes a house board from the card alone.

### T14.8 From KRT's flags
Build: a table of every KRT flag a house chain or a recorded run uses, with astar's equivalent or why
none is needed.
Done when: every flag in `chain-4layer.sh` and `chain-2layer.sh` has its row.
