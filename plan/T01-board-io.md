# T1 Board I/O

**Wave 1.** Mission: a `.kicad_pcb` in, a routed `.kicad_pcb` out, with no other tool in between.

Owns: `src/io/`. Needs: scene 2 (T0.2). Gives: scenes from real boards to every track, and the writer.
Today the scene comes from `kicad-cli pcb route-field --scene` and the board is written by braid-field's
`emit_plan` (a stand-in is in `bench/shim/`). Both stay as cross-checks.
Reference for what a board can hold: KRT's `py_router/kicad_parser.py` (8,300 lines) and
`docs/api-kicad-parser.md`; the house format is KiCad 10.99.

### T1.1 S-expressions, lossless
Build: a reader and writer that give back an untouched file byte for byte, and let one node be replaced.
Done when: every corpus board round-trips identical.

### T1.2 Layers, outline, holes (parallel)
Build: copper layers and their order, the stackup, Edge.Cuts as rings (lines, arcs, circles, polygons,
cut-outs, outline drawn inside footprints), holes: round, slotted, plated or not.
Done when: outline and holes match the house KiCad's scene export within 1 um on the corpus.

### T1.3 Pads (parallel)
Build: every pad shape (circle, oval, rect, round rect, chamfered, trapezoid, custom) at any angle, drill
offset from the copper, the layers each pad is on, castellated pads, pad and footprint clearance
overrides, net-tie groups, copper drawn in a footprint.
Done when: pad polygons match the house export within 1 um; KRT's rotated-QFN and offset-drill boards
(`kicad_files/haasoscope*`, `qfn_*`) included.

### T1.4 Nets, classes, rules (parallel)
Build: nets in both file dialects; classes and their patterns from `.kicad_pro`; class width, clearance,
via, pair width and gap; the board's minimums; from `.kicad_dru` the layer-scoped clearance and width rules.
A rule astar cannot read is named in the report, not skipped silently.
Done when: for every corpus board the resolved clearance of 1,000 sampled pad pairs equals KRT's.

### T1.5 Copper already there (parallel)
Build: tracks, arcs, vias of any span, zones with outline, priority, fill and thermal settings, rule areas
with what they forbid, locked flags, KiCad groups.
Done when: copper length and via count per net equal KRT's parser on the routed boards of the corpus.

### T1.6 The writer
Build: a plan onto a board: tracks, arcs, vias, zones added; ripped copper removed by id; everything else
left byte for byte. New ids from a seed, so the same run writes the same file. Sibling files copied
(`.kicad_pro`, `.kicad_dru`, `.kicad_prl`). A new via carries no protection token (it inherits the board's);
a via that was there keeps its own.
Done when: the house KiCad opens every written board with no load warning; DRC of the bench five equals
the rows made through `emit_plan`.

### T1.7 Formats
Build: fixtures for KiCad 9, 10 and 10.99 files; boards with duplicate references, footprints without a
reference, nets with odd names.
Done when: each reads, routes, writes and reopens.

### T1.8 Parity suite
Build: a command that compares astar's scene of a board with the house export and with KRT's parser, value
by value, and lists differences by board.
Done when: zero unexplained differences on the corpus; each explained one is a line in `docs/parity.md`.

### T1.9 Board to board
Build: `astar route IN.kicad_pcb -o OUT.kicad_pcb` and a router-bench adapter of kind board to board.
Done when: the bench five score the same through it as through the scene path.

### T1.10 The project file
Build: only what Chris rules (see the doc's decisions): either leave `.kicad_pro` alone, or record the sizes
a run really used so KiCad grades at them, as KRT does.
Done when: the ruling is implemented and a fixture shows it.
