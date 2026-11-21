"""Build libkyty_gpu.so from an existing Linux kyty_emulator build (the Linux counterpart of
build_kyty_gpu.py).

The emulator must be built with position-independent code (-DCMAKE_POSITION_INDEPENDENT_CODE=ON,
-fPIC). Takes the compile command of one emulator source and the final link command from
`ninja -t commands`, compiles src/hybrid/kytyGpuBridge.cpp with the same flags, and links the
emulator's objects (minus main.cpp.o) with it into <out>/libkyty_gpu.so. Only KytyGpuGetApi,
KytyGpuQueuePadAudio and the NID names in src/hybrid/kyty_gpu.def are exported (version script),
so the many libraries Kyty links statically (SDL3, ...) stay private to it: AnyPS5 loads the
library with RTLD_GLOBAL next to its own SDL2.

Usage: python3 -I tools/hybrid/build_kyty_gpu_linux.py <kyty build dir> <out dir>
"""

import pathlib
import re
import shlex
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]


def compile_command(build, source):
    """The emulator's compile command for `source` (compile_commands.json; configure with
    -DCMAKE_EXPORT_COMPILE_COMMANDS=ON)."""
    import json
    for entry in json.loads((build / "compile_commands.json").read_text()):
        if entry["file"].endswith(source) and "kyty_emulator.dir" in entry.get("output", entry["command"]):
            return shlex.split(entry["command"])
    sys.exit(f"compile_commands.json has no kyty_emulator entry for {source}")


def link_block(build):
    """Objects and variables of the kyty_emulator link (build.ninja; the .rsp file is temporary)."""
    text = (build / "build.ninja").read_text(encoding="utf-8", errors="replace")
    start = text.find("build kyty_emulator:")
    if start < 0:
        sys.exit("build.ninja has no 'build kyty_emulator:'")
    block = text[start:text.find("\n\n", start)]
    lines = block.splitlines()
    inputs = lines[0].split(":", 1)[1].split("|")[0].split()[1:]  # drop the rule name
    variables = {}
    for line in lines[1:]:
        match = re.match(r"\s+(\w+) = (.*)", line)
        if match:
            variables[match.group(1)] = match.group(2)
    objects = [i for i in inputs if i.endswith(".o")]
    return objects, variables


def exported_names():
    names = []
    for line in (ROOT / "src/hybrid/kyty_gpu.def").read_text().splitlines():
        line = line.strip()
        if not line or line in ("EXPORTS",) or line.startswith("LIBRARY"):
            continue
        match = re.match(r'"([^"]+)"=', line)
        names.append(match.group(1) if match else line.split("=")[0])
    return names


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    build = pathlib.Path(sys.argv[1]).resolve()
    out = pathlib.Path(sys.argv[2]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    compile_cmd = compile_command(build, "src/libs/agc.cpp")
    objects, link_vars = link_block(build)

    # The bridge, with the emulator's own compile flags.
    bridge_obj = out / "kytyGpuBridge.cpp.o"
    cmd = []
    skip = False
    for arg in compile_cmd:
        if skip:
            skip = False
            continue
        if arg in ("-o", "-MF", "-MT", "-c"):
            skip = True
            continue
        if arg in ("-MD", "-MMD", "-fPIE", "-fpie"):
            continue
        cmd.append(arg)
    cmd += ["-c", str(ROOT / "src/hybrid/kytyGpuBridge.cpp"), "-o", str(bridge_obj)]
    print("compile: src/hybrid/kytyGpuBridge.cpp", flush=True)
    subprocess.run(cmd, cwd=build, check=True)

    # Version script: only the bridge's names are global.
    script = out / "kyty_gpu.map"
    names = exported_names()
    script.write_text("{\n  global:\n" + "".join(f'    "{n}";\n' for n in names) +
                      "    kyty_gpu_nid_*;\n  local: *;\n};\n")  # thunks: the Linux .prx forwarders jump to them

    # The emulator's link, as a shared library without main.cpp.o. The -Map and dependency-file
    # options are the executable's own and are left out.
    objects = [o for o in objects if not o.endswith("/main.cpp.o")]
    flags = [f for f in shlex.split(link_vars.get("FLAGS", "")) if f not in ("-pie", "-fPIE")]
    raw = shlex.split(link_vars.get("LINK_FLAGS", ""))
    link_flags = []
    i = 0
    while i < len(raw):
        f = raw[i]
        if f == "-Xlinker" and i + 1 < len(raw) and raw[i + 1].startswith("--dependency-file"):
            i += 2  # "-Xlinker --dependency-file=..." pair
            continue
        if not f.startswith("-Wl,-Map=") and not f.startswith("--dependency-file"):
            link_flags.append(f)
        i += 1
    libraries = shlex.split(link_vars.get("LINK_LIBRARIES", ""))
    so = out / "libkyty_gpu.so"
    cmd = [compile_cmd[0]] + flags + link_flags + ["-shared", "-o", str(so), str(bridge_obj)] + objects + \
        libraries + [f"-Wl,--version-script={script}", "-Wl,-Bsymbolic", "-Wl,-soname,libkyty_gpu.so"]
    print(f"link: {len(names)} exports, {len(objects)} emulator objects, {len(libraries)} libraries",
          flush=True)
    subprocess.run(cmd, cwd=build, check=True)
    print(f"built {so}")


if __name__ == "__main__":
    main()
