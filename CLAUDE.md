# astar: Chris's board router

A grid A* PCB router in Rust with no dependencies: a placed KiCad board in, the routed board out. It is
judged by the house KiCad's DRC (`shieldyguy/kicad`, 10.99), never by its own word alone. README.md is how
it works and where it stands; this page is how to use it and what not to trust it with.

## Install it

    ./install.sh                  # cargo build --release (about a minute), then ~/.local/bin/astar
    ./install.sh --dest DIR       # somewhere else; --which says what it would do; --help the rest

Without cargo it takes the newest release's binary (Linux x86_64, macOS arm64; needs `gh` logged in to
this private repo); `./install.sh --release` takes it even where cargo is. A release is made only when
asked: `git push origin main:release` builds, tests and publishes `build-<sha12>` for both platforms.

## Use it

    astar route IN.kicad_pcb -o OUT.kicad_pcb --plan plan.json --scene scene.json

- It reads the board and the `.kicad_pro` beside it (classes, widths, clearances, minimums), routes every
  net that is not yet joined, keeps copper the board already has, and writes OUT with the project files
  copied beside it. A few seconds at most on the boards it is used for.
- Exit 0 every net joined, 1 nets open (`astar: open: NET: why` lines say which), 2 the input could not
  be read, 3 a width narrowed under `--strict`, 4 astar's own check found fault with its plan.
- The folder OUT goes in must exist already (exit 2 says so). Write OUT somewhere new, not over IN.
- `astar` with no arguments is the usage; `astar --passes` the passes in order.
- **Refill and judge before you believe it.** astar does not fill zones: a plane it pours has no fill in
  OUT, and a zone that came filled keeps its old fill. Always:

      kicad-cli pcb drc --refill-zones --all-track-errors --severity-all --format json -o drc.json OUT.kicad_pcb

  and read `unconnected_items` and the errors. To save a filled board, refill through kipy
  (`board.refill_zones()`, `board.save_as(...)`). `tests/judge.py` is the full judgement against the
  bare board's own DRC.
- Rules are the board's: set track and clearance in the project's net classes and minimums, not by flags.
- Some nets only: `--nets 'SPI_*,CLK'` routes those, `--except 'CLK*'` routes all but those (`*` is any
  run of letters; a scene's `scope.nets` does the same). A net not routed gets no copper, is not counted as
  open, and the others keep clear of its pads, its copper and the way out of its pins.
- Copper the board already has is joined as KiCad counts it: whatever copper of a net overlaps is one,
  pads lying on pads included. Lay the nets you want by hand first; astar builds on them.
- A block laid out alone whose ground is its host's pour: `--drops GND`.

## What it does not do yet (an outside review, 2026-11-19)

Read its notes on stderr: it says what it ignored, but the exit code does not change for these.

- **Ground is not poured.** With no ground zone drawn, ground is routed as thin track. Draw the ground
  pour or plane first. A rail may be given a free inner layer by itself (`--pour 0` turns that off).
- **No power sense.** A net is as wide as its class and nothing more; no vias in exposed pads; a class's
  via size is ignored; nothing warns when a rail is at signal width.
- **Differential pairs are two plain nets.** No length matching, no impedance.
- **Custom rules (`.kicad_dru`) are mostly not read**: anything with a condition, and width, via and hole
  rules. Clearance rules without a condition are read.
- **Nothing is lifted.** It keeps the copper a board has and never moves it, so old copper stays where
  a part has since moved.
- **A pour-only keep-out is not seen by a plane astar pours itself.**
- **Small faults KiCad calls errors**: a track can graze a keep-out's corner by under a micron; a thermal
  relief can be left with one spoke.
- "No way was found among the other nets" names no blocker. There is no default time limit
  (`--seconds S` sets one).

## Working on it

- `./gate.sh --light` (seconds, no KiCad) and `./gate.sh` (the judge too: needs `kicad-cli` from the house
  KiCad on PATH) before a change is kept. `tests/same.sh --record` only when copper is meant to change.
- The same scene gives the same copper every run; a change that breaks that is a fault.
- A change is measured on boards and judged by KiCad before it is kept. What was tried and did not earn
  its place does not go into the repo, its docs or its comments.
- Boards: the repo carries dock, g474 and the house, fixture and QA boards (`corpus/boards`). ccb, tb150,
  ub400, KiCad's demos and KRT's fixtures are not in it and must not be added (corpus/README.md);
  `corpus/prepare.py` makes them locally where their sources are, and the gate uses them when it finds them.
- History before 2026-11-21 is on `shieldyguy/krt-better`, branch `claude/astar-router`, folder
  `astar-router/`.
