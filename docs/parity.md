# Parity: astar's reader beside the house KiCad's export

astar reads a `.kicad_pcb` itself (`src/io/`, `astar scene IN.kicad_pcb`). Before it did, a scene came from
the house KiCad (`kicad-cli pcb route-field --scene`, made neutral by `corpus/prepare.py`). That export stays
as the cross-check: `parity/parity.py` compares the two scenes of every corpus board value by value, lengths
within 1 um, and puts every difference in a class. A class on this page is explained; anything else is not,
and the command exits 1.

    python3 parity/parity.py            every corpus board
    python3 parity/parity.py NAME -v    one board, every unexplained difference listed

The export is format 1.1 and astar's scene is format 2, so the comparison is of what both can say: the copper
layers, the planes, the six rules, the outline, the holes, the pads (place, net, side, shape, clearance), the
nets (role, width, clearance), the keep-outs, the parts. What only format 2 carries (classes, floors, the
stackup, copper already there, pairs, groups) has nothing to be compared with.

## The differences, by class

Each line: the class as the command prints it, and why the two scenes differ there. "The export" is the house
KiCad's; where it is wrong about the board, the line says so.

| class | why |
|---|---|
| `export: none` | The export gave no scene: 15 house blocks have no Edge.Cuts outline, and the exporter dies on krt_orangecrab_ext_pll. astar reads all 16. |
| `outline: made up` | A board with no outline (a block on its own) gets the box round its pads and courtyards, grown by the edge rule and 1 mm. The scene's `source.notes` says so. |
| `plane: a small zone` | The export calls every zone its net's plane. astar calls a zone a plane only when its outline covers half the board's box or more (the rule `corpus/manifest.json` uses); a small pour does not reach every pad of its net, so its net is not "joined by a via into the plane". Every zone is in `copper.zones` either way. |
| `plane: one a net` | A net with pours over the board on several layers (GND on F.Cu, In2.Cu and B.Cu) has one plane in astar's scene: an inner layer before an outer one (an outer pour shares its layer with the parts and their tracks), then the larger pour, then the lower layer. The export lists every such layer and a reader keeps the last, whichever that is. |
| `net: not listed by the export` | The export lists only the nets it would route: two or more of the pads it took, and not named `unconnected-...`. Format 2 lists every net a pad or a piece of copper names. The nets to join are still the ones with two or more pads. This is why KiCad wanted nets joined that the exported scene did not ask for (interf_u `/PC-IOR`: its pads on the back were not pads in the export). |
| `net: role` | Ground: the export takes the net of the largest pour on the field's ground layer, else the net named GND. astar takes the plane whose name says ground, else the net whose name does (GND, /x/GND, AGND, VSS), else the largest plane. They differ where a board has no pour on the export's ground layer and its ground is not spelled GND, and on a two-layer board exported through a four-layer copy. |
| `net: class width` | A net in a class whose track is not the Default's gets that width. The export passes it only for a rail (wider than Default, and not ground): a ground net in a wide class and a net in a narrow class (a 0.127 mm pair) had the Default width in the export. |
| `pad: not in the export` | Pads the export had no use for. None is left on the corpus once the back pads below are counted. |
| `pad: on the back, a keep-out in the export` | A surface pad on B.Cu is a pad with `side: "B"`. Where the export's layer roles had no place for it (a two-layer board through the four-layer copy) it became a keep-out polygon, and its net lost a pad. |
| `pad: round corners` | The same copper, written two ways. The export writes a rounded rectangle as the full box with `r` the corner radius; format 2 says a polygon is grown by `r`, so astar writes the box drawn in by the radius. Read as format 2 says, the export's pads are too large by their corner radius; astar's are the pad. A pad whose radius is half its width is an oval or a circle. |
| `pad: shape is not a rectangle` | Trapezoid, chamfered and custom pads: the export gives the box round them in the pad's frame (its size is KiCad's polygon, 5 to 10 um over), astar the outline. A chamfered pad's other corners are left sharp (KiCad rounds them: astar is over by less than a third of that radius). A custom pad of one polygon is that polygon; one of several pieces is the hull round them, and the reader's notes count those. |
| `pad: clearance below the rule` | A pad's own clearance (or its footprint's) is passed whenever the board gives one. The export passes it only when wider than the rule. |
| `pad: clearance of a net not listed` | The export gives a pad of a net it does not list its class's clearance as the pad's own. In format 2 the net is listed and carries it. |
| `pad: ref of a second footprint` | Format 2 wants each part's `ref` once. Two footprints of one reference: the later ones are `REF~2`, `REF~3` (KRT's rule). A footprint without a reference is `#uuid`. The export repeats the name. |
| `part: no reference` | The export leaves out a footprint that has no reference; astar keeps it as `#uuid`. |
| `hole: a slot` | A non-plated slot: the export gives one round hole as wide as the slot is long; astar a row of round holes of the slot's width, their rim within a micron of the slot's. |
| `keepout: vias kept out` | A rule area that keeps vias out: the export spreads it over all three layers it routes on. astar keeps the area's own layers and says what it forbids (`forbid`). |
| `keepout: layers the export does not route on` | The export knows three layers (front, lane, hop). A keep-out on other layers is missing from it, and one on more layers is cut down to those three. |
| `keepout: a hole in it` | A rule area drawn with holes (krt_glasgow_revC: the whole board kept out but for a window): the file's first polygon is the outline and the others are holes. The export takes the outline alone, so the window is kept out too and nothing on the board can be routed. astar cuts the holes out (the scene's polygon is the outline joined to each hole by a seam). |
| `keepout: an inner pad` | A surface pad on an inner layer only is copper there and nothing a router ends on: a keep-out on that layer in both scenes. None on the corpus. |
| `copper: drawn in footprints` | Copper that is no track or zone: shapes drawn on a copper layer in a footprint or on the board (a tab, an antenna). In astar's scene as locked `copper` (a stroke a track, a filled shape a zone with its fill), on the net of the footprint's own pad it touches, else on none. The export has no place for it. |

## What is not in the scene, and is said

The reader's notes (`source.notes`, and `astar: board:` lines on stderr) name what it could not say:

- rule areas that keep out neither tracks nor vias (zones, pads or footprints only): not in the scene, since
  until the router reads `forbid` every keep-out keeps tracks and vias out;
- custom rules of the `.kicad_dru` it cannot read, by name. Read: a clearance or a width with no condition,
  on all layers or on some (`rules.layers`); a clearance between named classes (`class_pairs`); the most any
  rule asks from the edge, a hole, or between holes (`rules.edge`, `hole_clearance`, `hole_to_hole`);
- net class patterns that look like regular expressions and match nothing as wildcards;
- copper text, non-plated holes with copper round them, zones that do not connect to pads at all, scaled
  footprints, and a board with no `.kicad_pro` (KiCad's defaults are taken).

Two things a board says that format 2 has no key for are carried under `source`, which nothing reads yet:
`castellated` (the `REF.NUM` of each castellated pad) and `net_ties` (the pads a footprint ties together).
