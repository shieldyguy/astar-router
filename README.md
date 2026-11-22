# astar: a fast grid A* board router

A library and one binary over it, Rust, no dependencies. A scene goes in, a plan comes out.

    ./install.sh                  (cargo build --release, then ~/.local/bin/astar; CLAUDE.md has the rest)
    target/release/astar route IN.kicad_pcb -o OUT.kicad_pcb     a board in, the routed board out
    target/release/astar route SCENE.json PLAN.json [-v] [--strict]
    target/release/astar scene IN.kicad_pcb [-o SCENE.json]      the board as a scene
    target/release/astar lay BOARD.kicad_pcb PLAN.json -o OUT.kicad_pcb      a plan laid on its board
    target/release/astar --passes          the passes of a run, in order; --skip NAME leaves one out

It exits 0 when every net is joined, 1 when nets are open, 2 when the input could not be taken (and says
why), 3 when a width was narrowed and `--strict` was given. Every run checks the plan it wrote with
`astar check` before it reports: copper that breaks a rule, or a net called joined that is not, is
printed and the run exits 4 (a fault in astar, never in the board). It exits 5 when every net is joined
but KiCad will find fault that astar could not help: a custom rule it did not read that may ask more than
it kept, or a short the board came with (a pad lying on another net's copper); the lines say which. `astar SCENE.json PLAN.json` is the
old way to say the same and exits 0 whenever it wrote a plan the check finds no fault with. `docs/scene.md` is the scene, `docs/hooks.md` is how a
track adds a pass, a job kind, a cost term, a copper kind or a report entry.

astar reads a KiCad board itself (`src/io/`: KiCad 7 to the house 10.99, nets by number or by name) and
writes the plan back onto it: new copper in the file's own dialect, everything else byte for byte, the
`.kicad_pro`, `.kicad_dru` and `.kicad_prl` copied beside it unchanged. `docs/parity.md` says where its scene
differs from the house KiCad's export and why (`python3 parity/parity.py`); `python3 parity/formats.py` reads,
routes, writes and reopens a board of each file format.

`SCENE.json` and `PLAN.json` are router-bench's contract (its `CONTRACT.md`): pads, holes, outline, rules and
nets in; tracks, vias and poured planes per net out. `bench/astar.py` is the bench adapter (copy it to
`router-bench/routers/`).

## Where it stands

router-bench's five boards, judged by KiCad's DRC (`./gate.sh`, 2026-11-22). astar ran on a 2-core cloud
machine; the other two columns are the bench scoreboard's, recorded on the Mac. This repo carries dock and
g474 of the five; the other three are routed where a router-bench checkout is beside it. What astar does not
do yet is in CLAUDE.md; `corpus/SCOREBOARD.md` is the wider corpus at the same commit: 58 of 80 boards whole
with no new DRC error (the house's and the fork's own: 38 of 41; the rest are KRT's fixtures, KiCad's demos
and open boards, most of them BGA boards astar does not fan out).

| board | nets | astar | landed | new DRC | track mm | vias | krt_better | topo |
|-------|-----:|------:|-------:|--------:|---------:|-----:|-----------:|-----:|
| dock  |   28 | 2.0 s |  28/28 | 0 |  446 |  55 |  69.5 s, 443 mm |  0.1 s, 509 mm |
| g474  |   41 | 0.1 s |  41/41 | 0 |  739 | 113 |  19.4 s, 738 mm |  0.5 s, 927 mm |
| ccb   |   47 | 0.5 s |  47/47 | 0 | 1097 | 119 |  33.5 s, 1074 mm |  0.6 s, 1442 mm |
| tb150 |  118 | 1.1 s | 118/118 | 0 | 2755 | 238 |  46.5 s, 2542 mm |  1.6 s, 3524 mm |
| ub400 |  243 | 11.2 s | 243/243 | 0 | 9866 | 803 | 267.4 s, 9954 mm | 13.6 s, 12298 mm |

astar's seconds are its own (`astar route`), the judging apart. The same scene gives the same copper
every run.

## How it works

1. **Clearance field.** The board becomes a grid (0.1 mm by default). Each node of each layer holds its
   distance to the nearest copper and to the nearest copper of another net, so "may this net put a track of
   this width here" is two compares. Fixed copper (pads, holes, edge, keep-outs) and routed copper are kept
   apart, so lifting a route rebuilds only the blocks it touched.
   The grid is laid where the pads are (`route::grid_offset`): each way it is shifted, by less than a
   pitch, to where the pads' middle lines lie nearest its lines, a narrow pin counting for much more than
   a broad pad. A chip's pins then have their own grid line to run out on, and a via beside one stands
   in line with it.
2. **Fences** (topo's escapes). A pad too narrow for the grid gets an exact stub along its own centre line,
   checked against the true shapes, at its net's own width where that stands clear and else at the
   narrowest the net may be drawn. The lane in front of a fine-pitch row costs other nets a toll, and no
   foreign via may sit in its mouth.
3. **Exits** (`src/passes/exits.rs`). A long pad is left, and reached, along its length and down its
   middle: toward the inside of its part or away from it. A track that crosses such a pad's side pays
   2 mm of track for it, so it does that only where the ends are shut or far out of its way. Square pads,
   tabs that lie across their part and the two pads of a wide capacitor have no front to prefer. Style
   does not cost a net: a board that stays open with the style rules on (this one, and the price of
   crossing a pour in 10) is routed once more without them, and that board is kept when fewer of its
   nets are open (the plan's note says so). Only where that can help: when a net is open for want of
   room among the others, and the run has time left. A board whose open pads are all shut in by the
   board itself (8, below) is routed once.
4. **Plane drops.** Every surface pad of a plane net gets a short track and a via of its own beside it.
   Ending on a via or a pad the net already has costs as much as a new via (`--share`, 1.5 mm of track),
   so a pad shares one only where that is nearer than the room for its own. An exposed pad (a big pad
   inside its part's ring of pins) takes its via inside itself, on a plane net and on any other.
5. **Rail pour** (krt-better's rail_pours). The busiest rail is poured on the free inner layer (the next
   one after the ground's, where the router planned that: 10). The board is
   routed both ways at once, with the pour and without. The poured one is kept when it is whole and took
   at most a quarter more search than the plain one (and four million nodes); past that it is given up.
   The choice depends on the two boards' search work alone, never on which was done first.
6. **Search.** Weighted A* over (layer, node): eight compass steps and a via, a price for turns, a bucket
   queue. One search per pad, from the pad to the net's tree.
   Where the board's via does not fit, the smallest via the board's own rules allow is laid instead
   (never under 0.3 mm on a 0.15 mm hole), at a price; the plan gives each such via its size.
   A net keeps its width. Where the full width does not fit, the net may be drawn narrower, down to
   the fab's narrowest track, but a neck costs 4 mm of track to begin and four times the step along it,
   so the net goes round whatever is in its way unless the way round is longer than that. A neck is as
   wide as there is room for: while the nets are finding their ways it is drawn at its narrowest (what
   it does not take, another net may need), and once they have (9), each narrowed stretch is widened
   to the most that is clear of what stands beside it, measured exactly, to a hundredth of a
   millimetre. So a 1.5 mm rail that must pass a 0.8 mm gap is 0.5 mm there, not the fab's 0.127.
   Every neck is in the plan's notes.
7. **Negotiation.** The first pass may cross other nets at a price. Each round re-routes the nets that
   clash, with the price of a crossing and the wear of each contested node going up, until nothing clashes
   or the same nets have come up to route again for the seventh time (the same set every round, or two
   sets turn about: nets going round in a circle). A last strict pass takes off anything that still
   clashes.
   **One tolerance** (`geom::SLACK`). KiCad's DRC lets a clearance be short by 0.5 um. astar lets its
   own be short by 0.4 um of that, the same in the grid's thresholds and in every exact check of a
   piece of track against what stands beside it, pads and routed copper alike (measured exactly, piece
   against shape). So a track as wide as its pad leaves a row of pads whose gap is the board's
   clearance exactly, which a fine-pitch part on a board at its fab's limits needs.
   **A step the grid is not sure of is measured** (`Router::t_low`, `route::exact::Tight`). The grid
   says how far the nearest copper is from a node, to a hundredth, and keeps an allowance for a step
   between two nodes that sags toward a corner. Beside a straight edge there is no sag: two tracks side
   by side at exactly the clearance are legal, and the grid alone would refuse them (it sent every other
   net of a 0.4 mm row round the back). So where a node is within that allowance of the limit, the
   search measures the step itself, piece against shape, against fixed and routed copper alike; clash
   noting and pull-tight ask the same question. Seven nets at 0.2 track and 0.2 clearance run side by
   side on a 0.4 pitch (`tests/scenes/lanes.json`).
   **True ends** (`src/route/ends.rs`). A path is drawn as it is laid, and its ends are drawn where the
   copper they join is, not at the grid node beside it: the end leg runs on the line through the pad's
   centre (out of a pin: on the pin's middle line), and the corner at its other end slides along the
   next leg to meet it, so both legs keep their headings. A leg that runs straight into a via or a track
   of its net ends in that copper, on its own line. Each piece moved off the grid is checked against the
   exact shapes; where it is not clear the end is laid as the grid found it, and the plan's notes count
   those.
   **Copper already there** (`src/route/old.rs`). The tracks and vias a board comes with stay where
   they are and are part of their nets. What they join is joined: pads and pieces that touch (as KiCad
   counts it: their copper overlaps on a layer both are on, so a track whose round end lies on a pad,
   a track that runs over one, two tracks that cross, and two pads that lie on each other with no track
   at all) are one island, and an island comes into its net's tree whole. Pads it joins get no track;
   the gate asks KiCad the same question of the same copper (`tests/touch_judge.py`). A new track may start anywhere on it and end anywhere
   on it, drawn to the old copper's own line. A piece no pad stands on is joined to its net like a pad
   (KiCad counts it open otherwise); one that cannot be is in the plan's notes (`loose`). On a plane
   net, a pad that old copper takes to a via in the plane needs no via, and pads joined by a track
   share one. So wiring laid first (the short runs of `pcb wire --near --easy`, a fanout, a stamped
   block) is kept and built on.
8. **Mending** (`src/route/mend.rs`). A net the rounds left open is laid through whatever stands in its
   way, what it went through is lifted and routed again round it, two deep. The move is kept only when
   fewer pads are open after it; else the board is put back exactly as it was, and the next try keeps
   off the nets the last one went through: the room a net needs is sometimes held by a net nobody has
   clashed with (a rail that ran along the front of a row of pads).
   A net left open says why (`src/route/solve.rs`). When the searches for a pad find no way, a flood
   from the pad over every node a track of the net may stand on as far as what stays goes (pads, edges,
   keep-outs, fences, copper the board came with) asks the board whether it leaves one at all. Where
   it leaves none, the pad is shut in by the board itself: it is not tried again, in the rounds or
   here, and the run says `the board itself leaves no way to U1.2`, which is a matter for the placement
   or the rules, and names what stopped the flood, nearest the pad first (`src/route/wall.rs`): `U1.2
   is shut in by pad U1.3 (SCK) and a keep-out`. Else it says no way was found among the other nets,
   and whose copper is in the way: last of all, each net still open is laid once more, free to run
   through other nets' copper, the nets it runs through are noted with the place, and the board is
   put back as it was (`Router::blame`): `the nearest way runs through NRST at (24.10, 18.30)`.
9. **Redrawing** (`src/route/redraw.rs`). How a path is drawn depends on what stood beside it when it
   was laid: a stretch narrowed, an end laid as found, a whole path kept as the search found it because
   it ran through another net's copper (wiggles and all). That is often copper since lifted. So once
   the rounds are over, before the mending and again after it, every net drawn with such a concession
   is pulled tight and drawn again from its paths, against the board as it stands; the joints of its
   tree stay where they are. After the mending that is done for every net: a track that stepped aside
   for a neighbour which was then routed another way is drawn straight again. The new drawing is kept only when it is clear
   of what has been laid since; else the net keeps the drawing it had.
10. **Pours** (`src/route/fill.rs`). A net the board pours copper for leans on that copper: astar works out
   where KiCad will fill the pour round the copper that stands, which pads the fill joins (solid, or
   through the spokes of a thermal relief as KiCad builds them), and lays track only for what is left; a
   track may end anywhere in the fill. Where other nets later cut the fill, the net is routed again. A
   relief left with too few spokes gets a short track of its own from the pad into the fill, which is
   what KiCad's DRC asks for.
   A pour is copper to go down into, as a plane is: the net's tree starts in the pour's largest piece,
   so each pad reaches the pour the shortest way it can (a stub and a via when the pour is on the other
   side) and no pad is first wired to another across it.
   **Crossing** (`src/passes/cross.rs`). A pour is copper someone meant to be there: a ground under the
   signals, a wide path for a current. A step across another net's pour costs twice a step beside it
   (`--cross`), so a track cuts a pour where the way round is dearer and no further than it must. Where
   every layer is poured nothing is charged: there no layer is the better one to cross on.
   **The board is the declaration, and what it leaves unsaid the router plans** (`src/passes/ground.rs`).
   What a net's copper should be is read off the board first: a zone drawn for a net is its pour, a layer
   given to a net is its plane. A ground the board draws nothing for gets what its designer would have
   drawn: the first free inner layer as its plane, and on a board with no inner layer a pour over the
   whole board on the side with fewer parts, relieved at its pads by 10 mil spokes over a 10 mil gap. The
   board is then routed as if that had been drawn: every ground pad a via down, signals crossing the
   pour no further than they must. The plan says so (`planned`), and the plane or pour is written to
   the board as a zone. A plane meets its net's through-hole pins by the same reliefs (a plane the
   board draws, by its own zone's): where other nets' pins or vias leave such a pin fewer than two
   spokes, it gets short tracks of its own on the plane's layer, as a pad in a pour does (9). `--plane 'GND=In1.Cu,+3V3=In2.Cu'` gives any net its layers outright (an inner layer is a
   plane, an outer one gets a pour; `--plane 'GND=In1.Cu+B.Cu'` is both). `--ground tracks` wires the
   ground as any net is, and the notes say so (`ground`).
   On the 29 house and fixture boards whose ground has nothing drawn, the planned pour cut the track by
   a fifth (5,859 mm to 4,589 mm) with the same nets joined, and KiCad's verdict after a refill was the
   same on every one: no error and no open net that the wired board did not have.
   **Pairs** (`src/route/duo.rs`). A differential pair is two nets that run side by side between the two
   parts both have a pad on (the connector or its ESD array, the chip). Between them the pair is one
   thing in the negotiation: a centre line, searched as a net of its own that keeps half the pair's
   breadth more from everything than a plain net does, and the two tracks drawn off it, one to each
   side, at the pair's width and gap (the pair's own, else its class's; the gap never under the
   clearance). The line pays for what it runs through like any path, so the pair and its neighbours
   settle who gives way by price; lifting either net lifts both. Each net's pads are then joined to
   the nearer end of its track like any net's. A pair stays on its pads' layer. One that cannot be
   laid so (its pads lie the other way round at one end, the only way is the long way round, its
   pads are on both sides) is routed as two plain nets, and the plan says which and why (`pair`,
   `astar: pair: ...` on stderr); a laid pair's note gives its length side by side and how much
   longer one half is. The two halves are not made the same length. `--skip pairs` routes every
   pair as two nets. Of the 13 pairs on the house, fixture and bench boards 7 are laid side by
   side, with the same nets joined and the same verdict from KiCad as before.
   **Some nets only** (`--nets`, `--except`, the scene's `scope`). The nets asked for are routed; the
   others get no copper and are not counted as open. They are still on the board: their pads and their
   copper are fixed copper to the nets that are routed, and their fine-pitch pins keep their fences and
   lanes, so the way out of them is still there for whoever routes them next. The plan names them
   (`not_asked`).
   **The layers a net is routed on** (`--route-on 'CLK*=B.Cu'`, a net's `route_on` in the scene). Its
   tracks run on the layers named and nowhere else, in every search and in the redrawing. Its vias are
   not part of it: a through via goes through every layer, as always. A pad of the net on another
   layer is left by a stub to a via: off its layers the net may stand only within `--stub` (2.5 mm)
   of the copper of a pad of its own on that layer, and a step there costs three, so the stub is as
   short as there is room for. A rule that cannot be kept leaves the net open, and the run says the
   rule is what shuts the pad in. The plan records the rule (`route_on`) and its notes say how much of
   the net runs where; astar's own check holds the plan's copper to it (`layer`).
   Where 2.5 mm comes from: every signal net of the 38 boards in `corpus/boards`, 426 of them, was
   kept to the bottom layer by itself. All 426 were joined at 2.5 mm, and at 4 and 6 mm too; with room
   to spare the longest stub ran to 5.9 mm, and the shorter limit gave less track off the layer in all.
   **Drops** (`--drops NET`). A block laid out alone has no pour of its own: its ground is the pour of
   the board it will be set into. Such a net's pads go down by vias (a pad may share one, and old copper that reaches a via counts) and nothing else, the vias
   are joined by nothing here (the notes say so, `drops`), no zone is drawn for it, and the layer the
   vias go down to stays open to tracks, at the crossing price.
11. **Pull tight.** Each path is redrawn as the fewest straight runs and 45° elbows the field allows, never
   with more of it too tight for the full width than it had, never through more of another pin's lane
   than it had (the search paid to keep out of it), and never across the side of a pad that the search
   left by an end. At a pad the track runs straight on and the bend goes to the far end of the
   stretch: out of a pin, straight for as far as it can go. This pass decides the look.
12. **Round** (krt-better's round_corners). Every corner becomes an arc of one radius (`--radius`, 1 mm; 0
   keeps them sharp), checked with exact geometry against all other copper. The biggest radius is tried on
   every corner first, so tracks that turn together keep their spacing through the bend. About nine
   corners in ten round; the rest are at pads, vias and branches, or have no room.

## What it does not do yet

The plan to close these, and the rest of the distance to KRT, is in `plan/` (start at `plan/README.md`).

- The ground and one rail get a plane or a pour; the other rails are track. A board with inner planes
  gets no ground pour on its outer layers unless `--plane` asks for one.
- Copper already on the board is never lifted: a net whose old copper is in another's way is not
  routed again. The fill a zone was last given is not read; astar works the fill out itself (10).
- A differential pair runs side by side on one layer only, and its halves are not length-matched; a
  match group and an impedance target are carried and not used.
- A clearance asked on one layer only, or between two classes only, is kept everywhere (the largest of
  them): legal, and more room than the rule asks. A track width for one layer is not used, and every net
  gets the board's via, whatever its class asks (a class that asks another is named in the notes).
- A net is as wide as its class says. A net that is power by its name and no wider than a signal is
  named (`astar: thin: ...`); nothing widens it but a net class.
- The tuning is for the 0.1 mm grid, the only one judged by DRC. By its own count: at 0.05 mm dock lands
  27 of 28 and ub400 takes 14 s; at 0.2 mm ub400 takes 1.3 s but dock and ccb leave fine-pitch pins open.
- The same five boards on two layers with no planes (ground as track), by its own count: four land
  whole in under a second, ub400 lands 242 of 243 in 21 s.

## Files

    src/main.rs      the command line                    src/route/     the router: the run (drive), jobs,
    src/lib.rs       the library: astar::route(scene,                   negotiation, laying and lifting
                     &cfg), Scene, Cfg, Routed, Error                   copper, old copper, the exact index
    src/scene/       the scene: types, the readers of    src/passes/    the pass hook and one file a planning
                     1.1 and 2, the validator                           pass: pour, fences, exits, cross, drops
    src/geom.rs      shapes and distances                src/finish/    pull tight; corners into arcs
    src/field/       the clearance field, copper kinds   src/report/    notes, counts, open nets, the plan
    src/search/      the A*, its queue, the cost hook    examples/      a scene routed through the library
    src/json.rs      a small JSON reader and writer      docs/          scene.md (scene 2), hooks.md (the five
    bench/           the bench adapter; a board writer                  hooks, errors and exit codes)
                     for machines without braid-field    tests/         same.sh and golden/ (the bench five's
                     (BENCH_FIELD_PY=bench/shim)                        copper), scene.rs, lib.rs, cli.rs,
                                                                        fuzz.rs, scenes/ (the fixtures)
