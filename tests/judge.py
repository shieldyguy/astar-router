#!/usr/bin/env python3
"""judge: the house KiCad's word on one plan (the plan's first judge, plan/README.md).

    judge.py SCENE.json PLAN.json BARE.kicad_pcb [--bare-drc FILE] [--keep DIR] [--name NAME] [--timeout S] [--krt]

The plan's copper is laid on a copy of the bare board in a temp folder of this run's own (bench/shim/
emit_plan.py writes it), the zones are refilled and KiCad's DRC is run with the board's own project, and the
result is compared with the bare board's own DRC: an error is new when the routed board has more of its type
than the bare board. Both DRC runs name every error of every track (--all-track-errors), so a count does not
move from run to run. One JSON row goes to stdout:

    board, nets, landed, unconnected, unlanded_nets, left_out, drc_new {type: n}, warnings_new {type: n},
    track_mm, vias, seconds (the router's own, from the plan), judge_s, pass, why

Exit 0 only when every net landed, KiCad finds nothing unconnected and no error is new; 1 when the plan fails;
2 when the judge itself could not run (the row says why).

KiCad's DRC stops counting a kind of error at 199 (clearance and unconnected items at 499). So: with 499
unconnected items `landed` is an upper bound (`landed_is_at_most`); and a kind of error the bare board has
its limit of already cannot show a new one (`drc_blind`), which is not a pass: the judge cannot tell.

The copy KiCad is given has every uuid made its own (`unique_uuids`): KiCad names the items of a finding by
uuid, and names the wrong twin on a board where two parts share them.

--krt            the second judge beside KiCad: KRT's own checkers (py_router/check_connected.py and check_drc.py of
                 this repo, or of $KRT_ROOT) are run on the routed board, and check_drc on the bare board to tell
                 what is new. The row's `krt`: connected {open_nets, issues}, drc {violations, by_type, graded_at},
                 drc_new, and the nets only one of the two finds open (only_kicad, only_krt). It never changes
                 `pass`: KiCad is the authority. KRT's check_drc holds copper to its own fab floors (hole
                 clearance 0.2 at least), which can ask more than the board's project does.
--bare-drc FILE  the bare board's DRC is read from FILE when it is there, and written to it when it is not
                 (KRT's of the bare board beside it, FILE less .json plus -krt.json)
--keep DIR       the routed board, its project and both DRC reports are copied to DIR
The kicad-cli: $KICAD_CLI, $BENCH_KICAD_CLI, ~/.local/kicad-gs/kicad-gs/kicad-cli, or the one on PATH.
Nothing is written beside the inputs. Several judges may run at once: each has its own temp folder, and a DRC
run holds one of the machine's two heavy slots (ASTAR_HEAVY_JOBS; lock files in /tmp/astar-heavy).
"""
import argparse, collections, contextlib, copy, fcntl, hashlib, json, math, os, re, shutil, signal, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
SHIM = os.path.join(HERE, "..", "bench", "shim")
# warnings about libraries this machine does not have: placement's and the machine's, never the router's
IGNORED_WARNINGS = ("lib_footprint_issues", "lib_footprint_mismatch", "footprint_symbol_mismatch")
PAD_MODES = {"solid": " yes", "thermal": "", "thermal_tht": " thru_hole_only"}
# KiCad's DRC stops counting a kind of error at 199, and clearance and unconnected items at 499
# (pcbnew/drc/drc_engine.cpp: ERROR_LIMIT, EXTENDED_ERROR_LIMIT): a count at its limit means "this many or more"
LIMIT = lambda kind: 499 if kind in ("clearance", "unconnected_items") else 199


@contextlib.contextmanager
def heavy_slot():
    """one of this machine's heavy slots (ASTAR_HEAVY_JOBS, 2 by default), held while a DRC runs: however many
    judges, gates and sessions are going, no more DRC runs than that are going at once"""
    n = max(1, int(os.environ.get("ASTAR_HEAVY_JOBS", "2")))
    d = os.environ.get("ASTAR_HEAVY_DIR", "/tmp/astar-heavy")
    os.makedirs(d, exist_ok=True)
    while True:
        for k in range(n):
            f = open(os.path.join(d, f"slot{k}.lock"), "w")
            try:
                fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError:
                f.close()
                continue
            try:
                yield
            finally:
                f.close()
            return
        time.sleep(0.2)


