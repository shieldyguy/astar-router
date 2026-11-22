#!/usr/bin/env python3
"""route_on.py STUB OUT.jsonl [BOARD...]: what `--route-on` does to the boards in corpus/boards.

Every signal net of every board, one at a time, is kept to the bottom layer (`--route-on NET=B.Cu --stub
STUB`) and the board routed. One row a net in OUT.jsonl: was it joined, which other nets opened that were
joined without the rule, how far from its pads' copper it ran on the other layers (`reach`), and how
much track it has off and on its layer. The default stub (README, "The layers a net is routed on") was
chosen from these rows; run it again when the search or the stub rule changes.

    python3 corpus/route_on.py 2.5 /tmp/rows.jsonl            every board
    python3 corpus/route_on.py 2.5 /tmp/rows.jsonl dock g474  these
"""
import json, math, os, subprocess, sys, tempfile, glob
HERE = os.path.dirname(os.path.abspath(__file__))
A = os.environ.get('ASTAR') or os.path.join(HERE, '..', 'target', 'release', 'astar')
ROOT = os.path.join(HERE, 'boards')
if len(sys.argv) < 3:
    sys.exit(__doc__)
stub, out = sys.argv[1], sys.argv[2]
boards = sys.argv[3:] or sorted(d for d in os.listdir(ROOT) if os.path.isdir(os.path.join(ROOT, d)))
tmp = tempfile.mkdtemp(prefix='ro-')
rows = open(out, 'w')

def seg_d(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay; l2 = dx * dx + dy * dy
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / l2)) if l2 > 0 else 0.0
    return math.hypot(ax + t * dx - px, ay + t * dy - py)

def pad_dist(q, x, y):
    """from a point to the pad's copper: 0 inside it"""
    sh = q['shape']
    if 'circle' in sh:
        return max(0.0, math.hypot(x - sh['circle'][0], y - sh['circle'][1]) - sh['circle'][2])
    if 'oval' in sh:
        ax, ay, bx, by, r = sh['oval']; return max(0.0, seg_d(x, y, ax, ay, bx, by) - r)
    pts, r = sh['poly'], sh.get('r', 0)
    inside = False; best = 1e9; j = len(pts) - 1
    for i in range(len(pts)):
        (xi, yi), (xj, yj) = pts[i], pts[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi: inside = not inside
        best = min(best, seg_d(x, y, xi, yi, xj, yj)); j = i
    return 0.0 if inside else max(0.0, best - r)

def run(board, extra):
    plan = os.path.join(tmp, 'plan.json')
    r = subprocess.run([A, 'route', board, '-o', os.path.join(tmp, 'o', 'x.kicad_pcb'), '--plan', plan] + extra, capture_output=True, text=True, timeout=300)
    if not os.path.exists(plan):
        return None, r.returncode
    p = json.load(open(plan)); os.remove(plan)
    return p, r.returncode

os.makedirs(os.path.join(tmp, 'o'), exist_ok=True)
for name in boards:
    pcbs = glob.glob(os.path.join(ROOT, name, '*.kicad_pcb'))
    if not pcbs: continue
    board = pcbs[0]
    scene = os.path.join(tmp, 'scene.json')
    subprocess.run([A, 'scene', board, '-o', scene], capture_output=True)
    if not os.path.exists(scene): continue
    s = json.load(open(scene)); os.remove(scene)
    layers = s['stack']['copper']; bottom = layers[-1]
    planes = {p['net'] for p in s['stack'].get('planes', [])} | {z['net'] for z in s.get('copper', {}).get('zones', []) if z.get('net')}
    pads = {}
    for p in s['pads']:
        if p.get('net'): pads.setdefault(p['net'], []).append(p)
    base, _ = run(board, [])
    if base is None: continue
    base_open = {x['net'] for x in base['left_out']}
    for n in s['nets']:
        nm = n['name']
        if n.get('role', 'signal') != 'signal' or nm in planes or nm in base_open or len(pads.get(nm, [])) < 2 or '*' in nm or ',' in nm or '=' in nm:
            continue
        front = [p for p in pads[nm] if not p.get('tht') and p.get('side', 'F') == 'F']
        if not front: continue
        p, code = run(board, ['--route-on', f'{nm}={bottom}', '--stub', stub])
        if p is None:
            rows.write(json.dumps({'board': name, 'net': nm, 'error': code}) + '\n'); continue
        opened = {x['net'] for x in p['left_out']}
        reach, off_mm, on_mm = 0.0, 0.0, 0.0
        for pn in p['nets']:
            if pn['name'] != nm: continue
            for t in pn['tracks']:
                pts = []
                for seg in t['path']:
                    v = seg.get('line') or seg.get('arc')
                    pts += [(v[i], v[i + 1]) for i in range(0, len(v), 2)]
                mm = sum(math.hypot(seg['line'][2] - seg['line'][0], seg['line'][3] - seg['line'][1]) for seg in t['path'] if 'line' in seg)
                if t['layer'] == bottom:
                    on_mm += mm; continue
                off_mm += mm
                own = [q for q in pads[nm] if q.get('tht') or (q.get('side', 'F') == 'F') == (t['layer'] == layers[0])]
                for (x, y) in pts:
                    reach = max(reach, min((pad_dist(q, x, y) for q in own), default=99))
        rows.write(json.dumps({'board': name, 'net': nm, 'joined': nm not in opened, 'others_opened': sorted(opened - base_open - {nm}), 'reach': round(reach, 3), 'off_mm': round(off_mm, 2), 'on_mm': round(on_mm, 2), 'front_pads': len(front), 'pads': len(pads[nm])}) + '\n')
        rows.flush()

rows.close()
done = [json.loads(l) for l in open(out)]
ok = [x for x in done if x.get('joined')]
far = sorted(x['reach'] for x in ok)
print(f"{len(done)} nets on {len({x['board'] for x in done})} boards: {len(ok)} joined, {sum(1 for x in done if x.get('others_opened'))} runs opened another net, longest reach {far[-1] if far else 0} mm, {sum(x['off_mm'] for x in ok):.0f} mm off the layer in all")
