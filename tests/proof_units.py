#!/usr/bin/env python3
"""proof_units: the proof's own tools held to what they were mended for (plan/status/T13.md, "the false successes").

    proof_units.py

No KiCad, under a second. One line a check; exit 0 when all hold, 1 when one does not.

    the judge      unique_uuids: on a board where two parts share uuids every uuid becomes its own, the first
                   holder keeps it, the text is otherwise the same
    the scenes     with_plain_layer_names: the copper layers lose their user names, nothing else changes;
                   pads_not_in_scene: a pad of the board the scene lacks is found;
                   planes_of: a zone is a plane only when it holds every pad of its net
    the corpus     every board of corpus/smoke.txt has a scene that lacks no pad of its board (the manifest's
                   pads_not_in_scene), and the boards whose scene does lack pads are counted
"""
import json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, "corpus"))
import judge as J
import prepare as P

FIX = os.path.join(HERE, "fixtures", "false_success", "back-pad-on-a-renamed-layer.kicad_pcb")


def main():
    bad = 0

    def say(ok, what, detail=""):
        nonlocal bad
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'} {what}" + (f": {detail}" if detail else ""))

    text = open(FIX).read()
    uu = lambda t: re.findall(r'\(uuid "([^"]+)"\)', t)
    before, after = uu(text), uu(J.unique_uuids(text))
    firsts = [u for k, u in enumerate(before) if u not in before[:k]]
    say(len(set(before)) < len(before) and len(set(after)) == len(after) == len(before)
        and [u for u in after if u in firsts] == firsts and re.sub(r'\(uuid "[^"]+"\)', "", J.unique_uuids(text)) == re.sub(r'\(uuid "[^"]+"\)', "", text)
        and J.unique_uuids(J.unique_uuids(text)) == J.unique_uuids(text),
        "the judge gives KiCad a board whose every uuid is its own", f"{len(before)} uuids, {len(set(before))} different before, {len(set(after))} after")

    plain = P.with_plain_layer_names(text)
    say('(2 "B.Cu" signal)' in plain and "bottom_copper" not in plain and '(25 "Edge.Cuts" user)' in plain
        and plain.replace('(0 "F.Cu" signal)', '(0 "F.Cu" signal "top_copper")').replace('(2 "B.Cu" signal)', '(2 "B.Cu" signal "bottom_copper")') == text
        and P.with_plain_layer_names(plain) == plain,
        "the exporter is given the copper layers under their own names", "top_copper, bottom_copper -> F.Cu, B.Cu")

    fps = P.footprints(text)
    pad = lambda ref, num, net, at: {"ref": ref, "num": num, "net": net, "at": at}
    whole = [pad("J1", "1", "SIG", [5, 5]), pad("J1", "2", "GND", [5, 5]), pad("R1", "1", "SIG", [14.25, 5]), pad("R1", "2", "GND", [15.75, 5])]
    lacking = [p for p in whole if (p["ref"], p["num"]) != ("J1", "1")]            # as the exporter gave it: the back pad a keep-out
    say(P.pads_not_in_scene(fps, {"pads": whole}) == [] and P.pads_not_in_scene(fps, {"pads": lacking}) == [("J1", "1", "SIG")],
        "a pad of the board that the scene lacks is found", "J1.1 [SIG] on bottom_copper")

    zones = P.zones_of(text, {"F.Cu": "F.Cu", "B.Cu": "B.Cu"})
    planes, rest = P.planes_of(zones, whole)
    over_all = [dict(z, rings=[[(0, 0), (20, 0), (20, 10), (0, 10)]]) for z in zones]
    say(len(zones) == 1 and planes == [] and rest == [{"net": "GND", "layer": "F.Cu"}]
        and P.planes_of(over_all, whole)[0] == [{"net": "GND", "layer": "F.Cu"}],
        "a zone is a plane only when it holds every pad of its net", "GND under R1 alone: not a plane; over the whole board: one")

    man = os.path.join(ROOT, "corpus", "manifest.json")
    smoke = os.path.join(ROOT, "corpus", "smoke.txt")
    if os.path.exists(man) and os.path.exists(smoke):
        rows = {r["name"]: r for r in json.load(open(man))["boards"]}
        names = [ln.strip() for ln in open(smoke) if ln.strip() and not ln.startswith("#")]
        short = [n for n in names if n not in rows or rows[n].get("pads_not_in_scene") != 0]
        lack = sorted(n for n, r in rows.items() if r.get("scene") and r.get("pads_not_in_scene"))
        say(not short, "every smoke board's scene has every pad of its board",
            (f"{len(short)} do not: {', '.join(short[:5])}" if short else f"{len(names)} boards")
            + (f"; in the whole corpus {len(lack)} scenes lack pads (the runner says so in their rows): {', '.join(lack[:4])}" if lack else ""))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
