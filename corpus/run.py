#!/usr/bin/env python3
"""run: astar over the corpus, each board judged by KiCad (T13.2, T13.3).

    run.py [NAME ...] [--budget 60] [--jobs 2] [--resume] [--krt] [--out corpus/rows.jsonl]
    run.py --smoke        the boards named in corpus/smoke.txt, each against its row in corpus/baseline.jsonl

For each board of corpus/manifest.json: astar routes its scene (`astar route SCENE PLAN --seconds BUDGET`, stopped
BUDGET + 30 s in), astar's own exact check looks at the plan (`astar check`: the row's `check`, with the nets
only one of the two judges finds open), tests/judge.py lays the plan on the bare board and asks KiCad, whose
word is the status. A row a board goes to
corpus/rows.jsonl as it ends, and a table is printed at the end:

    board, layers, nets, landed, unconnected, new DRC, seconds (astar's own), and why a board did not pass

A board's status is one of:
    pass           every net landed, nothing unconnected, no new DRC error
    nets open      KiCad finds nets not joined (astar may or may not have said so: `left_out` is its own word)
    DRC            every net landed, but with DRC errors the bare board does not have
    nets open, DRC both
    no scene       the board gave no scene (the manifest says why): astar was not run
    not prepared   a board too large to commit that has not been made here (corpus/prepare.py NAME)
    scene refused  astar would not take the scene (exit 2, its own message)
    crash          astar died (a panic, a signal, or any other exit)
    timeout        astar was still going 30 s past its budget
    judge failed   the plan could not be judged (the row says why)
    cannot tell    all landed and nothing new seen, but the bare board already has KiCad's limit of some kind of
                   error (199), so a new one of that kind would not show
A landed count with 499 unconnected items is an upper bound: KiCad stops listing them there (`landed_is_at_most`).

At most two jobs at once; astar and KiCad's DRC each hold one of the machine's two heavy slots while they run.
astar's memory is capped a job (--mem-gb, 3): two jobs share a 7 GB machine, and a board that needs more dies
of a failed allocation, which the row calls a crash and says so.
The plan, astar's words, the routed board and the DRC report of every board are kept in corpus/out/NAME/ (not
committed). --resume keeps the rows the out file already has and runs the rest.

--budget-stress S gives the boards of the stress corpus another budget (each row says its own, `budget_s`).
$ASTAR names the binary and $ASTAR_REV its commit, for a long run that must not change binary half way.
The baseline (corpus/baseline.jsonl) is a whole run's rows.jsonl, copied over it in a commit that says why.
--smoke exits 1 when a smoke board crashes, times out, or is worse than its baseline row: fewer nets landed,
more new DRC errors, or no longer a pass.
"""
import argparse, concurrent.futures as cf, json, os, re, resource, shutil, subprocess, sys, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(ROOT, "tests"))
import judge as J

MANIFEST = os.path.join(HERE, "manifest.json")
BOARDS = os.path.join(HERE, "boards")
CACHE = os.environ.get("ASTAR_CORPUS_CACHE") or "/home/claude/corpus-cache"       # third-party boards (source "stress")
OUT = os.path.join(HERE, "out")
OWN_SCENE = False
BASELINE = os.path.join(HERE, "baseline.jsonl")
SMOKE = os.path.join(HERE, "smoke.txt")
GRACE = 30


def commit():
    if os.environ.get("ASTAR_REV"):          # a binary kept aside for a long run says itself which commit it is
        return os.environ["ASTAR_REV"]
    r = subprocess.run(["git", "-C", ROOT, "rev-parse", "--short", "HEAD"], capture_output=True, text=True)
    d = subprocess.run(["git", "-C", ROOT, "status", "--porcelain", "--", "src", "Cargo.toml"], capture_output=True, text=True)
    return (r.stdout.strip() or "?") + ("+dirty" if d.stdout.strip() else "")


