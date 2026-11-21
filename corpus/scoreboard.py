#!/usr/bin/env python3
"""scoreboard: a run's rows as a page anyone can read (T13.4, astar's half).

    scoreboard.py [--rows corpus/baseline.jsonl] [--out corpus/SCOREBOARD.md]

Reads the rows of a corpus run (corpus/run.py) and the manifest, writes one Markdown page: what was run, a
summary by source, the boards by failure class, what the second and third judges say beside KiCad, and a row
a board (source, layers, nets, landed, new DRC, seconds, verdict, the reason in a few words).
The verdict is KiCad's (tests/judge.py): the router's own count is never the grade.
"""
import argparse, collections, json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ORDER = ["bench", "krt", "house", "qa", "demo", "stress"]
WHAT = {"bench": "router-bench's five", "krt": "KRT's kicad_files/", "house": "the house's blocks and fixtures",
        "qa": "the KiCad fork's field tests", "demo": "the KiCad fork's demos", "stress": "KRT's stress corpus (open boards, fetched)"}
CLASSES = [("pass", "every net landed, 0 new DRC errors"),
           ("nets open", "KiCad finds nets not joined; no new DRC error"),
           ("nets open, DRC", "nets not joined, and DRC errors the bare board does not have"),
           ("DRC", "every net landed, with DRC errors the bare board does not have"),
           ("cannot tell", "all landed, but the bare board has KiCad's limit of some error already"),
           ("crash", "astar died"),
           ("timeout", "astar was still going 30 s past its budget"),
           ("scene refused", "astar would not take the scene"),
           ("judge failed", "the plan could not be judged"),
           ("no scene", "the board gave no scene: astar was not run"),
           ("not prepared", "fetched or listed, not made bare and exported on this machine yet")]


def reason(r):
    """why a board did not pass, in a few words"""
    s, why = r["status"], r.get("why") or ""
    if s == "pass":
        return ""
    if s in ("nets open", "nets open, DRC", "DRC", "cannot tell"):
        parts = []
        opened = (r.get("nets") or 0) - (r.get("landed") or 0)
        if opened or r.get("unlanded_outside_scene"):
            n = opened + len(r.get("unlanded_outside_scene") or [])
            parts.append(f"{n}{'+' if r.get('landed_is_at_most') else ''} open")
            m = re.search(r"(\d+) it called joined are not", why)
            if m:
                parts.append(f"{m.group(1)} called joined")
            if r.get("pads_not_in_scene"):
                parts.append(f"scene lacks {r['pads_not_in_scene']} pads")
        kinds = sorted((r.get("drc_new") or {}).items(), key=lambda kv: -kv[1])
        if kinds:
            parts.append(", ".join(f"{v} {k}" for k, v in kinds[:2]) + (f", +{len(kinds) - 2} kinds" if len(kinds) > 2 else ""))
        if r.get("drc_blind"):
            parts.append("blind to " + ", ".join(r["drc_blind"][:2]))
        return "; ".join(parts)
    if s == "crash":
        m = re.search(r"(src/[\w/]+\.rs:\d+)", why)
        if m:
            return "panic at " + m.group(1)
        if "out of memory" in why:
            return "out of memory under the cap"
        m = re.search(r"(signal \d+|exit \d+)", why)
        return m.group(1) if m else why[:50]
    if s == "no scene":
        if "outline" in why.lower() or "edge" in why.lower():
            return "no board outline"
        if "died" in why or "signal" in why:
            return "the exporter died"
        if "still going" in why:
            return "the exporter ran out of time"
        return re.sub(r"\s+", " ", why)[:60]
    if s == "scene refused":
        return re.sub(r"^astar: \S+: ", "", why)[:70]
    if s == "not prepared":
        return ""
    return re.sub(r"\s+", " ", why)[:70]


