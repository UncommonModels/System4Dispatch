#!/usr/bin/env python3
"""Convert an SCab2 `.nlay` track map into a System4Dispatch layout.

    python3 tools/scab2s4d.py ../SCab2/full.nlay "TMRC" > layout.json

SCab2 stores track as vector polylines. Every point is snapped to the
centre of a grid cell (CELL units per cell); each segment becomes the run of
cells it passes through, and each cell's tile shape is chosen from the set of
directions track leaves it in. Switch, block and label names carry over;
event IDs don't exist in SCab2, so they are left empty. The turntable becomes
a turntable with its roundhouse: stall tracks keep their real angles.

Track ends that SCab2 only labels as continuing elsewhere (the same name at
both ends of the map) become pairs of portals; see PORTALS.
"""
import ast
import collections
import itertools
import json
import math
import sys

CELL = 40
# Loose track ends that continue at another loose end, by SCab2 track
# number and end point: (portal name, (track, (x, y)), (track, (x, y))).
# TMRC's full.nlay draws the junction where the layout wraps round twice,
# once at each end of the map: switches S92, S93-1, S94 and S94-1 each
# have one half at the east end and the other at the west end. A track
# leaving the east drawing therefore carries on from the west stub that
# belongs to the same switch. Alvar Cutoff has no shared switch and is
# paired by the label SCab2 puts at both of its ends. Only used for maps
# that have these tracks.
PORTALS = [
    ("JP", (464, (100, -1190)), (477, (-7620, -1220))),    # S94-1
    ("S1", (471, (270, -1000)), (479, (-7660, -1220))),    # S93-1 / S94
    ("S2", (491, (270, -960)), (497, (-7760, -1240))),     # S92
    ("Alvar", (493, (190, -890)), (485, (-7920, -1010))),  # label
]
# Direction steps, 0 = N clockwise, as in s4d_core::layout (y grows down).
STEP = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)]


def load(path):
    recs = collections.defaultdict(list)
    for line in open(path):
        parts = line.rstrip("\n").split("||")
        if not parts[0]:
            continue
        d = {}
        for p in parts[1:]:
            k, v = p.split("|", 1)
            try:
                d[k] = ast.literal_eval(v)
            except (ValueError, SyntaxError):
                d[k] = v
        recs[parts[0]].append(d)
    return recs


def snap(c):
    return (math.floor(c[0] / CELL + 0.5), math.floor(c[1] / CELL + 0.5))


def dir_of(dx, dy):
    return STEP.index((dx, dy))


def trace(pieces, cell_tracks):
    """The cells a chain of polylines passes through, as 8-way neighbours.
    `pieces` is a list of (coords, track number); `cell_tracks` collects
    which tracks touch each cell.

    The line is sampled finely and each sample snapped to its cell; then
    staircases (a horizontal step next to a vertical one) are collapsed into
    single diagonal steps, which is how a 45° line is drawn on the grid.
    """
    cells = []
    for coords, track in pieces:
        for a, b in zip(coords, coords[1:]):
            n = max(1, int(math.dist(a, b) / 4))
            for i in range(n + 1):
                t = i / n
                c = snap((a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t))
                cell_tracks[c].add(track)
                if not cells or cells[-1] != c:
                    cells.append(c)
    changed = True
    while changed:
        changed = False
        out = []
        for c in cells:
            if out and out[-1] == c:
                continue
            # Doubling back (A, B, A): drop the excursion.
            if len(out) >= 2 and out[-2] == c:
                out.pop()
                changed = True
                continue
            # Staircase (A, B, C) with A and C diagonal neighbours: drop B.
            if len(out) >= 2:
                a = out[-2]
                if abs(a[0] - c[0]) == 1 and abs(a[1] - c[1]) == 1:
                    out.pop()
                    changed = True
            out.append(c)
        cells = out
    return cells