def kicad_cli():
    for p in (os.environ.get("KICAD_CLI"), os.environ.get("BENCH_KICAD_CLI"),
              os.path.expanduser("~/.local/kicad-gs/kicad-gs/kicad-cli"), shutil.which("kicad-cli")):
        if p and os.path.exists(p):
            return p
    raise RuntimeError("no kicad-cli: set KICAD_CLI (the house build, `. ~/.local/kicad-gs/env.sh`)")


def sh(cmd, timeout, env=None, cwd=None):
    """cmd under nice, in its own process group, killed whole past the timeout: (exit code, stdout + stderr)"""
    p = subprocess.Popen(["nice", "-n", "10"] + cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                         start_new_session=True, env=env, cwd=cwd)
    try:
        out, _ = p.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return None, out
    return p.returncode, out


# ---------------------------------------------------------------- the board file, read lightly

_QUOTED = re.compile(r'"(?:\\.|[^"\\])*"')


def block_spans(lines, kinds):
    """(kind, first line, last line) of each top-level item of these kinds in a tab-indented .kicad_pcb, as KiCad
    writes it. Brackets inside a quoted string are not counted."""
    pat = re.compile(r"^\t\((" + "|".join(kinds) + r")\b")
    i = 0
    while i < len(lines):
        m = pat.match(lines[i])
        if m:
            depth = 0
            j = i
            while j < len(lines):
                bare = _QUOTED.sub('""', lines[j])
                depth += bare.count("(") - bare.count(")")
                if depth <= 0:
                    break
                j += 1
            j = min(j, len(lines) - 1)
            yield m.group(1), i, j
            i = j + 1
            continue
        i += 1


def top_blocks(text, kinds):
    """(kind, block text) for each top-level item of these kinds"""
    lines = text.split("\n")
    for kind, i, j in block_spans(lines, kinds):
        yield kind, "\n".join(lines[i:j + 1])


def layer_table(text):
    """{canonical name: user name} of the copper layers, top to bottom"""
    m = re.search(r"\n\t\(layers\n(.*?)\n\t\)", text, re.S)
    out = {}
    for lm in re.finditer(r'\((\d+) "([^"]+)" (\w+)(?: "([^"]*)")?\)', m.group(1) if m else ""):
        if lm.group(2).endswith(".Cu"):
            out[lm.group(2)] = lm.group(4) or lm.group(2)
    order = lambda n: -1 if n == "F.Cu" else 999 if n == "B.Cu" else int(re.sub(r"\D", "", n) or 0)
    return dict(sorted(out.items(), key=lambda kv: order(kv[0])))


def zone_key(block, canon):
    net = re.search(r'\(net (?:\d+ )?"?([^")]*)"?\)', block)
    lm = re.search(r'\(layers? "([^"]+)"', block)
    return (net.group(1) if net else "", canon.get(lm.group(1), lm.group(1)) if lm else "")


def arc_length(a, m, b):
    (ax, ay), (bx, by), (cx, cy) = a, m, b
    d = 2 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by))
    if abs(d) < 1e-12:
        return math.hypot(cx - ax, cy - ay)
    ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d
    uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d
    r = math.hypot(ax - ux, ay - uy)
    t0, tm, t1 = (math.atan2(y - uy, x - ux) for x, y in (a, m, b))
    turn = lambda p, q: (q - p + math.pi) % (2 * math.pi) - math.pi
    return abs(turn(t0, tm) + turn(tm, t1)) * r


