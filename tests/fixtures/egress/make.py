#!/usr/bin/env python3
"""make: the hand-countable egress fixtures (T4.2 to T4.6), as scene 1.1 JSON, each with its expected counts.

    python3 tests/fixtures/egress/make.py      writes NAME.json and NAME.expect.json beside this file

Every expected number is counted by hand below, from the rules: n tracks of width w at clearance c need
n*w + (n+1)*c of room, so a gap of g mm holds floor((g - c) / (w + c)) tracks. On the grid a track's centre
stands on a node; the field lets it stand where the nearest copper is at least sqrt((w/2 + c)^2 + pitch^2/2)
away (the half width, the clearance, and the sag of a diagonal step), counted in whole hundredths of a mm.
tests/egress.rs holds the router to these files.
"""
import json, os

HERE = os.path.dirname(os.path.abspath(__file__))


def rect(x, y, w, h):
    return {"poly": [[r(x - w / 2), r(y - h / 2)], [r(x + w / 2), r(y - h / 2)], [r(x + w / 2), r(y + h / 2)], [r(x - w / 2), r(y + h / 2)]], "r": 0}


def r(v):
    return round(v, 4)


def scene(name, w, h, rules, layers=("F.Cu", "B.Cu")):
    out = [{"line": [0, 0, w, 0]}, {"line": [w, 0, w, h]}, {"line": [w, h, 0, h]}, {"line": [0, h, 0, 0]}]
    return {"bench": {"contract": "1.1", "board": name}, "stack": {"copper": list(layers), "planes": []}, "version": 1, "units": "mm",
            "outline": out, "rules": rules, "nets": [], "pads": [], "holes": [], "keepouts": [], "copper": [], "parts": []}


def pad(sc, ref, num, net, x, y, fx, fy, w, h, shape=None, tht=False):
    if net and not any(n["name"] == net for n in sc["nets"]):
        sc["nets"].append({"name": net, "class": "signal"})
    sc["pads"].append({"ref": ref, "num": str(num), "net": net, "at": [r(x), r(y)], "fp": [r(fx), r(fy)], "size": [w, h], "tht": tht,
                       "shape": shape or rect(x, y, w, h)})


def write(name, sc, expect):
    json.dump(sc, open(os.path.join(HERE, name + ".json"), "w"), indent=1)
    json.dump(expect, open(os.path.join(HERE, name + ".expect.json"), "w"), indent=1)
    print("wrote %s: %d pads, %d nets" % (name, len(sc["pads"]), len(sc["nets"])))


RULES = {"track": 0.15, "clearance": 0.15, "via": 0.6, "drill": 0.3, "edge": 0.3, "hole_clearance": 0.2}