def route(astar, scene, plan, log, budget, mem_gb):
    """(status or None, why, wall seconds): astar on one scene, under nice, holding a heavy slot, its memory
    capped (two jobs share the machine: past the cap an allocation fails and astar dies, which is a crash)"""
    cmd = ["nice", "-n", "10", astar, "route", scene, plan, "--seconds", str(budget)]
    cap = int(mem_gb * (1 << 30))
    limit = (lambda: resource.setrlimit(resource.RLIMIT_AS, (cap, cap))) if mem_gb > 0 else None
    t0 = time.time()
    with J.heavy_slot():
        t0 = time.time()
        p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, start_new_session=True, preexec_fn=limit)
        try:
            out, _ = p.communicate(timeout=budget + GRACE)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, 9)
            out, _ = p.communicate()
            open(log, "w").write(out or "")
            return "timeout", f"still going {GRACE} s past its {budget} s budget", time.time() - t0
    wall = time.time() - t0
    open(log, "w").write(out or "")
    last = ([ln.strip() for ln in (out or "").split("\n") if ln.strip()] or [""])[-1]
    # astar route: 0 every net joined, 1 nets open, 3 a rule narrowed, 5 joined but KiCad will fault what
    # astar could not help (a short the board came with): a plan each time; 2 the scene refused
    if p.returncode in (0, 1, 3, 5) and os.path.exists(plan):
        return None, last, wall
    if p.returncode == 2:
        return "scene refused", last[:300], wall
    said = [ln.strip() for ln in (out or "").split("\n") if ln.strip()]
    panic = next((ln for ln in said if "panicked at" in ln), "")
    if panic and said.index(panic) + 1 < len(said):
        panic += " " + said[said.index(panic) + 1]            # the line after says what the panic was
    how = f"signal {-p.returncode}" if p.returncode < 0 else f"exit {p.returncode}"
    if "memory allocation" in (out or ""):
        how += f", out of memory under the {mem_gb:g} GB cap"
    return "crash", f"{how}: {(panic or last)[:300]}", wall


_OPEN = re.compile(r"^(open|open-unsaid) (.*) at \(-?[\d.]+, -?[\d.]+\): its \d+ pads are in ")


def check(astar, scene, plan):
    """astar's own exact check of the plan (the third judge, `astar check`): how many findings of each kind, and
    the nets it finds open; or why it could not run"""
    try:
        r = subprocess.run(["nice", "-n", "10", astar, "check", scene, plan], capture_output=True, text=True, timeout=300)
    except subprocess.TimeoutExpired:
        return {"failed": "still going after 300 s"}
    if r.returncode not in (0, 1):
        return {"failed": f"exit {r.returncode}: {r.stderr.strip()[-200:]}"}
    kinds, opened = {}, []
    for ln in r.stdout.split("\n"):
        if not ln.strip() or ln.startswith("... and"):
            continue
        k = ln.split(" ", 1)[0]
        kinds[k] = kinds.get(k, 0) + 1
        m = _OPEN.match(ln)
        if m:
            opened.append(m.group(2))
    return {"findings": sum(kinds.values()), "kinds": kinds, "open_nets": opened}


