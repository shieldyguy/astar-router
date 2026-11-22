#!/usr/bin/env python3
"""scene_board.py SCENE.json OUT.kicad_pcb: a bare KiCad board with a scene's pads, old tracks and vias and
outline, so that KiCad can be asked what it counts as joined. Rectangles, circles and ovals only."""
import json, sys, math
s = json.load(open(sys.argv[1]))
nets = [""] + [n["name"] for n in s["nets"]]
nid = {n: i for i, n in enumerate(nets)}
o = ['(kicad_pcb (version 20240108) (generator "scene_board") (general (thickness 1.6)) (paper "A4")',
     ' (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))', ' (setup (pad_to_mask_clearance 0))']
o += [f' (net {i} "{n}")' for i, n in enumerate(nets)]
for k, p in enumerate(s["pads"]):
    x, y = p["at"]; net = p.get("net", "")
    sh = p["shape"]
    if "poly" in sh:
        xs = [q[0] for q in sh["poly"]]; ys = [q[1] for q in sh["poly"]]
        w, h, kind = max(xs) - min(xs), max(ys) - min(ys), "rect"
    elif "circle" in sh:
        w = h = 2 * sh["circle"][2]; kind = "circle"
    else:
        raise SystemExit("shape not handled")
    side = "B.Cu" if p.get("side") == "B" else "F.Cu"
    if p.get("tht"):
        pad = f'(pad "{p["num"]}" thru_hole {kind} (at 0 0) (size {w:g} {h:g}) (drill {p.get("drill", 0.3):g}) (layers "*.Cu") (net {nid[net]} "{net}"))'
    else:
        pad = f'(pad "{p["num"]}" smd {kind} (at 0 0) (size {w:g} {h:g}) (layers "{side}") (net {nid[net]} "{net}"))'
    o.append(f' (footprint "scene:{p["ref"]}_{k}" (layer "{side}") (at {x:g} {y:g}) (property "Reference" "{p["ref"]}{k}" (at 0 0) (layer "F.Fab")) {pad})')
for t in s.get("copper", {}).get("tracks", []):
    for piece in t["path"]:
        v = piece["line"]
        o.append(f' (segment (start {v[0]:g} {v[1]:g}) (end {v[2]:g} {v[3]:g}) (width {t["width"]:g}) (layer "{t["layer"]}") (net {nid[t["net"]]}))')
for v in s.get("copper", {}).get("vias", []):
    o.append(f' (via (at {v["at"][0]:g} {v["at"][1]:g}) (size {v["size"]:g}) (drill {v["drill"]:g}) (layers "F.Cu" "B.Cu") (net {nid[v["net"]]}))')
for e in s["outline"]:
    v = e["line"]
    o.append(f' (gr_line (start {v[0]:g} {v[1]:g}) (end {v[2]:g} {v[3]:g}) (stroke (width 0.05) (type solid)) (layer "Edge.Cuts"))')
o.append(')')
open(sys.argv[2], "w").write("\n".join(o) + "\n")