def copper_of(text):
    """(track mm, vias) of a board's own tracks, arcs and vias"""
    mm, vias = 0.0, 0
    nums = lambda key, b: [float(x) for x in re.search(r"\(" + key + r" ([^)]*)\)", b).group(1).split()[:2]]
    for kind, b in top_blocks(text, ["segment", "arc", "via"]):
        if kind == "via":
            vias += 1
        elif kind == "segment":
            (x0, y0), (x1, y1) = nums("start", b), nums("end", b)
            mm += math.hypot(x1 - x0, y1 - y0)
        else:
            mm += arc_length(nums("start", b), nums("mid", b), nums("end", b))
    return mm, vias


# ---------------------------------------------------------------- the plan onto the board

def outline_box(scene):
    xs, ys = [], []
    for q in scene.get("outline", []):
        g = q.get("line") or q.get("arc") or []
        xs += g[0::2]
        ys += g[1::2]
    return (min(xs), min(ys), max(xs), max(ys)) if xs else None


def lay(bare, plan, scene, dst):
    """the plan's copper on a copy of the bare board at dst, the bare board's project beside it. As router-bench
    lays it (CONTRACT.md): layer names made canonical, a via's own size kept, a plane without an outline a zone
    over the whole board, a plane in place of the bare board's zone of that net on that layer, other zones kept."""
    sys.path.insert(0, SHIM)
    import emit_plan
    text = open(bare).read()
    canon = {}
    for c, u in layer_table(text).items():
        canon[u] = c
        canon[c] = c
    p = copy.deepcopy(plan)
    for n in p.get("nets", []):
        n.setdefault("tracks", [])
        n.setdefault("vias", [])
        for t in n["tracks"]:
            t["layer"] = canon.get(t["layer"], t["layer"])
    planes = p.get("planes") or []
    box = outline_box(scene)
    for q in planes:
        q["layer"] = canon.get(q["layer"], q["layer"])
        if not q.get("outline") and box:
            x0, y0, x1, y1 = box
            q["outline"] = [[[x0 - 1, y0 - 1], [x1 + 1, y0 - 1], [x1 + 1, y1 + 1], [x0 - 1, y1 + 1]]]
            q["priority"] = 0
    p["planes"] = planes
    rules = scene.get("rules", {})
    via, drill = rules.get("via", 0.45), rules.get("drill", 0.2)
    emit_plan.write_board(bare, dst, p, via=(via, drill))
    out = open(dst).read()
    named = {(q["net"], q["layer"]) for q in planes}
    for _, b in top_blocks(text, ["zone"]):
        if "(keepout" not in b and zone_key(b, canon) in named:
            out = out.replace(b + "\n", "", 1)
    for n in p.get("nets", []):
        for v in n["vias"]:
            if "size" in v or "drill" in v:
                x, y = v["at"]
                head = f"(at {x:.6f} {y:.6f})\n\t\t(size "
                out = out.replace(f"{head}{via:g})\n\t\t(drill {drill:g})",
                                  f"{head}{v.get('size', via):g})\n\t\t(drill {v.get('drill', drill):g})", 1)
    mode = {(q["net"], q["layer"]): PAD_MODES[q["pads"]] for q in planes if q.get("pads") in PAD_MODES}
    if mode:
        own = {b for _, b in top_blocks(text, ["zone"])}
        for _, b in list(top_blocks(out, ["zone"])):
            m = mode.get(zone_key(b, canon))
            if m is not None and b not in own and "(connect_pads yes" in b:
                out = out.replace(b, b.replace("(connect_pads yes", "(connect_pads" + m, 1), 1)
    open(dst, "w").write(unique_uuids(out))
    copy_project(bare, dst)


_UUID = re.compile(r'\(uuid "([^"]+)"\)')


def unique_uuids(text):
    """the board's text with every uuid its own: the second and later holders of one get a new one made from
    it. KiCad's DRC report names an item by looking its uuid up, so on a board where two parts share uuids (a
    block placed twice) it names the wrong twin, with the wrong net: fix_lakehouse.placed has 188 such, and
    an open Net-(U5-CPP) was reported as "Pad 1 [SUS] of C17"."""
    seen = collections.Counter()

    def one(m):
        u = m.group(1)
        seen[u] += 1
        if seen[u] == 1:
            return m.group(0)
        h = hashlib.sha1(f"{u}#{seen[u]}".encode()).hexdigest()
        return f'(uuid "{h[:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}")'

    return _UUID.sub(one, text)