def run_one(entry, astar, budget, rev, mem_gb, with_krt=False):
    name = entry["name"]
    row = {"board": name, "source": entry["source"], "layers": entry["copper_layers"], "commit": rev, "budget_s": budget}
    bd = os.path.join(CACHE, "boards", name) if entry["source"] == "stress" else os.path.join(BOARDS, name)
    scene, board = os.path.join(bd, "scene.json"), os.path.join(bd, name + ".kicad_pcb")
    if OWN_SCENE and os.path.exists(board):
        # the scene astar reads from the board itself, not the one KiCad's exporter made
        os.makedirs(os.path.join(OUT, name), exist_ok=True)
        scene = os.path.join(OUT, name + ".scene.json")
        r = subprocess.run([astar, "scene", board, "-o", scene], capture_output=True, text=True)
        if r.returncode != 0 or not os.path.exists(scene):
            return dict(row, status="scene refused", nets=entry.get("nets"), why=(r.stderr.strip().splitlines() or ["astar scene failed"])[-1][:200])
        sc = json.load(open(scene))
        count = {}
        for pad in sc.get("pads", []):
            if pad.get("net"):
                count[pad["net"]] = count.get(pad["net"], 0) + 1
        entry = dict(entry, nets=sum(1 for v in count.values() if v >= 2), scene=True)
    if not entry.get("scene"):
        return dict(row, status="no scene", nets=entry.get("nets"), why=entry.get("scene_why") or "")
    if not os.path.exists(scene) or not os.path.exists(board):
        how = f"corpus/fetch_stress.py --only {name}, then corpus/prepare.py {name}" if entry["source"] == "stress" else f"corpus/prepare.py {name}"
        return dict(row, status="not prepared", nets=entry.get("nets"), why=f"not committed ({'third-party' if entry['source'] == 'stress' else 'too large'}): run {how}")
    out = os.path.join(OUT, name)
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    plan = os.path.join(out, "plan.json")
    row["nets"] = entry.get("nets")
    status, why, wall = route(astar, scene, plan, os.path.join(out, "astar.log"), budget, mem_gb)
    row["wall_s"] = round(wall, 1)
    if status:
        return dict(row, status=status, why=why)
    try:
        p = json.load(open(plan))
    except ValueError as e:
        return dict(row, status="crash", why=f"the plan is not JSON: {e}")
    row["astar"] = {"left_out": len(p.get("left_out", [])), "note": (p.get("summary") or {}).get("note", ""),
                    "rounds": (p.get("summary") or {}).get("rounds"), "poured": [f"{q['net']}@{q['layer']}" for q in p.get("planes", [])]}
    if entry.get("pads_not_in_scene"):
        row["pads_not_in_scene"] = entry["pads_not_in_scene"]
    c = check(astar, scene, plan)
    try:
        j = J.judge(scene, plan, board, cache=os.path.join(bd, "bare-drc.json"), keep=out, name=name, with_krt=with_krt)
    except Exception as e:
        return dict(row, status="judge failed", check=c, why=f"{type(e).__name__}: {e}"[:300])
    if "open_nets" in c:
        # the two judges on which nets are open: KiCad's list is cut at 499 items, so what only the check finds
        # open is told only when KiCad's list is whole
        kicad, own = set(j["unlanded_nets"]) | set(j["unlanded_outside_scene"]), set(c.pop("open_nets"))
        c.update(open=len(own), only_kicad=sorted(kicad - own)[:12], only_check=[] if j["landed_is_at_most"] else sorted(own - kicad)[:12])
    row["check"] = c
    if "krt" in j:
        row["krt"] = j["krt"]
    opened = j["landed"] < j["nets"] or j["unconnected"] > 0
    # every pad joined and only stray pieces of pour left over is its own word: not the router's nets
    isl = j.get("islands_only", [])
    only_islands = opened and bool(isl) and set(j["unlanded_nets"]) <= set(isl) and not j["unlanded_outside_scene"]
    status = "pass" if j["pass"] else ", ".join(x for x in (("pour islands" if only_islands else "nets open") if opened else "", "DRC" if j["drc_new_total"] else "") if x) or "cannot tell"
    row["islands_only"] = isl[:12]
    row.update({k: j[k] for k in ("nets", "landed", "unconnected", "unlanded_nets", "unlanded_outside_scene", "left_out", "drc_new",
                                  "drc_new_total", "landed_is_at_most", "drc_blind", "warnings_new", "track_mm", "vias", "seconds", "judge_s")})
    row["unlanded_nets"] = row["unlanded_nets"][:12]
    why = j["why"]
    if opened:
        # astar's own word beside KiCad's: fewer left out than KiCad finds open is a net called joined that is not
        said = row["left_out"]
        found = len(j["unlanded_nets"]) + len(j["unlanded_outside_scene"])
        why += f" (astar left out {said}" + (f": {found - said} it called joined are not)" if found > said else ")")
    if row["check"].get("only_kicad"):
        why += f"; {len(row['check']['only_kicad'])} nets open by KiCad that astar check finds joined ({', '.join(row['check']['only_kicad'][:3])})"
    if row.get("pads_not_in_scene"):
        why += f"; the scene lacks {row['pads_not_in_scene']} pads of the board"
    return dict(row, status=status, why=why)


def table(rows):
    head = f"{'board':42s} {'L':>2s} {'nets':>5s} {'landed':>6s} {'unconn':>6s} {'newDRC':>6s} {'sec':>6s}  status"
    out = [head, "-" * len(head)]
    f = lambda v, w, fmt="{}": (fmt.format(v) if v is not None else "-").rjust(w)
    for r in rows:
        why = "" if r["status"] == "pass" else ": " + (r.get("why") or "")
        landed = None if r.get("landed") is None else f"{'<=' if r.get('landed_is_at_most') else ''}{r['landed']}"
        out.append(f"{r['board']:42s} {f(r.get('layers'), 2)} {f(r.get('nets'), 5)} {f(landed, 6)} {f(r.get('unconnected'), 6)} "
                   f"{f(r.get('drc_new_total'), 6)} {f(r.get('seconds'), 6, '{:.1f}')}  {r['status']}{why}"[:260])
    return "\n".join(out)


def summary(rows):
    by = {}
    for r in rows:
        by.setdefault(r["status"], []).append(r["board"])
    with_scene = [r for r in rows if r["status"] not in ("no scene", "not prepared")]
    lines = [f"{len(rows)} boards; {len(with_scene)} with a scene; {len(by.get('pass', []))} whole with 0 new DRC errors"]
    for s in sorted(by, key=lambda s: (s != "pass", -len(by[s]))):
        lines.append(f"  {len(by[s]):3d} {s}" + ("" if s == "pass" else f" (e.g. {by[s][0]})"))
    return "\n".join(lines)


def load_rows(path):
    return [json.loads(ln) for ln in open(path) if ln.strip()] if os.path.exists(path) else []