def cell(v):
    return str(v).replace("|", "\\|") if v is not None else "-"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--rows", default=os.path.join(HERE, "baseline.jsonl"))
    ap.add_argument("--out", default=os.path.join(HERE, "SCOREBOARD.md"))
    ap.add_argument("--note", default="", help="a line for the head of the page")
    a = ap.parse_args()
    rows = [json.loads(ln) for ln in open(a.rows) if ln.strip()]
    man = json.load(open(os.path.join(HERE, "manifest.json")))
    facts = {e["name"]: e for e in man["boards"]}
    listed = [e for e in man["boards"] if e["name"] not in {r["board"] for r in rows}]
    for e in listed:            # in the manifest and not in the run: said, not hidden
        rows.append({"board": e["name"], "source": e["source"], "layers": e.get("copper_layers"), "nets": e.get("nets"),
                     "status": "no scene" if not e.get("scene") else "not prepared", "why": e.get("scene_why") or "not run"})
    by_src = collections.defaultdict(list)
    for r in rows:
        by_src[r["source"]].append(r)
    srcs = [s for s in ORDER if s in by_src] + sorted(set(by_src) - set(ORDER))
    out = []
    w = out.append
    revs = collections.Counter(r.get("commit") for r in rows if r.get("commit"))
    budgets = sorted({(r["source"] == "stress", r.get("budget_s")) for r in rows if r.get("budget_s")})
    w("# Scoreboard: astar on the corpus\n")
    w(f"Made by `corpus/scoreboard.py` from `{os.path.relpath(a.rows, os.path.dirname(HERE))}`. The binary: "
      + ", ".join(f"`{k}`" + (f" ({v} boards)" if len(revs) > 1 else "") for k, v in revs.most_common())
      + ". Budget a board: " + ", ".join(f"{b:g} s{' (stress)' if st else ''}" for st, b in budgets)
      + f". KiCad: {man.get('kicad', '?')}.")
    w("The verdict is the house KiCad's: the plan laid on the bare board, zones refilled, DRC; a board passes when every net "
      "landed and no DRC error is new against the bare board's own. astar's own count is never the grade.")
    if a.note:
        w("\n" + a.note)
    ran = [r for r in rows if r["status"] not in ("no scene", "not prepared")]
    passed = [r for r in rows if r["status"] == "pass"]
    w(f"\n**{len(passed)} of {len(rows)} boards whole with 0 new DRC errors** ({len(ran)} routed and judged; "
      f"{sum(1 for r in rows if r['status'] == 'no scene')} gave no scene; {sum(1 for r in rows if r['status'] == 'not prepared')} not prepared yet).\n")

    w("## By source\n")
    w("| source | what | boards | routed | pass | nets open | DRC | crash, timeout, refused | no scene | not prepared | nets landed |")
    w("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    def line(name, what, rs):
        c = collections.Counter(r["status"] for r in rs)
        j = [r for r in rs if r.get("landed") is not None]
        landed, asked = sum(r["landed"] for r in j), sum(r["nets"] for r in j)
        return (f"| {name} | {what} | {len(rs)} | {sum(1 for r in rs if r['status'] not in ('no scene', 'not prepared'))} | {c['pass']} | "
                f"{c['nets open'] + c['nets open, DRC']} | {c['DRC'] + c['nets open, DRC']} | {c['crash'] + c['timeout'] + c['scene refused'] + c['judge failed']} | "
                f"{c['no scene']} | {c['not prepared']} | {landed} of {asked}{f' ({100 * landed / asked:.0f}%)' if asked else ''} |")
    for s in srcs:
        w(line(s, WHAT.get(s, ""), by_src[s]))
    w(line("**all**", "", rows))
    w("\n`nets open` and `DRC` each count the boards that have both. `nets landed`: over the boards KiCad judged; where KiCad's list of "
      "unconnected items is cut at 499 the count is an upper bound.\n")

    w("## By failure class\n")
    w("| class | boards | what it means | e.g. |")
    w("|---|---:|---|---|")
    c = collections.Counter(r["status"] for r in rows)
    for k, what in CLASSES:
        if c[k]:
            eg = [r["board"] for r in rows if r["status"] == k][:3]
            w(f"| {k} | {c[k]} | {what} | {', '.join(eg)} |")
    for k in sorted(set(c) - {k for k, _ in CLASSES}):
        w(f"| {k} | {c[k]} | | |")
    opened = [r for r in rows if r["status"] in ("nets open", "nets open, DRC") and r.get("nets")]
    if opened:
        frac = lambda r: r["landed"] / r["nets"]
        b = [sum(1 for r in opened if lo <= frac(r) < hi) for lo, hi in ((0.95, 1.01), (0.8, 0.95), (0.5, 0.8), (0, 0.5))]
        w(f"\nOf the {len(opened)} boards with nets open: {b[0]} land 95% or more of their nets, {b[1]} land 80 to 95%, {b[2]} land 50 to 80%, {b[3]} land under half.")
        false = [r["board"] for r in opened if "it called joined are not" in (r.get("why") or "")]
        w(f"Boards with a net astar called joined that KiCad finds open: {len(false)}" + (f" ({', '.join(false[:8])})." if false else "."))
    kinds = collections.Counter()
    for r in rows:
        for k in (r.get("drc_new") or {}):
            kinds[k] += 1
    if kinds:
        w("New DRC errors, by kind, as the number of boards that have them: " + ", ".join(f"{k} {v}" for k, v in kinds.most_common()) + ".")
    crashes = collections.Counter(reason(r) for r in rows if r["status"] == "crash")
    if crashes:
        w("Crashes: " + "; ".join(f"{k} ({v})" for k, v in crashes.most_common()) + ".")
    noscene = collections.Counter(reason(r) for r in rows if r["status"] == "no scene")
    if noscene:
        w("No scene: " + "; ".join(f"{k} ({v})" for k, v in noscene.most_common(6)) + ".")

    w("\n## The other two judges\n")
    chk = [r for r in rows if isinstance(r.get("check"), dict) and "open" in r["check"]]
    if chk:
        agree = [r for r in chk if not r["check"].get("only_kicad") and not r["check"].get("only_check")]
        ok_only = [r["board"] for r in chk if r["check"].get("only_kicad")]
        ck_only = [r["board"] for r in chk if r["check"].get("only_check")]
        unsaid = [r["board"] for r in chk if r["check"].get("kinds", {}).get("open-unsaid")]
        rule = [r["board"] for r in chk if any(k in r["check"].get("kinds", {}) for k in ("short", "clearance", "hole", "hole-to-hole", "edge", "keepout", "outside"))]
        w(f"- **`astar check`** (astar's own exact check, from the scene and the plan alone) ran on {len(chk)} boards. It names the same open nets as KiCad on "
          f"{len(agree)}. KiCad finds nets open that it calls joined on {len(ok_only)}" + (f" ({', '.join(ok_only[:6])}{' ...' if len(ok_only) > 6 else ''})" if ok_only else "")
          + f"; it finds nets open that KiCad does not on {len(ck_only)}" + (f" ({', '.join(ck_only[:6])}{' ...' if len(ck_only) > 6 else ''})" if ck_only else "")
          + f". A net in pieces that the plan itself called joined: {len(unsaid)} boards" + (f" ({', '.join(unsaid[:6])})" if unsaid else "")
          + f". The plan's copper breaks the scene's own rules (clearance, holes, edge, keep-outs) on {len(rule)} boards.")
    kr = [r for r in rows if isinstance(r.get("krt"), dict)]
    if kr:
        conn = [r for r in kr if "open_nets" in r["krt"].get("connected", {})]
        agree = [r for r in conn if not r["krt"].get("only_kicad") and not r["krt"].get("only_krt")]
        k_only = [r["board"] for r in conn if r["krt"].get("only_kicad")]
        t_only = [r["board"] for r in conn if r["krt"].get("only_krt")]
        failed = [r["board"] for r in kr if "failed" in r["krt"].get("connected", {}) or "failed" in r["krt"]]
        drc = [r for r in kr if "drc_new" in r["krt"]]
        more = collections.Counter()
        for r in drc:
            for k in r["krt"]["drc_new"]:
                more[k] += 1
        both_clean = sum(1 for r in drc if not r["krt"]["drc_new_total"] and not r.get("drc_new_total"))
        krt_only = [r["board"] for r in drc if r["krt"]["drc_new_total"] and not r.get("drc_new_total")]
        kicad_only = [r["board"] for r in drc if not r["krt"]["drc_new_total"] and r.get("drc_new_total")]
        w(f"- **KRT's checkers** (`check_connected.py`, `check_drc.py` of this repo) ran on {len(kr)} boards. check_connected names the same open nets as KiCad on "
          f"{len(agree)} of {len(conn)}; KiCad alone finds nets open on {len(k_only)}" + (f" ({', '.join(k_only[:5])}{' ...' if len(k_only) > 5 else ''})" if k_only else "")
          + f", KRT alone on {len(t_only)}" + (f" ({', '.join(t_only[:5])}{' ...' if len(t_only) > 5 else ''})" if t_only else "")
          + (f"; it could not run on {len(failed)} ({', '.join(failed[:4])}{' ...' if len(failed) > 4 else ''})" if failed else "")
          + f". check_drc against the bare board ran on {len(drc)}: both it and KiCad find nothing new on {both_clean}; it alone finds new violations on {len(krt_only)}, "
          f"KiCad alone on {len(kicad_only)}. Its new violations by kind, as boards: " + (", ".join(f"{k} {v}" for k, v in more.most_common()) or "none")
          + ". It holds copper to its own fab floors (0.2 mm from a hole at least), which can ask more than the board's project does.")
    if not chk and not kr:
        w("Not asked in this run.")

    w("\n## Every board\n")
    w("`L` copper layers; `landed` of `nets` by KiCad (`<=`: KiCad's list was cut at 499 items); `DRC` new errors; `s` astar's own seconds.\n")
    for s in srcs:
        rs = by_src[s]
        w(f"### {s}: {WHAT.get(s, '')} ({sum(1 for r in rs if r['status'] == 'pass')} of {len(rs)} pass)\n")
        w("| board | L | nets | landed | DRC | s | verdict | reason |")
        w("|---|---:|---:|---:|---:|---:|---|---|")
        for r in rs:
            landed = None if r.get("landed") is None else f"{'<=' if r.get('landed_is_at_most') else ''}{r['landed']}"
            sec = None if r.get("seconds") is None else f"{r['seconds']:.1f}"
            name = r["board"][len("stress_"):] if s == "stress" else r["board"]
            w(f"| {cell(name)} | {cell(r.get('layers'))} | {cell(r.get('nets'))} | {cell(landed)} | {cell(r.get('drc_new_total'))} | {cell(sec)} | "
              f"{'**pass**' if r['status'] == 'pass' else r['status']} | {cell(reason(r))} |")
        w("")
    if any(r["source"] == "stress" for r in rows):
        w("The stress boards are third-party and not in this repository: `corpus/manifest.json` names each one's repository, commit and "
          "path, `corpus/fetch_stress.py` fetches them, `corpus/prepare.py --source stress` makes them bare and exports their scenes.")
    open(a.out, "w").write("\n".join(out) + "\n")
    print(f"scoreboard: {len(rows)} boards, {len(passed)} pass; {a.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