def copy_project(src, dst):
    for ext in (".kicad_pro", ".kicad_dru"):
        s = src[:-len(".kicad_pcb")] + ext
        if os.path.exists(s):
            shutil.copy(s, dst[:-len(".kicad_pcb")] + ext)


# ---------------------------------------------------------------- KiCad's DRC

def drc(board, out_json, timeout):
    """KiCad's DRC of this board with its zones refilled (the board is not saved)"""
    # --all-track-errors: without it KiCad names one error a track, and which one moves with the tracks' uuids,
    # which are new every time a plan is laid: the same plan gave shorting_items 1 one run and 0 the next
    cmd = [kicad_cli(), "pcb", "drc", "--format", "json", "--severity-error", "--severity-warning", "--refill-zones",
           "--all-track-errors", "-o", out_json, board]
    with heavy_slot():
        code, out = sh(cmd, timeout)
    if code is None:
        raise RuntimeError(f"KiCad's DRC did not end in {timeout:.0f} s")
    if not os.path.exists(out_json):
        raise RuntimeError(f"KiCad's DRC wrote nothing (exit {code}): {out[-300:].strip()}")
    return tally(json.load(open(out_json)))


def tally(d):
    vs, unc = d.get("violations", []), d.get("unconnected_items", [])
    errs = collections.Counter(v["type"] for v in vs if v.get("severity") == "error")
    warns = collections.Counter(v["type"] for v in vs if v.get("severity") == "warning")
    by_net, by_net_zone = collections.Counter(), collections.Counter()
    for u in unc:
        nets = set()
        for it in u.get("items", []):
            # "Pad 2 [GND] of R3 on F.Cu": the net is between the first [ and the last ] (a name may hold brackets)
            s = it.get("description", "")
            a, b = s.find("["), s.rfind("]")
            if 0 <= a < b:
                nets.add(s[a + 1:b])
        for n in nets:
            by_net[n] += 1
            # a break whose one side is a zone: a piece of a pour that touches nothing of its net
            if any(it.get("description", "").startswith("Zone") for it in u.get("items", [])):
                by_net_zone[n] += 1
    # copper a footprint draws over its own pad (a SOT-89 tab, an antenna feed): KiCad calls the drawing netless
    # and faults the pad for touching it on the bare board, and then every track that reaches that pad. Such a
    # track cannot be laid clean; KRT's own DRC waives it too (its "own-pad" lift). art_pads: the (footprint,
    # net) pairs the board itself is faulted for; art_hits: the errors between such a drawing and a track or via.
    def parts(it):
        d = it.get("description", "")
        a, b = d.find("["), d.rfind("]")
        net = d[a + 1:b] if 0 <= a < b else None
        ref = d[b + 1:].split(" of ", 1)[1].split(" on ", 1)[0].strip() if " of " in d[b + 1:] else None
        return d.split(" ", 1)[0], net, ref
    art_pads, art_hits = set(), collections.Counter()
    for v in vs:
        if v.get("severity") != "error" or len(v.get("items", [])) != 2:
            continue
        x, y = parts(v["items"][0]), parts(v["items"][1])
        for g, o in ((x, y), (y, x)):
            if g[1] == "<no net>" and g[2] and o[1] and o[1] != "<no net>":
                if o[0] in ("Pad", "PTH", "SMD") and o[2] == g[2]:
                    art_pads.add((g[2], o[1]))
                elif o[0] in ("Track", "Via"):
                    art_hits[f"{v['type']}|{g[2]}|{o[1]}"] += 1
    # every error by what it is between: its kind, and each thing it names with where that thing is. An error
    # the bare board has is the same error on the routed board; one that names other things is a new one, even
    # when the bare board has as many of its kind.
    def ident(v):
        things = sorted("%s@%.3f,%.3f" % (it.get("description", ""), (it.get("pos") or {}).get("x", 0), (it.get("pos") or {}).get("y", 0)) for it in v.get("items", []))
        return v["type"] + "|" + "; ".join(things)
    ids = sorted(ident(v) for v in vs if v.get("severity") == "error")
    return {"ids": ids, "errors": dict(errs), "warnings": dict(warns), "unconnected": len(unc), "unconnected_by_net": dict(by_net),
            "islands_only": sorted(n for n in by_net if by_net_zone[n] == by_net[n]),
            "art_pads": sorted(map(list, art_pads)), "art_hits": dict(art_hits)}