def shape_for(exits, reverse_dirs):
    """(shape, rot, is_turnout, swapped) for a set of exit directions."""
    e = sorted(exits)
    n = len(e)
    if n == 1:
        return "bumper", (e[0] - 6) % 8, False
    if n == 2:
        a, b = e
        diff = (b - a) % 8
        if diff == 4:
            return "straight", (a + 2) % 4, False
        if diff in (3, 5):
            # Curve: W (6) to NE (1), i.e. leg a, then a + 3.
            first = a if diff == 3 else b
            return "curve", (first - 6) % 8, False
        if diff in (2, 6):
            # Tight curve: W (6) to N (0).
            first = a if diff == 2 else b
            return "tight_curve", (first - 6) % 8, False
        return None
    if n == 3:
        for p in e:
            others = [d for d in e if d != p]
            rel = sorted((d - p) % 8 for d in others)
            if all(r in (3, 4, 5) for r in rel):
                rot = (p - 6) % 8
                rel = sorted((d - rot) % 8 for d in others)  # among 1, 2, 3
                rev = [(d - rot) % 8 for d in others if d in reverse_dirs]
                if rel == [1, 2]:
                    return "turnout_left", rot, True
                if rel == [2, 3]:
                    return "turnout_right", rot, True
                if rel == [1, 3]:
                    return "wye", rot, True
        return None
    if n == 4:
        # A 3-way switch: points at p, routes to p+3, p+4 and p+5.
        for p in e:
            if sorted((d - p) % 8 for d in e if d != p) == [3, 4, 5]:
                return "three_way", (p - 6) % 8, True
        pairs = [(a, b) for a in e for b in e if a < b and (b - a) == 4]
        if len(pairs) == 2:
            (a1, _), (a2, _) = pairs
            gap = (a2 - a1) % 8
            if gap in (2, 6):
                return "crossing", min(a1, a2) % 2, False
            # W-E with NW-SE is crossing_diag at rot 0 (legs 6-2, 7-3).
            for rot in range(8):
                legs = {frozenset({(6 + rot) % 8, (2 + rot) % 8}), frozenset({(7 + rot) % 8, (3 + rot) % 8})}
                if legs == {frozenset(pairs[0]), frozenset(pairs[1])}:
                    return "crossing_diag", rot, False
    return None


