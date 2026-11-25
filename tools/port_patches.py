"""Port a KytyPS5 cheat file (_Patches/<TITLE>.json) from one game build to another.

usage: port_patches.py <cheats.json> <old eboot.elf> <new eboot.elf> <out.json> [new version
                       [old AstroBot.registry.json new AstroBot.registry.json]]

Both ELF files are unwrapped eboots (engines/anyps5/tools/unwrap_self.py). Each write is moved to the address of the
same code in the new build, and every jump, call and rip-relative operand in its "on" bytes is pointed
at the new address of its target:
- code targets: located by matching byte windows around them (unique matches only);
- data targets: located through the game instructions that refer to them in the old build;
- Kyty code caves at 0x12000000+ stay; caves in the padding after the old text segment move past the
  new text end.
A write whose site or target cannot be found makes its whole mod "enabled": false, with a reason.
"""
import json, struct, sys
from collections import Counter
import numpy as np
sys.path.insert(0, __file__.rsplit("/", 1)[0])
import patchscan, patchdis

CAVE_LO, CAVE_HI = 0x12000000, 0x13000000


class Build:
    def __init__(self, path):
        self.segs = patchscan.load(path)
        self.text_base, self.text = self.segs[0]
        self.text_end = self.text_base + len(self.text)
        self.np = np.frombuffer(self.text, dtype=np.uint8)

    def read(self, va, n):
        return patchscan.read(self.segs, va, n)

    def find(self, pat, limit=2):
        hits = []
        for base, data in self.segs:
            i = data.find(pat)
            while i >= 0 and len(hits) < limit:
                hits.append(base + i)
                i = data.find(pat, i + 1)
        return hits

    def rip_refs(self, target, limit=8):
        """Instruction addresses (end of disp32) in text whose rip-relative disp32 reaches target."""
        t = self.np
        n = len(t) - 4
        disp = (t[:n].astype(np.int64) | (t[1:n + 1].astype(np.int64) << 8) |
                (t[2:n + 2].astype(np.int64) << 16) | (t[3:n + 3].astype(np.int64) << 24))
        disp = np.where(disp >= 1 << 31, disp - (1 << 32), disp)
        pos = np.arange(n, dtype=np.int64) + self.text_base
        out = []
        for imm in (0, 1, 4):  # disp32 followed by no / imm8 / imm32
            hit = np.nonzero(pos + 4 + imm + disp == target)[0]
            out += [(int(pos[i]), imm) for i in hit[:limit]]
        return out


def map_code(old, new, va, span=96, win=16):
    votes = Counter()
    for d in range(-span, span + 1, 4):
        pat = old.read(va + d, win)
        if pat is None or len(pat) < win or not any(pat):
            continue
        hits = new.find(pat)
        if len(hits) == 1:
            votes[hits[0] - d] += 1
    if not votes:
        return None
    best, n = votes.most_common(1)[0]
    total = sum(votes.values())
    return best if n >= 2 and n * 2 > total else None


def map_data(old, new, target, cache={}):
    if target in cache:
        return cache[target]
    votes = Counter()
    for end_disp, imm in old.rip_refs(target):
        disp_at = end_disp  # address of the disp32 field
        site = map_code(old, new, disp_at)
        if site is None:
            continue
        raw = new.read(site, 4)
        disp = struct.unpack("<i", raw)[0]
        votes[site + 4 + imm + disp] += 1
    res = None
    if votes:
        best, n = votes.most_common(1)[0]
        if n * 2 > sum(votes.values()):
            res = best
    cache[target] = res
    return res


def stub_map(old_registry, new_registry):
    """Import stub address in the old build -> the stub of the same function (NID) in the new build."""
    def by_nid(path):
        out = {}
        for e in json.load(open(path)):
            for site in e.get("callSites", []):
                out.setdefault((e["nid"], e["library"]), []).append(int(site, 16))
        return out
    old, new = by_nid(old_registry), by_nid(new_registry)
    m = {}
    for key, sites in old.items():
        if key in new and len(sites) == 1 and len(new[key]) == 1:
            m[sites[0]] = new[key][0]
    return m


