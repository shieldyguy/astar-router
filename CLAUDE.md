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
  be read, 3 a width narrowed under `--strict`, 4 astar's own check found fault with its plan, 5 every net
  joined but KiCad will find fault astar could not help: a custom rule it did not read that may ask more
  than it kept (`astar: NOT HELD TO: ...`), a short the board came with (`astar: SHORT: ...`).
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
- The layers a net is routed on: `--route-on 'CLK*=B.Cu'` (several: `'CLK*=B.Cu,SPI_*=F.Cu+In2.Cu'`). Its
  tracks run only there; its vias go through every layer as always. A pad of it on another layer is left
  by a stub to a via, at most 2.5 mm from the pad (`--stub MM`). A rule that cannot be kept leaves the net
  open and says so; it can also cost a neighbour its way on a tight board, which the run reports like any
  open net. The plan's notes say how much runs where, and astar's check holds the plan to the rule.
  KiCad's DRC knows nothing of this rule.
- A net left open says what is in its way: `astar: open: NET: ...` names the pad or keep-out that shuts
  a pad in, or the nets whose copper the nearest way runs through, with the place.
- Ground and planes. A pour or plane drawn on the board is used as drawn. A ground with nothing drawn
  gets copper from astar (`astar: planned: ...` says what): the first free inner layer as its plane and
  a pour on each outer layer over it (the house way), or on a two-layer board a pour on the side with
  fewer parts, written to OUT as zones. The busiest rail
  gets the next free inner layer when the board still routes whole (`--pour 0`: never). Say it yourself
  with `--plane 'GND=In1.Cu,+3V3=In2.Cu'` (inner layer: plane; outer layer: pour; `GND=In1.Cu+B.Cu` both).
  A plane or pour astar draws meets pads by reliefs (10 mil spokes over a 10 mil gap); a through-hole
  pin of a plane left with fewer than two spokes gets a short track on the plane's layer.
  `--ground tracks` wires the ground as a net instead. Every pad of a plane net gets a via of its own
  beside it (`--share MM`: what sharing another pad's via costs; 0 shares freely).
- A block laid out alone whose ground is its host's pour: `--drops GND` (vias only, no pour drawn).

## What it does not do yet (an outside review, 2026-11-19)

Read its notes on stderr: it says what it ignored, but the exit code does not change for these.

- **Width is the board's to say.** A net is as wide as its class and nothing more: give rails a net class
  with a track width. A net that is power by name and at signal width is named (`astar: thin: ...`), not
  widened. A net lays its class's via (size and drill), the board's where its class says none. An
  exposed pad gets one via in itself, with no paste window over it.
- **A differential pair runs side by side on its pads' layer** between the two parts both nets have a
  pad on, at the pair's width and gap. One that cannot is routed as two plain nets and named
  (`astar: pair: ...`); `--skip pairs` does that for all. No length matching, no impedance.
- **Custom rules (`.kicad_dru`) with a condition are not read.** Without one, clearance and the least
  track, via, hole and ring are. A rule not read that asks no more than is kept anyway is a note; one
  that may ask more makes the run exit 5.
- **Nothing is lifted.** It keeps the copper a board has and never moves it, so old copper stays where
  a part has since moved.
- **A small fault KiCad calls an error**: a thermal relief can be left with one spoke.
- There is no default time limit (`--seconds S` sets one).

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