def convert(path, name):
    r = load(path)
    pts = {}
    for kind in ("PTrackPoint", "PTurnTablePoint", "PCenterPoint"):
        for p in r[kind]:
            pts[p["num"]] = p["coord"]
    tt_lines = {n for t in r["PTurnTable"] for group in t["lines"] for n in group}
    track_by_num = {t["num"]: t for t in r["PTrack"]}
    exits = collections.defaultdict(set)           # cell -> {dir}
    exit_tracks = collections.defaultdict(set)     # (cell, dir) -> {track num}
    cell_tracks = collections.defaultdict(set)     # cell -> {track num}

    def link(c, d, track):
        exits[c].add(d)
        exit_tracks[(c, d)].add(track)

    def lay(cells):
        for a, b in zip(cells, cells[1:]):
            d = dir_of(b[0] - a[0], b[1] - a[1])
            shared = cell_tracks[a] & cell_tracks[b] or cell_tracks[a] | cell_tracks[b]
            for track in shared:
                link(a, d, track)
                link(b, (d + 4) % 8, track)
            exits[a].add(d)
            exits[b].add((d + 4) % 8)

    # Turntable: centre cell, plus where its stall tracks point.
    tables = []
    for t, circle in zip(r["PTurnTable"], r["PTurnTableCircle"]):
        centre = pts[circle["center"]]
        tables.append((t, centre, circle["radius"]))

    def near_table(c):
        for _, centre, radius in tables:
            if math.dist(c, centre) <= radius + CELL / 2:
                return centre
        return None

    # Join tracks that meet end to end (and nowhere else) into chains, so a
    # crossover drawn as two half-tracks is traced as one diagonal.
    # Turntable lines: bridges (inside the pit) are dropped, lines whose
    # outer end meets other track are approaches (traced into the pit's
    # centre), and the rest are roundhouse stalls.
    track_ends = set()
    for t in r["PTrack"]:
        if t["num"] not in tt_lines:
            track_ends |= {tuple(pts[t["points"][0]]), tuple(pts[t["points"][-1]])}
    stall_lines = set()
    approach_lines = set()
    for n in tt_lines:
        t = track_by_num[n]
        coords = [pts[i] for i in t["points"]]
        centre = next(c for _, c, _ in tables)
        radius = next(rad for _, _, rad in tables)
        if all(math.dist(p, centre) <= radius + 1 for p in coords):
            continue
        if any(tuple(p) in track_ends for p in coords):
            approach_lines.add(n)
        else:
            stall_lines.add(n)
    pieces = {}
    for t in r["PTrack"]:
        if t["num"] in tt_lines and t["num"] not in approach_lines:
            continue
        coords = [tuple(pts[i]) for i in t["points"]]
        # Track that ends at a turntable runs on into the pit's centre.
        for i in (0, -1):
            centre = near_table(coords[i])
            if centre:
                coords[i] = tuple(centre)
        pieces[t["num"]] = coords
    ends = collections.defaultdict(list)
    for n, coords in pieces.items():
        ends[coords[0]].append(n)
        ends[coords[-1]].append(n)
    done = set()

    def extend(chain, at):
        """Grow `chain` (list of (coords, n)) forwards from point `at`."""
        while len(ends[at]) == 2:
            nxt = next((n for n in ends[at] if n not in done), None)
            if nxt is None:
                break
            done.add(nxt)
            c = pieces[nxt]
            if c[0] != at:
                c = c[::-1]
            chain.append((c, nxt))
            at = c[-1]
        return chain

    for n, coords in pieces.items():
        if n in done:
            continue
        done.add(n)
        fwd = extend([(coords, n)], coords[-1])
        back = extend([], coords[0])
        chain = [(c[::-1], m) for c, m in reversed(back)] + fwd
        lay(trace(chain, cell_tracks))

    elements = []
    turntables = []
    for i, (t, centre, radius) in enumerate(tables):
        c = snap(centre)
        tracks = []
        for n in sorted(stall_lines | approach_lines):
            track = track_by_num[n]
            far = max((pts[p] for p in track["points"]), key=lambda p: math.dist(p, centre))
            ang = math.degrees(math.atan2(far[0] - centre[0], -(far[1] - centre[1]))) % 360
            tracks.append((round(ang) % 360, n in stall_lines))
        exits.pop(c, None)
        # Track drawn inside the pit would only show through it.
        pit = radius / CELL
        for cell in list(exits):
            if 0 < math.dist(cell, c) < pit - 0.3:
                exits.pop(cell)
        turntables.append((t["name"], c, sorted(set(tracks)), max(1, round(2 * radius / CELL))))

    # Names: blocks and switches by the cells their tracks pass through.
    block_of_track = {}
    for b in r["PBlock"]:
        for n in b["lines"]:
            block_of_track[n] = b["name"]
    switches = r["PSwitch"] + r["PCrossing"]
    switch_of_track = {}
    reverse_tracks = set()
    for s in r["PSwitch"]:
        for n in s["lines0"] + s["lines1"]:
            switch_of_track[n] = s["name"]
        reverse_tracks |= set(s["lines1"])

    cols = [c[0] for c in exits]
    rows = [c[1] for c in exits]
    # Leave room for roundhouses, which reach beyond their tile.
    for _, c, _, size in turntables:
        reach = math.ceil(size / 2 + 1.6)
        cols += [c[0] - reach, c[0] + reach]
        rows += [c[1] - reach, c[1] + reach]
    labels = [(snap(t["coord"]), t["Text"]) for t in r["PTextPoint"]]
    x0 = min(cols + [l[0][0] for l in labels]) - 1
    y0 = min(rows + [l[0][1] for l in labels]) - 1
    x1 = max(cols) + 2
    y1 = max(rows + [l[0][1] for l in labels]) + 2

    used_turnouts = []
    track_nums = set(track_by_num)
    portal_cells = {}
    for pname, *ends in PORTALS:
        if all(n in track_nums for n, _ in ends):
            for _, xy in ends:
                portal_cells[snap(xy)] = pname
    problems = []
    taken = set()

    def block_for(c):
        names = collections.Counter(block_of_track[n] for n in cell_tracks[c] if n in block_of_track)
        return names.most_common(1)[0][0] if names else None

    # Vertical links between parallel tracks (crossovers drawn steeper than
    # 45°) can't be drawn; reroute each as a diagonal to the next cell.
    def drawable(c):
        return shape_for(exits[c], set()) is not None

    for a in sorted(exits, key=lambda c: (c[1], c[0])):
        b = (a[0], a[1] + 1)
        # Only between two east-west tracks, one above the other.
        if not ({2, 4, 6} <= exits[a] and {0, 2, 6} <= exits.get(b, set())):
            continue
        if drawable(a) and drawable(b):
            continue
        for dx, d_a, d_t in ((1, 3, 7), (-1, 5, 1)):
            t = (a[0] + dx, a[1] + 1)
            if t not in exits:
                continue
            old = (set(exits[a]), set(exits[b]), set(exits[t]))
            exits[a].discard(4)
            exits[a].add(d_a)
            exits[b].discard(0)
            exits[t].add(d_t)
            for d in (4,):
                for n in exit_tracks.pop((a, d), set()):
                    exit_tracks[(a, d_a)].add(n)
                    exit_tracks[(t, d_t)].add(n)
            if drawable(a) and drawable(t) and (not exits[b] or drawable(b)):
                break
            exits[a], exits[b], exits[t] = old

    def best_effort(c, rev_dirs):
        """The drawable shape that keeps the most of a cell's exits."""
        e = sorted(exits[c])
        subsets = [set(x) for k in (3, 2) for x in itertools.combinations(e, k)]
        # Prefer keeping a straight-through pair.
        subsets.sort(key=lambda x: -len(x) - any((a + 4) % 8 in x for a in x) * 0.5)
        for sub in subsets:
            got = shape_for(sub, rev_dirs & sub)
            if got:
                return got
        return shape_for({e[0]}, set())

    for c in sorted(exits, key=lambda c: (c[1], c[0])):
        if not exits[c]:
            continue
        rev_dirs = {d for d in exits[c] if exit_tracks[(c, d)] & reverse_tracks}
        got = shape_for(exits[c], rev_dirs)
        if got is None:
            problems.append(f"cell {c[0] - x0},{c[1] - y0}: can't draw exits {sorted(exits[c])}")
            got = best_effort(c, rev_dirs)
        shape, rot, is_turnout = got
        el = {"type": "track", "x": c[0] - x0, "y": c[1] - y0, "shape": shape, "rot": rot}
        if shape == "bumper" and c in portal_cells:
            el["shape"] = "portal"
            el["portal"] = portal_cells[c]
        b = block_for(c)
        if b:
            el["block"] = b
        if shape in ("crossing", "crossing_diag"):
            # Each track through a crossing keeps its own block.
            def exit_block(d):
                names = collections.Counter(
                    block_of_track[n] for n in exit_tracks[(c, d % 8)] if n in block_of_track)
                return names.most_common(1)[0][0] if names else None
            first = exit_block(6 + rot)
            second = exit_block((0 if shape == "crossing" else 7) + rot)
            if first:
                el["block"] = first
            if second and second != first:
                el["block2"] = second
        if is_turnout:
            names = collections.Counter(switch_of_track[n] for n in cell_tracks[c] if n in switch_of_track)
            tid = names.most_common(1)[0][0] if names else None
            base = tid or "T"
            tid = base
            k = 2
            while tid in used_turnouts:
                tid = f"{base}-{k}"
                k += 1
            used_turnouts.append(tid)
            el["turnout"] = tid
            if shape == "three_way":
                # The second set of points: the next switch name, or a new one.
                others = [n for n, _ in names.most_common() if n not in used_turnouts]
                t2 = others[0] if others else f"{tid}R"
                while t2 in used_turnouts:
                    t2 += "R"
                used_turnouts.append(t2)
                el["turnout2"] = t2
        elements.append(el)
        taken.add(c)

    tts = []
    for tt_name, c, tracks, size in turntables:
        tid = "TT1" if len(tts) == 0 else f"TT{len(tts) + 1}"
        el = {"type": "track", "x": c[0] - x0, "y": c[1] - y0, "shape": "turntable", "rot": 0, "turntable": tid}
        b = block_for(c)
        if b:
            el["block"] = b
        elements.append(el)
        taken.add(c)
        out = []
        leads = [a for a, stall in tracks if not stall]
        # Number stalls clockwise from the last lead, like the roundhouse doors.
        start = max(leads) if leads else 0
        stall_no = {a: i + 1 for i, a in enumerate(
            sorted((a for a, stall in tracks if stall), key=lambda a: (a - start) % 360))}
        for angle, stall in tracks:
            if stall:
                out.append({"angle": angle, "name": f"Stall {stall_no[angle]}", "stall": True})
            else:
                n = leads.index(angle) + 1
                out.append({"angle": angle, "name": "Lead" if len(leads) == 1 else f"Lead {n}"})
        tts.append({"id": tid, "name": "Turntable" if tt_name == "TT" else tt_name, "size": size, "tracks": out})

    # Labels go on the nearest free cell (scanning right, then nearby rows).
    for (lx, ly), text in labels:
        for dy, dx in sorted(((dy, dx) for dy in range(-2, 3) for dx in range(-4, 5)),
                             key=lambda v: (abs(v[0]) * 3 + abs(v[1]))):
            c = (lx + dx, ly + dy)
            if c not in taken:
                elements.append({"type": "label", "x": c[0] - x0, "y": c[1] - y0, "text": text})
                taken.add(c)
                break
        else:
            problems.append(f"no room for label {text!r}")

    block_names = sorted({e["block"] for e in elements if "block" in e},
                         key=lambda s: (len(s), s))
    layout = {
        "version": 1,
        "name": name,
        "grid": {"cols": x1 - x0, "rows": y1 - y0},
        "blocks": [{"id": b} for b in block_names],
        "turnouts": [{"id": t} for t in sorted(used_turnouts, key=lambda s: (len(s), s))],
        "turntables": tts,
        "elements": elements,
    }
    return layout, problems


if __name__ == "__main__":
    layout, problems = convert(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else "SCab layout")
    for p in problems:
        print("warning:", p, file=sys.stderr)
    print(json.dumps(layout, indent=2))
