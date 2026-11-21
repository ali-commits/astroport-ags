"""Disassemble cheat writes (objdump) and list the addresses their instructions refer to."""
import json, re, subprocess, sys, tempfile, os
def disasm(code, va):
    with tempfile.NamedTemporaryFile(delete=False) as t: t.write(code)
    out = subprocess.run(["objdump","-D","-b","binary","-mi386:x86-64",f"--adjust-vma={va:#x}",t.name],capture_output=True,text=True).stdout
    os.unlink(t.name); ins=[]
    for l in out.splitlines():
        mm = re.match(r"\s*([0-9a-f]+):\t((?:[0-9a-f]{2} )+)\s*\t?(.*)", l)
        if mm: ins.append((int(mm.group(1),16), bytes.fromhex(mm.group(2).replace(" ","")), mm.group(3).strip()))
    # merge continuation lines (long instructions are split by objdump)
    merged=[]
    for a,b,t in ins:
        if not t and merged: pa,pb,pt=merged[-1]; merged[-1]=(pa,pb+b,pt)
        else: merged.append((a,b,t))
    return merged
def refs(ins):
    """(insn addr, insn len, target, kind) for rel32 branches and rip-relative operands."""
    out=[]
    for a,b,t in ins:
        n=len(b)
        m=re.search(r"#\s*(0x[0-9a-f]+)", t)
        if "(%rip)" in t and m: out.append((a,n,int(m.group(1),16),"rip"))
        elif re.match(r"(call|jmp|j[a-z]+)\s", t):
            m2=re.match(r"\S+\s+(?:0x)?([0-9a-f]+)\b", t)
            if m2 and (b[0] in (0xe8,0xe9) or (b[0]==0x0f and 0x80<=b[1]<=0x8f)):
                out.append((a,n,int(m2.group(1),16),"rel32"))
    return out
if __name__=="__main__":
    d=json.load(open(sys.argv[1]))
    for m in d["mods"]:
        if not m["enabled"]: continue
        print("==",m["name"][:70])
        for w in m["memory"]:
            va=int(w["offset"],16); on=bytes.fromhex(w["on"])
            r=refs(disasm(on,va)); ext=[(hex(a),hex(t),k) for a,n,t,k in r if not (va<=t<va+len(on))]
            print("  ",hex(va),len(on),"refs:",ext[:12])
