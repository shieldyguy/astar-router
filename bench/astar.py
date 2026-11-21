"""astar: the house grid A* router (shieldyguy/astar-router): octilinear tracks, one binary, no dependencies.

Kind: scene -> plan.  One call: `astar route SCENE PLAN --seconds BUDGET`.  It builds a clearance field from the
scene, fences the fine-pitch pads, drops every surface pad of a plane net into its plane by a short track and a
via, pours the busiest rail on the free inner layer when the board still routes whole with it, routes the rest
by weighted A* and negotiates the clashes away.  The plan carries its own timing; `seconds` is the router's
whole run (read, build, route, write).  BENCH_ASTAR names the binary; BENCH_ASTAR_ARGS adds arguments.
"""
import json
import os

KIND = "scene->plan"
HERE = os.path.dirname(os.path.abspath(__file__))
SPOTS = [os.path.join(HERE, "..", "..", d, "target", "release", "astar") for d in ("astar-router", "astar-router-repo", os.path.join("krt-better", "astar-router"))]
PLAN_WRITTEN = (0, 1, 3)
ASTAR = os.environ.get("BENCH_ASTAR") or next((p for p in SPOTS if os.path.exists(p)), SPOTS[0])


def route(job):
    plan = os.path.join(job["out_dir"], "plan.json")
    extra = os.environ.get("BENCH_ASTAR_ARGS", "").split()
    r = job["run"]([ASTAR, "route", job["scene"], plan, "--seconds", str(int(job["budget_s"] * 0.9))] + extra)
    if r.timed_out:
        return {"failed": f"stopped at the budget ({job['budget_s']} s)"}
    # astar route: 0 every net joined, 1 nets open, 3 a rule narrowed (--strict): a plan was written each time
    if r.code not in PLAN_WRITTEN or not os.path.exists(plan):
        return {"failed": f"exit {r.code}: {(r.err or r.out)[-300:]}"}
    p = json.load(open(plan))
    s = p.get("summary", {})
    notes = "; ".join(x for x in (s.get("note"), f"pitch {s.get('pitch')} mm", f"{s.get('rounds')} rounds") if x)
    return {"plan": plan, "seconds": p.get("timing_s", {}).get("total", r.seconds), "notes": notes, "extra": {"timing_s": p.get("timing_s"), "summary": s}}
