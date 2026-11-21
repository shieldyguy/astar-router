#!/usr/bin/env python3
"""What the bench five do not have, each judged by the house KiCad: copper already on the board, a pour, a
fine-pitch part on two layers.

    old copper   g474 is routed, the short runs from pad to pad on the front are kept and the rest taken off;
                 astar routes the rest on top. Every net lands, no new DRC error, astar's own check is clean,
                 and the copper in all is no more than a tenth over the board routed from nothing.
    a pour       the Lakehouse fixture with ground poured over the whole back: every net lands (ground through
                 the pour), no new DRC error, and astar's check, which fills the pour itself, agrees.
    fine pitch   the Lakehouse fixture as it is (a 0.5 mm QFN on two layers, ground as track).

One line a check; exit 1 when one fails. ASTAR names the binary (default target/release/astar).
"""
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "tests"))
import judge as J  # noqa: E402

ASTAR = os.environ.get("ASTAR") or os.path.join(ROOT, "target", "release", "astar")
BOARDS = os.path.join(ROOT, "corpus", "boards")
ZONE = """	(zone
		(net "GND")
		(layer "B.Cu")
		(uuid "0a57a200-0000-4000-8000-00000000f111")
		(hatch edge 0.5)
		(connect_pads
			(clearance 0.2)
		)
		(min_thickness 0.2)
		(fill yes
			(thermal_gap 0.3)
			(thermal_bridge_width 0.3)
			(island_removal_mode 0)
		)
		(polygon
			(pts
				(xy %s %s) (xy %s %s) (xy %s %s) (xy %s %s)
			)
		)
	)
"""
bad = 0


def say(ok, what, detail):
    global bad
    bad += not ok
    print(f"{'ok  ' if ok else 'FAIL'} {what}: {detail}")


def run(*args):
    return subprocess.run([ASTAR, *args], capture_output=True, text=True)


def length(tracks):
    mm = 0.0
    for t in tracks:
        for q in t["path"]:
            v = q.get("line") or q.get("arc")
            mm += math.hypot(v[-2] - v[0], v[-1] - v[1])
    return mm


def board_of(name, into):
    """A copy of a corpus board with its project and rules beside it."""
    src = os.path.join(BOARDS, name, name)
    for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
        if os.path.exists(src + ext):
            shutil.copy(src + ext, os.path.join(into, "board" + ext))
    return os.path.join(into, "board.kicad_pcb")


def routed(board, tmp, tag):
    """The board's scene and plan, astar's exit code and what it said."""
    scene, plan = os.path.join(tmp, tag + ".scene.json"), os.path.join(tmp, tag + ".plan.json")
    r = run("scene", board, "-o", scene)
    if r.returncode != 0:
        return scene, plan, r.returncode, r.stderr
    r = run("route", scene, plan)
    return scene, plan, r.returncode, r.stderr


def judged(what, board, scene, plan, code, err, tmp):
    if code != 0:
        say(False, what, f"astar exited {code}: {[l for l in err.splitlines() if 'nets joined' in l or 'OWN CHECK' in l][:2]}")
        return None
    row = J.judge(scene, plan, board, cache=None, name=what)
    say(row["pass"], what, f"{row['landed']}/{row['nets']} landed, {row['drc_new_total']} new DRC errors, astar's own check clean"
        + ("" if row["pass"] else f": {row['why']}"))
    return row


def main():
    tmp = tempfile.mkdtemp(prefix="astar-gate-more-")
    try:
        # old copper
        d = os.path.join(tmp, "old")
        os.makedirs(d)
        bare = board_of("g474", d)
        scene, plan, code, err = routed(bare, d, "whole")
        if code != 0:
            say(False, "old copper", f"g474 does not route whole to begin with (exit {code})")
        else:
            p, s = json.load(open(plan)), json.load(open(scene))
            pads = {}
            for q in s["pads"]:
                pads.setdefault(q.get("net"), []).append(q["at"])
            top = s["stack"]["copper"][0]

            def direct(net, t):
                ends = []
                for q in (t["path"][0], t["path"][-1]):
                    v = q.get("line") or q.get("arc")
                    ends.append((v[0], v[1]) if q is t["path"][0] else (v[-2], v[-1]))
                return t["layer"] == top and length([t]) <= 4.0 and all(any(math.hypot(e[0] - a[0], e[1] - a[1]) < 0.02 for a in pads.get(net, [])) for e in ends)

            kept = [{"name": n["name"], "tracks": [t for t in n["tracks"] if direct(n["name"], t)], "vias": []} for n in p["nets"]]
            kept = [n for n in kept if n["tracks"]]
            half = dict(p, nets=kept)
            half.pop("left_out", None)
            json.dump(half, open(os.path.join(d, "half.plan.json"), "w"))
            d2 = os.path.join(d, "half")
            os.makedirs(d2)
            hb = os.path.join(d2, "board.kicad_pcb")
            r = run("lay", bare, os.path.join(d, "half.plan.json"), "-o", hb)
            for ext in (".kicad_pro", ".kicad_dru"):
                if os.path.exists(bare[:-len(".kicad_pcb")] + ext):
                    shutil.copy(bare[:-len(".kicad_pcb")] + ext, hb[:-len(".kicad_pcb")] + ext)
            if r.returncode != 0 or not kept:
                say(False, "old copper", f"the kept copper could not be laid ({len(kept)} nets): {r.stderr[-200:]}")
            else:
                scene2, plan2, code, err = routed(hb, d2, "rest")
                row = judged("old copper (g474, the short runs kept)", hb, scene2, plan2, code, err, d2)
                if row:
                    was, old, new = length(t for n in p["nets"] for t in n["tracks"]), length(t for n in kept for t in n["tracks"]), length(t for n in json.load(open(plan2))["nets"] for t in n["tracks"])
                    say(old > 20 and old + new <= was * 1.1, "old copper is built on", f"{old:.0f} mm kept, {new:.0f} mm added, {was:.0f} mm from nothing")
        # a pour, and the same board without one
        for what, pour in (("a pour (Lakehouse, ground poured on the back)", True), ("fine pitch (Lakehouse on two layers)", False)):
            d = os.path.join(tmp, "pour" if pour else "fine")
            os.makedirs(d)
            board = board_of("fix_lakehouse.placed", d)
            if pour:
                text = open(board).read()
                sc0 = os.path.join(d, "bare.scene.json")
                run("scene", board, "-o", sc0)
                pts = [v for o in json.load(open(sc0))["outline"] for v in (o.get("line") or o.get("arc"))]
                xs, ys = pts[0::2], pts[1::2]
                box = (min(xs) - 1, min(ys) - 1, max(xs) + 1, max(ys) + 1)
                zone = ZONE % (box[0], box[1], box[2], box[1], box[2], box[3], box[0], box[3])
                k = text.rstrip().rfind(")")
                open(board, "w").write(text[:k] + zone + ")\n")
            scene, plan, code, err = routed(board, d, "it")
            row = judged(what, board, scene, plan, code, err, d)
            if row and pour:
                gnd = [n for n in json.load(open(plan))["nets"] if n["name"] == "GND"]
                mm = length(gnd[0]["tracks"]) if gnd else 0.0
                say(mm < 120, "the pour is gone down into", f"ground is {mm:.0f} mm of track and {len(gnd[0]['vias']) if gnd else 0} vias (as track alone: over 300 mm)")
    finally:
        if bad:
            print(f"     (kept in {tmp})")
        else:
            shutil.rmtree(tmp, ignore_errors=True)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