def bare_drc(bare, cache, tmp, timeout):
    if cache and os.path.exists(cache):
        d = json.load(open(cache))
        if "art_pads" in d and "ids" in d:
            return d
    b = os.path.join(tmp, "bare", os.path.basename(bare))
    os.makedirs(os.path.dirname(b))
    open(b, "w").write(unique_uuids(open(bare).read()))
    copy_project(bare, b)
    d = drc(b, os.path.join(tmp, "bare-drc.json"), timeout)
    if cache:
        part = f"{cache}.{os.getpid()}.part"
        json.dump(d, open(part, "w"), indent=1)
        os.replace(part, cache)
    return d


# ---------------------------------------------------------------- KRT's checkers (the second judge)

def krt_root():
    """the KRT checkout whose checkers are asked: $KRT_ROOT, else the repo this file is in"""
    for d in (os.environ.get("KRT_ROOT"), os.path.join(HERE, "..", "..")):
        if d and os.path.exists(os.path.join(d, "py_router", "check_connected.py")):
            return os.path.abspath(d)
    return None


def krt_run(tool, args, timeout, cwd):
    env = dict(os.environ, KICAD_CLI=kicad_cli())          # its own cross-check against KiCad's refill runs with it
    with heavy_slot():
        return sh([sys.executable, os.path.join(krt_root(), "py_router", tool)] + args, timeout, env=env, cwd=cwd)


def krt_connected(board, timeout, cwd):
    """KRT's check_connected.py on a board: {"open_nets": [names], "issues": n}, or {"failed": why}. An exit code
    alone is not taken for an answer: the tool must say "ALL NETS FULLY CONNECTED" or "FOUND N ISSUES"."""
    code, out = krt_run("check_connected.py", [board], timeout, cwd)
    if code is None:
        return {"failed": f"still going after {timeout:.0f} s"}
    m = re.search(r"^FOUND (\d+) ISSUES?:", out, re.M)
    if "ALL NETS FULLY CONNECTED" in out and code == 0:
        return {"open_nets": [], "issues": 0}
    if not m or code != 1:
        return {"failed": f"exit {code}: {out.strip()[-200:]}"}
    nets, unrouted = [], False
    for ln in out.split("\n"):
        if ln.startswith("  Unrouted nets ("):
            unrouted = True
        elif ln.startswith("  Connectivity issues ("):
            unrouted = False
        elif unrouted and (u := re.match(r"^    (.+) \(\d+ pads\)$", ln)):
            nets.append(u.group(1))
        elif (c := re.match(r"^  (.+) \(net \d+\):$", ln)):
            nets.append(c.group(1))
    return {"open_nets": sorted(set(nets)), "issues": int(m.group(1))}


def krt_drc(board, timeout, cwd, tag):
    """KRT's check_drc.py on a board, at the board's own project's clearance: {"violations": n, "by_type": {..},
    "graded_at": {..}}, or {"failed": why}"""
    out_json = os.path.join(cwd, f"krt-drc-{tag}.json")
    code, out = krt_run("check_drc.py", [board, "--json", out_json, "-q"], timeout, cwd)
    if code is None:
        return {"failed": f"still going after {timeout:.0f} s"}
    if code not in (0, 1) or not os.path.exists(out_json):
        return {"failed": f"exit {code}: {out.strip()[-200:]}"}
    d = json.load(open(out_json))
    g = d.get("graded_at") or {}
    return {"violations": d.get("violations", 0), "by_type": d.get("by_type") or {},
            "graded_at": {k: g.get(k) for k in ("clearance", "hole_clearance", "hole_to_hole_clearance", "board_edge_clearance")}}


