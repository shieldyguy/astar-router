"""astar_board: the house grid A* router, board to board: it reads the KiCad board itself and writes the routed one.

Kind: board -> board.  One call: `astar route BOARD.kicad_pcb -o OUT_DIR/routed.kicad_pcb --plan OUT_DIR/plan.json
--seconds BUDGET`.  The same router as `astar` (routers/astar.py); what differs is the way in and out: astar's own
reader makes the scene (src/io, docs/parity.md says where it differs from the exported scene) and astar's own
writer lays the copper.  The bench judges the board that comes back with the bare board's project.
BENCH_ASTAR names the binary; BENCH_ASTAR_ARGS adds arguments.
"""
import json
import os

KIND = "board->board"
HERE = os.path.dirname(os.path.abspath(__file__))
SPOTS = [os.path.join(HERE, "..", "..", d, "target", "release", "astar") for d in ("astar-router", os.path.join("krt-better", "astar-router"))]
BOARD_WRITTEN = (0, 1, 3)
ASTAR = os.environ.get("BENCH_ASTAR") or next((p for p in SPOTS if os.path.exists(p)), SPOTS[0])


def route(job):
    out = os.path.join(job["out_dir"], "routed.kicad_pcb")
    plan = os.path.join(job["out_dir"], "plan.json")
    extra = os.environ.get("BENCH_ASTAR_ARGS", "").split()
    r = job["run"]([ASTAR, "route", job["board"], "-o", out, "--plan", plan, "--seconds", str(int(job["budget_s"] * 0.9))] + extra)
    if r.timed_out:
        return {"failed": f"stopped at the budget ({job['budget_s']} s)"}
    # astar route: 0 every net joined, 1 nets open, 3 a rule narrowed (--strict): a board was written each time
    if r.code not in BOARD_WRITTEN or not os.path.exists(out):
        return {"failed": f"exit {r.code}: {(r.err or r.out)[-300:]}"}
    p = json.load(open(plan))
    s = p.get("summary", {})
    notes = "; ".join(x for x in (s.get("note"), f"pitch {s.get('pitch')} mm", f"{s.get('rounds')} rounds", "read and written by astar") if x)
    return {"board": out, "seconds": p.get("timing_s", {}).get("total", r.seconds), "notes": notes, "extra": {"timing_s": p.get("timing_s"), "summary": s}}