def rows():
    """Six rows of six pads, each pad 1.2 mm long, facing down (south); every pad's net ends on a round test
    pad far below. Rules: track 0.15, clearance 0.15, so a track needs 0.15 + 2 * 0.15 = 0.45 of gap, two
    need 0.75. On the 0.1 mm grid a node is legal 0.24 mm or more from copper (sqrt(0.225^2 + 0.005) = 0.2358,
    counted in hundredths).

    pitch  pad    gap   by the rules             on the grid
    0.40   0.20   0.20  0                        0
    0.50   0.25   0.25  0                        0
    0.65   0.35   0.30  0  (0.45 needed)         0
    0.80   0.30   0.50  1  (0.35 / 0.30 = 1.17)  1  the middle node, 0.25 from both
    1.00   0.40   0.60  1  (0.45 / 0.30 = 1.5)   1  the middle node, 0.30 from both; its neighbours 0.20
    1.27   0.50   0.77  2  (0.62 / 0.30 = 2.07)  1  legal nodes span 0.29 mm: two tracks need 0.30 apart
    """
    sc = scene("rows", 44, 30, RULES)
    expect = {"layer": "F.Cu", "pitch": 0.1, "gaps": [], "gap_count": 0}
    table = [("P040", 0.4, 0.2, 0.2, 0, 0), ("P050", 0.5, 0.25, 0.25, 0, 0), ("P065", 0.65, 0.35, 0.3, 0, 0), ("P080", 0.8, 0.3, 0.5, 1, 1),
             ("P100", 1.0, 0.4, 0.6, 1, 1), ("P127", 1.27, 0.5, 0.77, 2, 1)]
    k = 0
    for i, (ref, pitch, w, gap, tracks, lanes) in enumerate(table):
        x0, y = 3.0 + (i % 3) * 14.0, 5.0 + (i // 3) * 10.0
        for n in range(6):
            x = x0 + n * pitch
            net = "N%d" % k
            pad(sc, ref, n + 1, net, x, y, x0 + 2.5 * pitch, y - 2.0, w, 1.2)
            tx = 3.0 + (k % 12) * 3.2
            ty = 22.0 if k < 12 else (25.5 if k < 24 else 28.0)
            pad(sc, "TP%d" % k, 1, net, tx, ty, tx, ty, 1.0, 1.0, shape={"circle": [tx, ty, 0.5]})
            k += 1
            if n:
                expect["gaps"].append({"a": "%s.%d" % (ref, n), "b": "%s.%d" % (ref, n + 1), "width": gap, "tracks": tracks, "lanes": lanes})
    expect["gap_count"] = len(expect["gaps"])
    write("rows", sc, expect)


def bga():
    """A quarter of a ball array: 6 by 6 balls, 0.8 mm pitch, 0.4 mm balls, its north and west sides open,
    its east and south sides shut by a keep-out where the rest of the array would be. Rules: track 0.1,
    clearance 0.1, via 0.45.

    Between two balls side by side: 0.8 - 0.4 = 0.4 mm: (0.4 - 0.1) / 0.2 = 1.5, one track; on the grid the
    middle node (0.2 from both; a node is legal from sqrt(0.15^2 + 0.005) = 0.1658, so 0.17) and no other.
    Balls corner to corner do not face each other: the two other balls of the square stand as near.
    60 gaps: 6 rows of 5 and 6 columns of 5.
    Between four balls a pocket: a via (0.45) needs 0.225 + 0.1 = 0.325 from every ball: the pocket's middle
    is sqrt(0.32) - 0.2 = 0.366 from each, the next node (0.1 off) only 0.3. One via site a pocket, 25 pockets.

    Ways out on the front: the 11 balls of the north row and the west column leave straight; each of the 10
    gaps between them passes one more: 21. With no via (the back shut by a keep-out, bga_novia) the other 15
    of the 36 have no way out; with vias every one of the 25 pockets holds one, so all 36 leave.
    """
    rules = {"track": 0.1, "clearance": 0.1, "via": 0.45, "drill": 0.2, "edge": 0.3, "hole_clearance": 0.2}
    for name, novia in (("bga", False), ("bga_novia", True)):
        sc = scene(name, 40, 24, rules)
        x0, y0 = 8.0, 12.0
        k = 0
        expect = {"layer": "F.Cu", "pitch": 0.1, "gaps": [], "pockets": [], "part": "U1"}
        for j in range(6):
            for i in range(6):
                x, y = x0 + i * 0.8, y0 + j * 0.8
                net = "B%d" % k
                # the footprint's centre is the whole array's: south-east of this quarter
                pad(sc, "U1", "%s%d" % ("ABCDEF"[j], i + 1), net, x, y, x0 + 4.4, y0 + 4.4, 0.4, 0.4, shape={"circle": [x, y, 0.2]})
                tx, ty = 2.0 + (k % 12) * 3.1, 2.0 + (k // 12) * 3.1
                pad(sc, "TP%d" % k, 1, net, tx, ty, tx, ty, 1.0, 1.0, shape={"circle": [tx, ty, 0.5]})
                k += 1
                if i:
                    expect["gaps"].append({"a": "U1.%s%d" % ("ABCDEF"[j], i), "b": "U1.%s%d" % ("ABCDEF"[j], i + 1), "width": 0.4, "tracks": 1, "lanes": 1})
                if j:
                    expect["gaps"].append({"a": "U1.%s%d" % ("ABCDEF"[j - 1], i + 1), "b": "U1.%s%d" % ("ABCDEF"[j], i + 1), "width": 0.4, "tracks": 1, "lanes": 1})
                if i and j:
                    expect["pockets"].append({"at": [r(x - 0.4), r(y - 0.4)], "vias": 1})
        expect["no_gap"] = [["U1.A1", "U1.B2"], ["U1.A2", "U1.B1"], ["U1.C3", "U1.D4"]]
        expect["gap_count"] = 60
        # the rest of the array: nothing passes east or south of the quarter
        # (0.1 mm from the last balls: (0.1 + 0.1 - 0.1) / 0.2 = 0.5, no track)
        e, s = x0 + 5 * 0.8 + 0.3, y0 + 5 * 0.8 + 0.3
        sc["keepouts"].append({"layers": ["F.Cu"], "poly": [[e, y0 - 0.4], [e + 3, y0 - 0.4], [e + 3, s + 3], [x0 - 0.4, s + 3], [x0 - 0.4, s], [e, s]]})
        if novia:
            sc["keepouts"].append({"layers": ["B.Cu"], "poly": [[0.5, 0.5], [39.5, 0.5], [39.5, 23.5], [0.5, 23.5]]})
        # (with vias the search over all 36 balls is cut short: it must find a way for 35 at least)
        expect["demand"] = 36
        expect["assigned"] = 21 if novia else 36
        expect["assigned_at_least"] = 21 if novia else 35
        expect["short"] = 15 if novia else 0
        write(name, sc, expect)


def channel():
    """One track too many. J1: five pads in a row, 1.0 mm pitch, 0.5 by 1.5 mm, facing a wall (a keep-out)
    0.5 mm in front of their ends, a second wall close behind them, the back of the board shut (no vias).
    Rules as in rows: a track needs 0.45 of room. Between a pad's end and the wall in front: 0.5 mm from the
    copper, and a keep-out wants no clearance, only the track's own half width: (0.5 + 0.15 - 0.15) / 0.3 =
    1.67: one track. So the channel holds one track.

    Pad 1 leaves sideways to the west, pad 5 to the east. Pad 2 goes through the channel past pad 1, pad 4
    past pad 5. Pad 3 would be the second track past pad 1 or past pad 5: the channel holds one.
    Need 5, have 4, short by 1, on J1's south side.
    """
    sc = scene("channel", 30, 20, RULES)
    x0, y = 12.0, 8.0
    for n in range(5):
        x = x0 + n * 1.0
        net = "C%d" % n
        pad(sc, "J1", n + 1, net, x, y, x0 + 2.0, y - 2.0, 0.5, 1.5)
        tx = 4.0 + n * 5.5
        pad(sc, "TP%d" % n, 1, net, tx, 16.0, tx, 16.0, 1.0, 1.0, shape={"circle": [tx, 16.0, 0.5]})
    # the wall in front: 0.5 mm past the pads' south ends; the wall behind: 0.2 mm past their north ends
    ys, yn = y + 0.75 + 0.5, y - 0.75 - 0.2
    sc["keepouts"].append({"layers": ["F.Cu"], "poly": [[x0 - 0.8, ys], [x0 + 4.8, ys], [x0 + 4.8, ys + 1.0], [x0 - 0.8, ys + 1.0]]})
    sc["keepouts"].append({"layers": ["F.Cu"], "poly": [[x0 - 0.8, yn - 1.0], [x0 + 4.8, yn - 1.0], [x0 + 4.8, yn], [x0 - 0.8, yn]]})
    sc["keepouts"].append({"layers": ["B.Cu"], "poly": [[0.5, 0.5], [29.5, 0.5], [29.5, 19.5], [0.5, 19.5]]})
    expect = {"layer": "F.Cu", "pitch": 0.1, "part": "J1", "side": "S", "demand": 5, "assigned": 4, "short": 1, "short_pads": ["J1.3"],
              "gaps": [{"a": "J1.%d" % (n + 1), "b": "keepout", "width": 0.5, "tracks": 1, "lanes": 1} for n in range(5)]}
    write("channel", sc, expect)


if __name__ == "__main__":
    rows()
    bga()
    channel()