def krt(bare, routed, cache, tmp, timeout=180):
    """the second judge's row: KRT's check_connected and check_drc on the routed board, and its check_drc on the bare
    board to tell which violations are new"""
    if not krt_root():
        return {"failed": "no KRT checkout (py_router/check_connected.py): set KRT_ROOT"}
    out = {"connected": krt_connected(routed, timeout, tmp)}
    now = krt_drc(routed, timeout, tmp, "routed")
    side = cache[:-len(".json")] + "-krt.json" if cache and cache.endswith(".json") else None
    if side and os.path.exists(side):
        before = json.load(open(side))
    else:
        b = os.path.join(tmp, "bare-krt", os.path.basename(bare))
        os.makedirs(os.path.dirname(b), exist_ok=True)
        shutil.copy(bare, b)
        copy_project(bare, b)
        before = krt_drc(b, timeout, tmp, "bare")
        if side and "failed" not in before:
            json.dump(before, open(side, "w"), indent=1)
    out["drc"] = now
    if "failed" not in now and "failed" not in before:
        out["drc_new"] = new_of(now["by_type"], before["by_type"])
        out["drc_new_total"] = sum(out["drc_new"].values())
    elif "failed" in before:
        out["drc_bare_failed"] = before["failed"]
    return out


def shown(name):
    """A net's name as KiCad's report writes it: the board file's {slash} is /."""
    return name.replace("{slash}", "/").replace("{colon}", ":").replace("{backslash}", "\\")


def routable_nets(scene):
    """the scene's nets with two or more pads: the nets a router is asked to join"""
    c = collections.Counter(p["net"] for p in scene["pads"] if p.get("net"))
    return sorted(n["name"] for n in scene["nets"] if c[n["name"]] >= 2)


def new_of(now, before, ignore=()):
    return {k: v - before.get(k, 0) for k, v in sorted(now.items()) if v - before.get(k, 0) > 0 and k not in ignore}


