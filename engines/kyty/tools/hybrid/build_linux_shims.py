"""Build the Linux forwarders for AstroHybrid: libSceAgc.prx, libSceAgcDriver.prx, libSceVideoOut.prx.

The Linux counterpart of make_run_hybrid.py's build_shim (port repository): each library exports the
NIDs src/hybrid/kytyGpuNids.inc lists for it, every one a single jump to libkyty_gpu.so's thunk
for that NID (kyty_gpu_nid_<index>), and libSceAgcDriver.prx also the two AnyPS5-internal HDR calls,
which go to libAstroHybridHost.prx. The game and AnyPS5's other libraries (libSceJobManager.prx,
libSceSystemService.prx) then reach Kyty's renderer instead of AnyPS5's own. Because the forwarders
link against libkyty_gpu.so, the dynamic loader loads it at startup.

Usage: python3 -I tools/hybrid/build_linux_shims.py <run libs dir> <out dir>
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SHIMS = ("libSceAgc.prx", "libSceAgcDriver.prx", "libSceVideoOut.prx")
HOST_FORWARDS = {
    "libSceAgcDriver.prx": {
        "doCCHPPfxkI": "AstroHybridHostHdr_nid_no_patch",
        "KMphJjTcNgk": "AstroHybridNoteReportedToneMapPeak_nid_no_patch",
    },
}


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    libs = pathlib.Path(sys.argv[1]).resolve()
    out = pathlib.Path(sys.argv[2]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    rows = re.findall(r'KYTY_GPU_NID\((\d+), "([^"]+)", "([^"]+)"',
                      (ROOT / "src/hybrid/kytyGpuNids.inc").read_text(encoding="utf-8"))
    for library in SHIMS:
        lines = ["\t.text"]
        count = 0
        for index, nid, owner in rows:
            if owner != library:
                continue
            lines += [f'\t.globl "{nid}"', f'\t.type "{nid}", @function', f'"{nid}":',
                      f"\tjmp kyty_gpu_nid_{index}@PLT", f'\t.size "{nid}", .-"{nid}"']
            count += 1
        for nid, target in HOST_FORWARDS.get(library, {}).items():
            lines += [f'\t.globl "{nid}"', f'\t.type "{nid}", @function', f'"{nid}":',
                      f"\tjmp {target}@PLT", f'\t.size "{nid}", .-"{nid}"']
            count += 1
        lines.append('\t.section .note.GNU-stack,"",@progbits')
        source = out / (library + ".s")
        source.write_text("\n".join(lines) + "\n", encoding="ascii")
        target = out / library
        cmd = ["clang", "-shared", "-nostdlib", "-o", str(target), str(source),
               str(libs / "libkyty_gpu.so")]
        if library in HOST_FORWARDS:
            cmd.append(str(libs / "libAstroHybridHost.prx"))
        cmd += [f"-Wl,-soname,{library}", "-Wl,-rpath,$ORIGIN", "-Wl,-z,now"]
        subprocess.run(cmd, check=True)
        print(f"{library}: {count} NIDs forwarded")


if __name__ == "__main__":
    main()