class Porter:
    def __init__(self, old, new, stubs=None):
        self.old, self.new = old, new
        self.stubs = stubs or {}
        # caves in the old text padding move past the new text end (16-byte aligned)
        pad_lo = old.text_end
        self.pad = (pad_lo, (pad_lo + 0x3fff) & ~0x3fff)
        self.pad_delta = ((new.text_end + 15) & ~15) - ((pad_lo + 15) & ~15)

    def map_addr(self, va):
        if CAVE_LO <= va < CAVE_HI:
            return va
        if self.pad[0] <= va < self.pad[1]:
            return va + self.pad_delta
        if va in self.stubs:
            return self.stubs[va]
        if self.old.text_base <= va < self.old.text_end:
            return map_code(self.old, self.new, va)
        return map_data(self.old, self.new, va)

    def port_write(self, w):
        va = int(w["offset"], 16)
        on, off = bytes.fromhex(w["on"]), bytes.fromhex(w["off"])
        cave = not any(off)
        nva = self.map_addr(va) if not cave else (va if CAVE_LO <= va < CAVE_HI else va + self.pad_delta)
        if nva is None:
            return None, f"site {va:#x} not found in the new build"
        if cave:
            new_off = off
            if not (CAVE_LO <= nva < CAVE_HI):
                cur = self.new.read(nva, len(off))
                if cur is not None and any(cur):
                    return None, f"cave {nva:#x} is used by the new build"
        else:
            new_off = self.new.read(nva, len(off))
            if new_off is None or len(new_off) != len(off):
                return None, f"site {nva:#x} unreadable"
        out = bytearray(on)
        ins = patchdis.disasm(on, va)
        for a, n, t, kind in patchdis.refs(ins):
            if va <= t < va + len(on):
                continue  # inside the write: moves with it
            nt = self.map_addr(t)
            if nt is None:
                return None, f"target {t:#x} (from {a:#x}) not found in the new build"
            raw = on[a - va:a - va + n]
            old_disp = t - (a + n)
            # rip operand: the disp32 may be followed by an immediate
            idx = None
            for imm in (0, 1, 2, 4):
                cand = n - 4 - imm
                if cand >= 0 and struct.unpack("<i", raw[cand:cand + 4])[0] == t - (a + n):
                    idx = cand
                    break
            if idx is None:
                return None, f"cannot locate the displacement at {a:#x}"
            na = a - va + nva
            out[a - va + idx:a - va + idx + 4] = struct.pack("<i", nt - (na + n))
        # unchanged original bytes inside the write must be the new build's bytes where on == off
        if not cave:
            for i in range(len(on)):
                if on[i] == off[i] and out[i] == on[i] and not self._in_ref(ins, va, i):
                    out[i] = new_off[i]
        return {"offset": f"{nva:#x}", "off": new_off.hex(), "on": bytes(out).hex()}, None

    @staticmethod
    def _in_ref(ins, va, i):
        a_i = va + i
        for a, n, t, kind in patchdis.refs(ins):
            if a <= a_i < a + n:
                return True
        return False


def main():
    src, old_path, new_path, out_path = sys.argv[1:5]
    version = sys.argv[5] if len(sys.argv) > 5 else None
    d = json.load(open(src))
    stubs = stub_map(sys.argv[6], sys.argv[7]) if len(sys.argv) > 7 else {}
    p = Porter(Build(old_path), Build(new_path), stubs)
    print(f"{len(stubs)} import stubs mapped through the relinker registries")
    print(f"text end {p.old.text_end:#x} -> {p.new.text_end:#x}; padding caves move by {p.pad_delta:#x}")
    ported = 0
    for m in d["mods"]:
        writes, why = [], None
        for w in m["memory"]:
            nw, why = p.port_write(w)
            if nw is None:
                break
            writes.append(nw)
        if why is None:
            m["memory"] = writes
            ported += 1
            moved = sorted({int(w["offset"], 16) - int(o["offset"], 16) for w, o in zip(writes, d_old[m["name"]])})
            print(f"  ok    {m['name'][:70]}  (moved {', '.join(hex(x) for x in moved)})")
        else:
            m["port_failed"] = why
            if m["enabled"]:
                m["enabled"] = False
            print(f"  FAIL  {m['name'][:70]}: {why}")
    if version:
        d["version"] = version
        d["name"] = d.get("name", "") + f" (ported to {version})"
    d.pop("source_sha256", None)
    json.dump(d, open(out_path, "w"), indent=2)
    print(f"{ported} of {len(d['mods'])} mods ported -> {out_path}")


if __name__ == "__main__":
    d_old = {m["name"]: m["memory"] for m in json.load(open(sys.argv[1]))["mods"]}
    main()
