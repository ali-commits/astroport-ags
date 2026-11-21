"""Check where the cheat file's original ("off") bytes are in an unwrapped eboot ELF."""
import json, sys
from elftools.elf.elffile import ELFFile
def load(path):
    f = open(path, 'rb'); e = ELFFile(f); segs = []
    for s in e.iter_segments():
        if s['p_type'] == 'PT_LOAD' and s['p_filesz']:
            f.seek(s['p_offset']); segs.append((s['p_vaddr'], f.read(s['p_filesz'])))
    return segs
def read(segs, va, n):
    for base, data in segs:
        if base <= va < base + len(data): return data[va-base:va-base+n]
    return None
if __name__ == "__main__":
    segs = load(sys.argv[2]); d = json.load(open(sys.argv[1]))
    print("segments", [(hex(b), hex(len(x))) for b, x in segs])
    for m in d["mods"]:
        if not m["enabled"]: continue
        ok = 0
        for w in m["memory"]:
            off = bytes.fromhex(w["off"]); got = read(segs, int(w["offset"], 16), len(off))
            ok += got == off
        print(f"{ok}/{len(m['memory'])} match  {m['name'][:80]}")