def worse(now, base):
    """why this row is worse than its baseline row, or None"""
    if now["status"] in ("crash", "timeout", "scene refused", "judge failed", "no scene", "not prepared"):
        return f"{now['status']}: {now.get('why', '')}"[:200]
    if base is None:
        return None
    if base["status"] == "pass" and now["status"] != "pass":
        return f"passed in the baseline, now {now['status']}: {now.get('why', '')}"[:200]
    if now.get("landed", 0) < base.get("landed", 0):
        return f"landed {now.get('landed')} of {now.get('nets')}, the baseline {base.get('landed')}"
    if now.get("drc_new_total", 0) > base.get("drc_new_total", 0):
        return f"{now.get('drc_new_total')} new DRC errors, the baseline {base.get('drc_new_total')}"
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("names", nargs="*")
    ap.add_argument("--budget", type=float, default=60, help="astar's seconds a board (60)")
    ap.add_argument("--budget-stress", type=float, default=0, help="astar's seconds a board of the stress corpus (0: --budget)")
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--mem-gb", type=float, default=3, help="astar's memory cap a job, GB (3: two jobs on a 7 GB machine; 0: none)")
    ap.add_argument("--resume", action="store_true")
    ap.add_argument("--out", default=None, help="the rows file (corpus/rows.jsonl; corpus/out/smoke.jsonl with --smoke)")
    ap.add_argument("--smoke", action="store_true")
    ap.add_argument("--exported-scene", action="store_true", help="use the scene.json KiCad's exporter made (as the corpus first did) instead of reading each board with `astar scene`")
    ap.add_argument("--krt", action="store_true", help="also ask KRT's checkers about every routed board (the row's `krt`)")
    a = ap.parse_args()
    global OWN_SCENE
    OWN_SCENE = not a.exported_scene
    astar = os.environ.get("ASTAR") or os.path.join(ROOT, "target", "release", "astar")
    if not os.path.exists(astar) or not os.path.exists(MANIFEST):
        print(f"FAIL corpus: {'no binary at ' + astar if not os.path.exists(astar) else 'no corpus/manifest.json'}")
        return 2
    entries = {e["name"]: e for e in json.load(open(MANIFEST))["boards"]}
    names = a.names
    if a.smoke:
        if not os.path.exists(SMOKE) or not os.path.exists(BASELINE):
            print("FAIL corpus smoke: corpus/smoke.txt or corpus/baseline.jsonl is not there")
            return 2
        names = [ln.strip() for ln in open(SMOKE) if ln.strip() and not ln.startswith("#")]
    unknown = [n for n in names if n not in entries]
    if unknown:
        print(f"FAIL corpus: not in the manifest: {', '.join(unknown)}")
        return 2
    todo = [entries[n] for n in names] if names else list(entries.values())
    os.makedirs(OUT, exist_ok=True)
    out_path = a.out or (os.path.join(OUT, "smoke.jsonl") if a.smoke else os.path.join(HERE, "rows.jsonl"))
    # --resume: the rows already there are kept, but for the boards that were not prepared then (they may be now)
    done = {r["board"]: r for r in load_rows(out_path) if r["status"] != "not prepared"} if a.resume else {}
    if not a.resume:
        open(out_path, "w").close()
    rev = commit()
    lock = threading.Lock()
    t0 = time.time()

    def job(e):
        if e["name"] in done:
            return done[e["name"]]
        r = run_one(e, astar, a.budget_stress if e["source"] == "stress" and a.budget_stress else a.budget, rev, a.mem_gb, a.krt)
        with lock:
            with open(out_path, "a") as f:
                f.write(json.dumps(r) + "\n")
            if not a.smoke:
                print(f"  {r['board']:42s} {r['status']}", file=sys.stderr, flush=True)
        return r

    with cf.ThreadPoolExecutor(max(1, min(2, a.jobs))) as ex:
        rows = list(ex.map(job, todo))
    with open(out_path, "w") as f:           # in the manifest's order, one row a board
        for r in rows:
            f.write(json.dumps(r) + "\n")
    if a.smoke:
        base = {r["board"]: r for r in load_rows(BASELINE)}
        bad = 0
        for r in rows:
            w = worse(r, base.get(r["board"]))
            bad += bool(w)
            got = f"{r.get('landed', '-')}/{r.get('nets', '-')} landed, {r.get('drc_new_total', '-')} new DRC errors, {r.get('seconds') or 0:.1f} s"
            print(f"{'FAIL' if w else 'ok  '} smoke {r['board']:36s} {got}" + (f": {w}" if w else ""))
        print(f"{'FAIL' if bad else 'ok  '} corpus smoke: {len(rows) - bad} of {len(rows)} boards no worse than the baseline ({time.time() - t0:.0f} s)")
        return 1 if bad else 0
    print(table(rows))
    print(summary(rows))
    print(f"rows: {out_path}; artifacts: {OUT}/NAME/; {time.time() - t0:.0f} s")
    return 0


if __name__ == "__main__":
    sys.exit(main())
