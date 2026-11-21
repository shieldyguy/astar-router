"""A stand-in for braid-field's emit_plan, enough for router-bench to lay a plan on a bare board.

router-bench imports `emit_plan` from BENCH_FIELD_PY (braid-field/field/py on the bench's home machine). Where
that checkout is not to hand, point BENCH_FIELD_PY at this folder: it writes a plan's tracks, arcs, vias and
planes onto a copy of the bare board in the fork's file format (nets by name). It does not insert inner layers
into a two-layer board and does not move parts.
"""
import math
import os
import re
import shutil
import uuid


def _q(s):
    return '"' + str(s).replace("\\", "\\\\").replace('"', '\\"') + '"'


def _n(v):
    s = f"{v:.6f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def pad_tester(scene):
    return None


def move_scene_pads(scene, moves):
    return scene


def _outline_box(text):
    xs, ys = [], []
    for m in re.finditer(r"\t\((?:gr_line|gr_rect|gr_arc)\b.*?\n\t\)", text, re.S):
        b = m.group(0)
        if '"Edge.Cuts"' not in b:
            continue
        for x, y in re.findall(r"\((?:start|end|mid) ([-\d.e]+) ([-\d.e]+)\)", b):
            xs.append(float(x)); ys.append(float(y))
    if not xs:
        return None
    return min(xs), min(ys), max(xs), max(ys)


def write_board(bare, dst, plan, pads_polys=None, via=(0.45, 0.2)):
    text = open(bare).read()
    layers = re.findall(r'\(\d+ "([^"]+\.Cu)" ', text)
    first, last = "F.Cu", "B.Cu"
    out = []
    for net in plan.get("nets", []):
        name = net["name"]
        for t in net.get("tracks", []):
            for seg in t.get("path", []):
                if "line" in seg:
                    x0, y0, x1, y1 = seg["line"]
                    if math.hypot(x1 - x0, y1 - y0) < 1e-6:
                        continue
                    out.append(f'\t(segment\n\t\t(start {_n(x0)} {_n(y0)})\n\t\t(end {_n(x1)} {_n(y1)})\n\t\t(width {_n(t["width"])})\n'
                               f'\t\t(layer {_q(t["layer"])})\n\t\t(net {_q(name)})\n\t\t(uuid "{uuid.uuid4()}")\n\t)')
                else:
                    sx, sy, mx, my, ex, ey = seg["arc"]
                    out.append(f'\t(arc\n\t\t(start {_n(sx)} {_n(sy)})\n\t\t(mid {_n(mx)} {_n(my)})\n\t\t(end {_n(ex)} {_n(ey)})\n\t\t(width {_n(t["width"])})\n'
                               f'\t\t(layer {_q(t["layer"])})\n\t\t(net {_q(name)})\n\t\t(uuid "{uuid.uuid4()}")\n\t)')
        for v in net.get("vias", []):
            x, y = v["at"]
            out.append(f'\t(via\n\t\t(at {x:.6f} {y:.6f})\n\t\t(size {v.get("size", via[0]):g})\n\t\t(drill {v.get("drill", via[1]):g})\n'
                       f'\t\t(layers {_q(first)} {_q(last)})\n\t\t(net {_q(name)})\n\t\t(uuid "{uuid.uuid4()}")\n\t)')
    box = _outline_box(text)
    for q in plan.get("planes", []):
        if q["net"].startswith("__BENCH_NO_PLANE__"):
            continue
        loops = q.get("outline")
        if not loops:
            if not box:
                continue
            x0, y0, x1, y1 = box
            loops = [[[x0 - 1, y0 - 1], [x1 + 1, y0 - 1], [x1 + 1, y1 + 1], [x0 - 1, y1 + 1]]]
            prio = 0
        else:
            prio = int(q.get("priority", 1))
        for loop in loops:
            pts = " ".join(f"(xy {_n(x)} {_n(y)})" for x, y in loop)
            pr = f"\n\t\t(priority {prio})" if prio else ""
            out.append(f'\t(zone\n\t\t(net {_q(q["net"])})\n\t\t(layer {_q(q["layer"])})\n\t\t(uuid "{uuid.uuid4()}")\n\t\t(hatch edge 0.5){pr}\n'
                       f'\t\t(connect_pads yes\n\t\t\t(clearance 0.2)\n\t\t)\n\t\t(min_thickness {_n(q.get("min_width", 0.2))})\n'
                       f'\t\t(fill yes\n\t\t\t(thermal_gap 0.3)\n\t\t\t(thermal_bridge_width 0.3)\n\t\t\t(island_removal_mode 0)\n\t\t)\n'
                       f'\t\t(polygon\n\t\t\t(pts\n\t\t\t\t{pts}\n\t\t\t)\n\t\t)\n\t)')
    k = text.rstrip().rfind(")")
    open(dst, "w").write(text[:k].rstrip() + "\n" + "\n".join(out) + "\n)\n")
    # the board's own rules go with it: the DRC reads the project beside the board
    for ext in (".kicad_pro", ".kicad_dru"):
        src = bare[:-len(".kicad_pcb")] + ext
        if os.path.exists(src):
            shutil.copy(src, dst[:-len(".kicad_pcb")] + ext)