def judge(scene_path, plan_path, bare, cache=None, keep=None, name=None, timeout=900, with_krt=False):
    t0 = time.time()
    scene, plan = json.load(open(scene_path)), json.load(open(plan_path))
    name = name or scene.get("bench", {}).get("board") or os.path.basename(bare)[:-len(".kicad_pcb")]
    tmp = tempfile.mkdtemp(prefix="astar-judge-")
    try:
        routed = os.path.join(tmp, "routed", os.path.basename(bare))
        os.makedirs(os.path.dirname(routed))
        before = bare_drc(bare, cache, tmp, timeout)
        lay(bare, plan, scene, routed)
        now = drc(routed, os.path.join(tmp, "drc.json"), timeout)
        mm, vias = copper_of(open(routed).read())
        second = krt(bare, routed, cache, tmp) if with_krt else None
        if keep:
            os.makedirs(keep, exist_ok=True)
            for f in os.listdir(os.path.dirname(routed)):
                shutil.copy(os.path.join(os.path.dirname(routed), f), keep)
            shutil.copy(os.path.join(tmp, "drc.json"), keep)
            json.dump(before, open(os.path.join(keep, "bare-drc.json"), "w"), indent=1)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    nets = routable_nets(scene)
    open_nets = [n for n in nets if now["unconnected_by_net"].get(shown(n))]
    outside = sorted(set(now["unconnected_by_net"]) - {shown(n) for n in nets})
    # errors that only say a track reached a pad its own footprint draws copper over: not the router's
    own = {tuple(x) for x in before.get("art_pads", [])}
    waived = collections.Counter()
    for key, n in now.get("art_hits", {}).items():
        kind, ref, net = key.split("|", 2)
        if (ref, net) in own:
            waived[kind] += n
    # new: the errors the bare board does not have, each told by what it is between (`tally`), not by how
    # many of its kind there are
    fresh = collections.Counter(now["ids"]) - collections.Counter(before["ids"])
    by_kind = collections.Counter()
    for key, n in fresh.items():
        by_kind[key.split("|", 1)[0]] += n
    new_err = {k: v - waived.get(k, 0) for k, v in sorted(by_kind.items()) if v - waived.get(k, 0) > 0}
    # what KiCad's limits hide: a list of unconnected items cut short names fewer open nets than there are, and
    # a kind of error the bare board has its limit of already cannot show one more
    cut = now["unconnected"] >= LIMIT("unconnected_items")
    blind = sorted(k for k, v in before["errors"].items() if v >= LIMIT(k))
    why = []
    if open_nets:
        why.append(f"{len(open_nets)} of {len(nets)} nets open" + (" or more (KiCad lists 499 unconnected items and stops)" if cut else ""))
    if outside:
        why.append(f"{len(outside)} nets open that the scene does not ask for ({', '.join(outside[:3])})")
    if now["unconnected"] and not open_nets and not outside:
        why.append(f"{now['unconnected']} unconnected items KiCad names no net for")
    if new_err:
        why.append("new DRC: " + ", ".join(f"{v}{'+' if now['errors'][k] >= LIMIT(k) else ''} {k}" for k, v in new_err.items()))
    if blind:
        why.append("cannot tell new " + ", ".join(blind) + ": the bare board has KiCad's limit of them already")
    if second is not None and "open_nets" in second.get("connected", {}):
        # the two judges on which nets are open (KiCad's list is cut at 499 items: then only what KRT misses is told)
        kicad, theirs = set(open_nets) | set(outside), set(second["connected"]["open_nets"])
        second["only_kicad"] = sorted(kicad - theirs)[:12]
        second["only_krt"] = [] if cut else sorted(theirs - kicad)[:12]
    row_krt = {"krt": second} if second is not None else {}
    return {**row_krt, "board": name, "nets": len(nets), "landed": len(nets) - len(open_nets), "unconnected": now["unconnected"],
            "unlanded_nets": open_nets, "unlanded_outside_scene": outside,
            # of the open nets, those whose every break is a piece of pour left on its own: their pads are joined
            "islands_only": [n for n in open_nets if n in set(now.get("islands_only", []))],
            "left_out": len(plan.get("left_out", [])),
            "drc_new": new_err, "drc_new_total": sum(new_err.values()), "drc_waived": dict(waived),
            "landed_is_at_most": cut, "drc_blind": blind,
            "warnings_new": new_of(now["warnings"], before["warnings"], IGNORED_WARNINGS),
            "bare_unconnected": before["unconnected"],
            "track_mm": round(mm, 1), "vias": vias,
            "seconds": (plan.get("timing_s") or {}).get("total"), "judge_s": round(time.time() - t0, 1),
            "pass": not why, "why": "; ".join(why)}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("scene")
    ap.add_argument("plan")
    ap.add_argument("board", help="the bare .kicad_pcb the scene was made from")
    ap.add_argument("--bare-drc", help="the bare board's DRC, read from here if there, else written here")
    ap.add_argument("--keep", help="a folder for the routed board and the DRC reports")
    ap.add_argument("--name")
    ap.add_argument("--timeout", type=float, default=900, help="seconds for each DRC run (900)")
    ap.add_argument("--krt", action="store_true", help="also ask KRT's checkers (the second judge): the row's `krt`")
    a = ap.parse_args()
    try:
        row = judge(a.scene, a.plan, a.board, cache=a.bare_drc, keep=a.keep, name=a.name, timeout=a.timeout, with_krt=a.krt)
    except Exception as e:
        print(json.dumps({"board": a.name or os.path.basename(a.board), "pass": False, "judge_failed": True,
                          "why": f"the judge could not run: {type(e).__name__}: {e}"}))
        return 2
    print(json.dumps(row))
    return 0 if row["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
