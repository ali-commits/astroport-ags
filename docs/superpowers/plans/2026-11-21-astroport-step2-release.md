# AstroPort step 2: release package Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `scripts/build-release.sh` builds `astroport-{version}-linux-x86_64.tar.zst` and its `.sha256` from a tagged commit in a clean container, with the DLSS-enabled renderer built against NVIDIA's DLSS SDK fetched at a pinned commit, and the same staging script serves the `[install.source]` build.

**Architecture:** Five small bash scripts under `scripts/release/` plus the entry point. `fetch-dlss.sh` fetches the pinned SDK into `build/release/dlss-sdk` (ignored). `stage.sh` lays out the install folder from the build outputs (used by the release and by AGS source builds). `package.sh` checks the folder against the AGS package rules and writes the deterministic archive and checksum. `build-release.sh` does the preflight on the host (version, channel, tag, clean tree, crates present), fetches the SDK, exports `git archive HEAD`, and runs `inside.sh` in a container built from `Containerfile`; `inside.sh` reuses `scripts/build-engines.sh` after pre-seeding Kyty's CMake cache with `KYTY_NGX_SDK_DIR`. One fixture test script covers everything that can run without a full engine build.

**Tech Stack:** bash, git, GNU tar, zstd, binutils (`nm`, `strip`), podman or docker, Ubuntu 24.04 container image, cargo/rustup 1.100.0.

**Spec:** `~/Games/astrobot-pc/native/docs/superpowers/specs/2026-11-21-astroport-design.md` (sections 4 and 9 step 2). Package rules: AGS `docs/extensions/STANDARD.md` sections 3 and 4 (`~/projects/AGS/.claude/worktrees/extension-protocol-2/`). Manifest shape: AGS m1 interface sheet section 2.1 (install and channels unchanged from protocol 1; a release is installed only when its packaged manifest says `protocol = 2`). Jobs and generated files: AGS m2-m5 interface sheet (`docs/superpowers/plans/2026-11-21-m2-m5-interfaces.md`) sections 3.2, 3.3, 3.5, 3.6; written "m2-m5 §n" below. Reference packager: KytyPS5-AGS `tools/package.sh` (branch `feat/ags-protocol-2`).

## Global Constraints

- Never start the game, AnyPS5, AGS or any service (docker/podman daemon included) in any step. `build-release.sh` only checks that a container engine answers and fails otherwise.
- Every test command runs under `timeout` (values given per step). Tests use fixtures in a `mktemp -d` folder that the test removes; nothing is written outside it and the repository's `build/`.
- Never modify game dumps. The opt-in read of `~/Games/astrobot-pc/native/src-1019` is not needed by this step and is not used.
- Never add game files, firmware `.prx`, keys, `ngx-sdk`, or any DLSS SDK file to the repository. The SDK lives only in `build/release/dlss-sdk` (ignored by `/build/`).
- No push, no tag push, no GitHub release and no upload without the owner's OK. This plan creates none. The full container build (Task 5) is run by the owner only.
- Implementers (Codex, in a worktree) cannot commit; each commit step is an exact git command the controller runs from the worktree root. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Only these files are created: `scripts/build-release.sh`, `scripts/release/{dlss.env,fetch-dlss.sh,stage.sh,package.sh,inside.sh,Containerfile,test.sh}`, `docs/release.md`. Nothing else is edited (see "Prelude and wrap-up needs").
- Bash scripts start with `#!/usr/bin/env bash` and `set -euo pipefail`, find the repository root from their own path (`readlink -f "${BASH_SOURCE[0]}"`, as `scripts/build-engines.sh` does), and report errors as one line `FAIL: <text>` on stderr with exit 1. No tracked file may contain the owner's home path (`scripts/check-repo.sh` rule).
- Fixed values: DLSS SDK `https://github.com/NVIDIA/DLSS`, tag `v310.9.16`, commit `9981c6350dc58c0ce21f024fd65e31e851c31689`. Version pattern `^[0-9]+\.[0-9]+\.[0-9]+(-testing\.[0-9]+)?$`; tag `v<version>`; stable `vX.Y.Z`, testing `vX.Y.Z-testing.N`. Asset `astroport-<version>-linux-x86_64.tar.zst`; checksum `<asset>.sha256` holding `<64 lowercase hex>  <asset>`. Architecture `x86_64` only. Manifest id `astroport` (not reserved: m2-m5 §3.6 reserves `steam`, `local`, `epic`, `extensions`), `protocol = 2`, `roles = ["launcher"]` (step 3's manifest, m1 §2.1).
- Archive: contents at the archive root (no top folder), `tar --sort=name --format=gnu --owner=0 --group=0 --numeric-owner --mode=u+rwX,go+rX,go-w --mtime=@$SOURCE_DATE_EPOCH` (default 0), `zstd -19 -T0`.

## Review Focus

1. A source build (no SDK, renderer without NGX): the stage must still be a valid package with no DLSS runtime and no NVIDIA licence. Pinned by `stage: no dlss` in Task 2.
2. The SDK is fetched but the renderer was built without NGX (the CMake cache variable was dropped, or the Linux link in `engines/kyty` is missing): the release must fail, never ship a renderer without DLSS. Pinned by `stage: require dlss fails` in Task 2.
3. An earlier SDK folder at another commit, or a moved upstream tag: fail without touching the folder, never build against an unpinned SDK. Pinned by `fetch: other commit` and `fetch: commit mismatch` in Task 1.
4. Build outputs that are symlinks (`build/stage/` is all links) or an already stripped renderer: the package holds regular files only, and a renderer with no symbol table is refused instead of being taken for "no DLSS". Pinned by `stage: links copied` and `stage: stripped input` in Task 2.
5. A stage whose manifest is not `protocol = 2`: AGS refuses to install it (m1 §2.1, `prepare_release`), so the package step must refuse it first. Pinned by `package: old protocol` in Task 3.
6. Re-packaging the same stage gives a different checksum, or executables lose their mode: the package must be byte-identical across runs and `bin/*` and `anyps5/relinker` stay executable after extraction. Pinned by `package: deterministic` and `package: modes` in Task 3.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `scripts/release/dlss.env` | The pinned SDK URL, tag, commit (`: "${VAR:=value}"` so tests can override). |
| `scripts/release/fetch-dlss.sh <dest>` | Sparse, pinned fetch of the SDK (headers, `lib/Linux_x86_64`, `LICENSE.txt`). |
| `scripts/release/stage.sh <stage-dir>` | Install-folder layout from build outputs; strips; DLSS runtime and licences. |
| `scripts/release/package.sh <stage-dir> <out-dir>` | Package rules check, deterministic `.tar.zst`, `.sha256`. |
| `scripts/release/inside.sh` | Container side: configure, `build-engines.sh`, cargo builds, stage, package. |
| `scripts/release/Containerfile` | Ubuntu 24.04 build image. |
| `scripts/build-release.sh [--check]` | Host side: preflight, SDK fetch, source export, container run, checksum check. |
| `scripts/release/test.sh [fetch\|stage\|package\|preflight]` | Fixture tests; no argument runs all groups. |
| `docs/release.md` | Channels, how to cut a release, source build, DLSS, release-notes template. |

`scripts/release/test.sh` prints `ok <case>` per passing case, `FAIL: <case>: <reason>` per failing one, and ends with `release tests OK: <n>` (exit 0) or exit 1. Task 1 creates it with the harness (`mktemp -d` fixture root removed by `trap ... EXIT`, helper `expect_fail <case> <message-substring> <cmd...>` that requires a nonzero exit and the substring on stderr) and the `fetch` group; later tasks add their group.

---

### Task 1: Pinned DLSS SDK fetch

**Files:**
- Create: `scripts/release/dlss.env`, `scripts/release/fetch-dlss.sh`, `scripts/release/test.sh`

**Interfaces:**
- Produces: `scripts/release/fetch-dlss.sh <dest>`; on success `<dest>` is a git checkout at `$DLSS_SDK_COMMIT` holding `include/nvsdk_ngx_vk.h`, `lib/Linux_x86_64/libnvsdk_ngx.a`, `lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.<n>`, `LICENSE.txt`; last stdout line `DLSS SDK <tag> ready at <dest>`. Env `DLSS_SDK_URL`, `DLSS_SDK_TAG`, `DLSS_SDK_COMMIT` override `dlss.env`.

- [ ] **Step 1: Write the `fetch` group in `scripts/release/test.sh`**

Fixture: a local repo `$T/dlss-origin` with `include/nvsdk_ngx_vk.h`, `lib/Linux_x86_64/libnvsdk_ngx.a`, `lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.310.9.16`, `LICENSE.txt`, `lib/Windows_x86_64/nvsdk_ngx_d.lib`, `doc/big.pdf`; committed with `git -c user.name=t -c user.email=t@example.invalid commit`, tagged `v1.0.0`, `git config uploadpack.allowFilter true`. Run with `DLSS_SDK_URL=file://$T/dlss-origin DLSS_SDK_TAG=v1.0.0 DLSS_SDK_COMMIT=<its HEAD>`.

Cases:
- `fetch: fresh` — exit 0; last line `DLSS SDK v1.0.0 ready at $T/sdk`; the four Linux files exist; `lib/Windows_x86_64` and `doc` do not (sparse).
- `fetch: reuse` — second run with `DLSS_SDK_URL=file:///nonexistent` still exits 0 with the same last line (no network when the folder is already at the commit).
- `fetch: commit mismatch` — fresh dest, `DLSS_SDK_COMMIT=0000000000000000000000000000000000000000`: `expect_fail` with `does not match the pinned commit`; dest does not exist afterwards and no `*.partial.*` sibling is left.
- `fetch: other commit` — dest is a git repo at another commit: `expect_fail` with `holds another DLSS SDK commit`; a marker file inside dest is still there.

- [ ] **Step 2: Run it to verify it fails**

Run: `timeout 120 scripts/release/test.sh fetch`
Expected: FAIL lines for the four `fetch:` cases (script missing), exit 1.

- [ ] **Step 3: Write `scripts/release/dlss.env` and `scripts/release/fetch-dlss.sh`**

`dlss.env`: `: "${DLSS_SDK_URL:=https://github.com/NVIDIA/DLSS}"`, `: "${DLSS_SDK_TAG:=v310.9.16}"`, `: "${DLSS_SDK_COMMIT:=9981c6350dc58c0ce21f024fd65e31e851c31689}"`.
`fetch-dlss.sh <dest>`: if `<dest>` exists: at the commit (`git -C <dest> rev-parse HEAD`) → print ready and exit 0; otherwise `FAIL: <dest> holds another DLSS SDK commit; remove it and run again` (never deletes it). Else clone into `<dest>.partial.$$` with `git clone --quiet --depth 1 --branch "$DLSS_SDK_TAG" --filter=blob:none --no-checkout`, `git sparse-checkout set --no-cone /include/ /lib/Linux_x86_64/ /LICENSE.txt`, `git checkout --quiet`; HEAD must equal `$DLSS_SDK_COMMIT` (else `FAIL: tag $DLSS_SDK_TAG is at <sha>, which does not match the pinned commit $DLSS_SDK_COMMIT`), the four files must exist (else `FAIL: DLSS SDK is missing <file>`), then `mv` to `<dest>`. A trap removes the `.partial` folder on any exit.

- [ ] **Step 4: Run it to verify it passes**

Run: `timeout 120 scripts/release/test.sh fetch && timeout 120 scripts/check-repo.sh`
Expected: four `ok fetch: ...` lines, `release tests OK: 4`, then `repo OK: ...`.

- [ ] **Step 5: Commit**

```bash
git add scripts/release/dlss.env scripts/release/fetch-dlss.sh scripts/release/test.sh
git commit -m "Release: fetch the pinned DLSS SDK into build/release" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Install-folder stage

**Files:**
- Create: `scripts/release/stage.sh`
- Modify: `scripts/release/test.sh` (add the `stage` group)

**Interfaces:**
- Consumes (paths relative to the repository root): `ags-extension.toml`, `assets/*`, `patches/*.json` (step 3); `adapter/target/release/astroport-adapter` (step 3); `runner/target/release/astroport-run` (step 4); `patcher/target/release/libastro_patcher.so`; from `scripts/build-engines.sh`: `build/kyty-gpu/libkyty_gpu.so`, `build/anyps5/core/relinker/relinker`, `build/anyps5/core/libs/libs/*.prx`, `build/shims/{libSceAgc,libSceAgcDriver,libSceVideoOut}.prx`; `engines/kyty/tools/u59-preset.json`, `engines/kyty/tools/u59-preset.PPSA21567.json`; `build/release/dlss-sdk/` (Task 1).
- Produces: `scripts/release/stage.sh <stage-dir>` (dir must not exist or be empty), env `ASTROPORT_REQUIRE_DLSS=1` to refuse a renderer without NGX. Layout in "Interfaces with other steps". Last stdout line `staged <stage-dir>: <n> system libraries, DLSS yes|no`.

- [ ] **Step 1: Write the `stage` group in `scripts/release/test.sh`**

Fixture root `$T/root`: a copy of `scripts/release/stage.sh` at `scripts/release/stage.sh`, every consumed file as a dummy (`assets/icon.png`, `patches/PPSA21567-1019.json`, two `.prx` system libraries plus `libAstroHybridHost.prx` in `build/anyps5/core/libs/libs/`, executables with mode 0755), and three ELF files built with `cc -g -shared` (`libkyty_gpu.so`, `libastro_patcher.so`) and `cc -g` (the adapter, runner): the "NGX" renderer from a C file defining `void NVSDK_NGX_VULKAN_Init_with_ProjectID(void){}`, the plain one without it. `engines/kyty/LICENSE`, `engines/kyty/LICENSES/Kyty-MIT.txt`, `engines/anyps5/LICENSE`, `engines/anyps5/3rdparty/zstd/COPYING`, `LICENSE`, `NOTICE.md`. SDK fixture `build/release/dlss-sdk/lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.310.9.16` and `build/release/dlss-sdk/LICENSE.txt`. `build/shims/libSceAgc.prx` is a symlink to a file elsewhere in the fixture.

Cases:
- `stage: layout` (plain renderer, no SDK folder) — exit 0; every path in the layout table exists; `anyps5/libs/` holds the two system `.prx` and not `libAstroHybridHost.prx`; `anyps5/libAstroHybridHost.prx` exists; last line `staged <dir>: 2 system libraries, DLSS no`.
- `stage: no dlss` — same run: no `lib/libnvidia-ngx-dlss.so.*`, no `licenses/NVIDIA-DLSS-SDK-LICENSE.txt`; `licenses/LICENSE`, `licenses/NOTICE.md`, `licenses/kyty/LICENSE`, `licenses/kyty/LICENSES/Kyty-MIT.txt`, `licenses/anyps5/LICENSE`, `licenses/anyps5/3rdparty/zstd/COPYING` exist.
- `stage: dlss` (NGX renderer, SDK present) — `lib/libnvidia-ngx-dlss.so.310.9.16` and `licenses/NVIDIA-DLSS-SDK-LICENSE.txt` exist; last line ends `DLSS yes`.
- `stage: require dlss fails` — plain renderer, `ASTROPORT_REQUIRE_DLSS=1`: `expect_fail` with `renderer was built without DLSS`.
- `stage: stripped input` — renderer pre-stripped with `strip --strip-all`: `expect_fail` with `has no symbol table`.
- `stage: stripped output` — `readelf -S lib/libkyty_gpu.so` and `bin/astroport-adapter` show no `.debug_info`.
- `stage: links copied` — `find <stage> -type l` is empty; `shims/libSceAgc.prx` is a regular file with the target's content.
- `stage: modes` — `bin/astroport-adapter`, `bin/astroport-run`, `anyps5/relinker` are 0755; `lib/libkyty_gpu.so`, `patches/*.json` are 0644.
- `stage: missing runner` — without `runner/target/release/astroport-run`: `expect_fail` with `missing runner/target/release/astroport-run (build the astroport-run crate in runner/, step 4)`.
- `stage: missing adapter` — likewise with `missing adapter/target/release/astroport-adapter (build the astroport-adapter crate in adapter/, step 3)`.
- `stage: not empty` — stage dir holding a file: `expect_fail` with `is not empty`.

- [ ] **Step 2: Run it to verify it fails**

Run: `timeout 120 scripts/release/test.sh stage`
Expected: FAIL lines for every `stage:` case, exit 1.

- [ ] **Step 3: Write `scripts/release/stage.sh`**

Copy every file with `install -D -m 0755|0644` (dereferences links, sets modes). Strip `bin/*`, `lib/libastro_patcher.so`, `lib/libkyty_gpu.so` with `strip --strip-unneeded` after copying. The DLSS decision is made on the unstripped source renderer: `nm` prints no symbols → `FAIL: build/kyty-gpu/libkyty_gpu.so has no symbol table; stage needs the unstripped build`; `nm` lists `NVSDK_NGX_VULKAN_Init_with_ProjectID` → DLSS yes: exactly one `build/release/dlss-sdk/lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.*` (else `FAIL: DLSS renderer but no single DLSS runtime in build/release/dlss-sdk (run scripts/release/fetch-dlss.sh build/release/dlss-sdk)`) to `lib/` and `LICENSE.txt` to `licenses/NVIDIA-DLSS-SDK-LICENSE.txt`; otherwise DLSS no, and with `ASTROPORT_REQUIRE_DLSS=1` `FAIL: the renderer was built without DLSS (KYTY_NGX_SDK_DIR did not reach engines/kyty)`. Licence files: `LICENSE`, `NOTICE.md`, each engine's `LICENSE` and `LICENSES/*`, and for each `engines/<e>/3rdparty/<name>/` the top-level files matching `licen[cs]e*`, `copying*`, `copyright*` (case-insensitive, as `check-repo.sh`) into `licenses/<e>/3rdparty/<name>/`. Missing input message: `FAIL: missing <relpath> (<hint>)`, hints: adapter/runner as in the tests; `ags-extension.toml`, `assets/`, `patches/settings.json`, `patches/versions.json` → `step 3 files`; `build/...` → `run scripts/build-engines.sh`; `patcher/...` → `cargo build --release --manifest-path patcher/Cargo.toml`. In the fixture the step-3 patch files are dummies created by the harness.

- [ ] **Step 4: Run it to verify it passes**

Run: `timeout 180 scripts/release/test.sh && timeout 120 scripts/check-repo.sh`
Expected: all `ok` lines, `release tests OK: 15`, then `repo OK: ...`.

- [ ] **Step 5: Commit**

```bash
git add scripts/release/stage.sh scripts/release/test.sh
git commit -m "Release: stage the install folder from the build outputs" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Package and checksum

**Files:**
- Create: `scripts/release/package.sh`
- Modify: `scripts/release/test.sh` (add the `package` group)

**Interfaces:**
- Consumes: a stage folder as Task 2 produces it.
- Produces: `scripts/release/package.sh <stage-dir> <out-dir>`; writes `<out-dir>/astroport-<version>-linux-x86_64.tar.zst` and `.sha256`, prints both absolute paths (two lines). Reads `SOURCE_DATE_EPOCH` (default `0`). Refuses to overwrite an existing asset in `<out-dir>`.

- [ ] **Step 1: Write the `package` group in `scripts/release/test.sh`**

Helper `make_stage <dir>` writes a minimal valid stage directly (no `stage.sh`): `ags-extension.toml` with `id = "astroport"`, `version = "0.1.0-testing.1"`, `protocol = 2`, `roles = ["launcher"]`, `adapter = "bin/astroport-adapter"`, `[assets]` `icon = "assets/icon.png"`, `logo = "assets/logo.png"`, `badge = "assets/badge.svg"`; and every required path of the layout table (dummies, executables 0755).

Cases:
- `package: ok` — exit 0; prints `<out>/astroport-0.1.0-testing.1-linux-x86_64.tar.zst` and `.sha256`; `(cd <out> && sha256sum -c <asset>.sha256)` passes; the `.sha256` first token matches `^[0-9a-f]{64}$`; `tar -I zstd -tf` lists `./ags-extension.toml` and no member starting with `/` or containing `..`.
- `package: deterministic` — packaging the same stage into a second out dir gives the same first token in both `.sha256` files.
- `package: modes` — after `tar -I zstd -xf` into a temp dir, `bin/astroport-adapter`, `bin/astroport-run`, `anyps5/relinker` are executable.
- `package: absolute link` — `ln -s /etc/hostname <stage>/lib/x`: `expect_fail` with `absolute link`.
- `package: escaping link` — `ln -s ../../outside <stage>/lib/x` (target exists outside): `expect_fail` with `escapes the package`.
- `package: fifo` — `mkfifo <stage>/x`: `expect_fail` with `special files`.
- `package: missing relinker` — `expect_fail` with `missing anyps5/relinker`.
- `package: no system libraries` — empty `anyps5/libs/`: `expect_fail` with `anyps5/libs holds no .prx`.
- `package: big asset` — `assets/icon.png` of 2 MiB + 1 byte: `expect_fail` with `larger than 2 MiB`.
- `package: asset link` — `assets/logo.png` a link to a file inside the stage: `expect_fail` with `is not a regular file`.
- `package: adapter not executable` — mode 0644: `expect_fail` with `is not an executable file`.
- `package: wrong id` — `id = "kytyps5"`: `expect_fail` with `manifest id is "kytyps5", not "astroport"`.
- `package: bad version` — `version = "1.0"`: `expect_fail` with `version "1.0" is neither X.Y.Z nor X.Y.Z-testing.N`.
- `package: old protocol` — `protocol = 1`: `expect_fail` with `manifest protocol is "1", not 2 (AGS installs protocol 2 releases only)`.
- `package: no overwrite` — packaging twice into the same out dir: `expect_fail` with `already exists`.

- [ ] **Step 2: Run it to verify it fails**

Run: `timeout 120 scripts/release/test.sh package`
Expected: FAIL lines for every `package:` case, exit 1.

- [ ] **Step 3: Write `scripts/release/package.sh`**

Port of KytyPS5-AGS `tools/package.sh` (read it on branch `feat/ags-protocol-2`, read-only) minus its CMake install, FSR and no-DLSS checks, plus: manifest `id`/`version`/`protocol`/`adapter` read with the same `sed` field reader (`protocol` must be `2`, m1 §2.1), the version pattern, `uname -m` must be `x86_64` (else `FAIL: unsupported architecture <arch>`), the required paths (`FAIL: missing <path>`): `ags-extension.toml`, `bin/astroport-adapter`, `bin/astroport-run`, `lib/libastro_patcher.so`, `lib/libkyty_gpu.so`, `anyps5/relinker`, `anyps5/libAstroHybridHost.prx`, the three `shims/*.prx`, the two `presets/*.json`, `patches/PPSA21567-1018.json`, `patches/PPSA21567-1019.json`, `patches/settings.json`, `patches/versions.json`, `licenses/LICENSE`, `licenses/NOTICE.md` (`make_stage` creates exactly these), at least one `anyps5/libs/*.prx`, and the no-overwrite check. Archive in a `mktemp -d` work folder, then `mv` both files into `<out-dir>`; the trap removes only that work folder.

- [ ] **Step 4: Run it to verify it passes**

Run: `timeout 240 scripts/release/test.sh && timeout 120 scripts/check-repo.sh`
Expected: all `ok` lines, `release tests OK: 30`, then `repo OK: ...`.

- [ ] **Step 5: Commit**

```bash
git add scripts/release/package.sh scripts/release/test.sh
git commit -m "Release: package the stage as a checked, reproducible tar.zst" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Release build entry point, container and docs

**Files:**
- Create: `scripts/build-release.sh`, `scripts/release/inside.sh`, `scripts/release/Containerfile`, `docs/release.md`
- Modify: `scripts/release/test.sh` (add the `preflight` group)

**Interfaces:**
- Consumes: Tasks 1-3 scripts; `scripts/build-engines.sh` (unchanged); `adapter/Cargo.toml`+`Cargo.lock` (crate `astroport-adapter`, step 3), `runner/Cargo.toml`+`Cargo.lock` (crate `astroport-run`, step 4), `patcher/Cargo.toml`.
- Produces: `scripts/build-release.sh [--check]`; env `ASTROPORT_CONTAINER` (`podman` or `docker`; default `podman` when on `PATH`, else `docker`). Output in `build/release/out/v<version>/`. `--check` stops after the preflight. First stdout line of both modes: `release v<version>: channel <stable|testing>, asset astroport-<version>-linux-x86_64.tar.zst, commit <40-hex>`.

- [ ] **Step 1: Write the `preflight` group in `scripts/release/test.sh`**

Fixture: a git repo `$T/repo` holding a copy of `scripts/build-release.sh`, `ags-extension.toml` (`version = "0.1.0-testing.1"`), `adapter/Cargo.toml`, `adapter/Cargo.lock`, `runner/Cargo.toml`, `runner/Cargo.lock` (dummy text), committed and tagged `v0.1.0-testing.1`. Every case runs `scripts/build-release.sh --check` in a fresh copy.

Cases:
- `preflight: testing` — exit 0; first line `release v0.1.0-testing.1: channel testing, asset astroport-0.1.0-testing.1-linux-x86_64.tar.zst, commit <HEAD>`.
- `preflight: stable` — version `0.2.0`, tag `v0.2.0`: line contains `channel stable`.
- `preflight: untagged` — tag removed: `expect_fail` with `HEAD is not tagged v0.1.0-testing.1`.
- `preflight: dirty` — an untracked file `x`: `expect_fail` with `uncommitted changes`.
- `preflight: no runner crate` — `runner/` removed and committed: `expect_fail` with `runner/Cargo.toml not found: the release needs the astroport-run crate (step 4)`.
- `preflight: no adapter lock` — `adapter/Cargo.lock` removed and committed: `expect_fail` with `adapter/Cargo.lock not found`.
- `preflight: bad version` — `version = "1.0"`: `expect_fail` with `is neither X.Y.Z nor X.Y.Z-testing.N`.
- `preflight: no manifest` — `ags-extension.toml` removed and committed: `expect_fail` with `ags-extension.toml not found (step 3)`.

- [ ] **Step 2: Run it to verify it fails**

Run: `timeout 120 scripts/release/test.sh preflight`
Expected: FAIL lines for every `preflight:` case, exit 1.

- [ ] **Step 3: Write `scripts/build-release.sh`**

Order: manifest, version pattern, channel (stable when no `-testing.N`), the four crate files, clean tree (`git status --porcelain` empty, else `FAIL: the working tree has uncommitted changes; a release is built from a commit`), tag (`git tag --points-at HEAD` contains `v<version>`, else `FAIL: HEAD is not tagged v<version>; tag it first (git tag v<version>), the release links that tag`), print the first line; `--check` exits 0 here. Then: container engine answers (`<engine> info`, else `FAIL: container engine <engine> is not reachable; start it yourself, this script never starts services`); out dir must not hold the asset; `scripts/release/fetch-dlss.sh build/release/dlss-sdk`; replace `build/release/src` with `git archive HEAD | tar -x -C build/release/src`; `<engine> build -t astroport-release -f scripts/release/Containerfile scripts/release`; `<engine> run --rm` as the host user (`--userns=keep-id` for podman, `--user $(id -u):$(id -g)` for docker), `-v build/release/src:/work/src`, `-v build/release/dlss-sdk:/work/src/build/release/dlss-sdk:ro`, `-v build/release/out/v<version>:/work/out`, `-e SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)`, `-w /work/src`, command `scripts/release/inside.sh`; finally `sha256sum -c` in the out dir and print the two file paths. Only `build/release/src` is ever deleted, and only under the repository's `build/release/`.

- [ ] **Step 4: Write `scripts/release/inside.sh` and `scripts/release/Containerfile`**

`inside.sh` (cwd `/work/src`), in order, stopping on the first failure:
```bash
cmake -S engines/kyty -B build/kyty -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_CXX_FLAGS=-fPIC -DKYTY_NGX_SDK_DIR=/work/src/build/release/dlss-sdk
scripts/build-engines.sh
cargo build --release --locked --manifest-path patcher/Cargo.toml
cargo build --release --locked --manifest-path adapter/Cargo.toml
cargo build --release --locked --manifest-path runner/Cargo.toml
ASTROPORT_REQUIRE_DLSS=1 scripts/release/stage.sh build/release/stage
scripts/release/package.sh build/release/stage /work/out
```
The first line seeds Kyty's CMake cache; its compiler and build-type flags must equal `scripts/build-engines.sh`'s Kyty line, or CMake drops the cache and DLSS with it (caught by `ASTROPORT_REQUIRE_DLSS=1`). `HOME` and `CARGO_HOME` point into `/tmp` inside the container.

`Containerfile`: `FROM docker.io/library/ubuntu:24.04`; apt packages: `build-essential clang lld cmake ninja-build pkg-config git ca-certificates python3 glslang-tools zstd binutils rustup libsdl2-dev libfreetype-dev libasound2-dev libdbus-1-dev libgl1-mesa-dev libpulse-dev libudev-dev libwayland-dev libx11-dev libxcursor-dev libxext-dev libxfixes-dev libxi-dev libxkbcommon-dev libxrandr-dev libxss-dev libxtst-dev wayland-protocols` (the KytyPS5-AGS release list plus gcc, cmake, python3, SDL2, FreeType for AnyPS5); `ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH`; `RUN rustup toolchain install 1.100.0 --profile minimal && rustup default 1.100.0`; `RUN chmod -R a+rX /opt/rustup /opt/cargo`. A package the clean build turns out to need is added here by the owner's run in Task 5, nowhere else.

- [ ] **Step 5: Write `docs/release.md`**

Sections, in this order:
1. **Channels**: stable `vX.Y.Z` (GitHub release), testing `vX.Y.Z-testing.N` (GitHub pre-release); the manifest blocks are in step 3's `ags-extension.toml` (copy the two `[[channels]]` blocks from "Interfaces with other steps").
2. **Cutting a release** (owner): set `version` in `ags-extension.toml`, commit, `git tag v<version>`, `scripts/build-release.sh --check`, `scripts/build-release.sh`; the package is in `build/release/out/v<version>/`. Publishing is the owner's alone: `git push origin v<version>` and `gh release create v<version> [--prerelease] --notes-file <notes> build/release/out/v<version>/astroport-<version>-linux-x86_64.tar.zst build/release/out/v<version>/astroport-<version>-linux-x86_64.tar.zst.sha256`.
3. **Before the first DLSS release** (owner checklist): read `build/release/dlss-sdk/LICENSE.txt` and confirm it allows redistributing `lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.310.9.16`; if it does not, the release is blocked until the owner decides (design section 4 ships it "where the SDK licence allows it").
4. **DLSS SDK**: fetched from `https://github.com/NVIDIA/DLSS` at tag `v310.9.16`, commit `9981c6350dc58c0ce21f024fd65e31e851c31689` (`scripts/release/dlss.env`), never committed; to update, change all three values together.
5. **Source build**: the `[install.source]` commands (no DLSS), and a DLSS source build by hand: `scripts/release/fetch-dlss.sh build/release/dlss-sdk`, then the `cmake` seed line from `inside.sh` with `-DKYTY_NGX_SDK_DIR=$PWD/build/release/dlss-sdk`, then the `[install.source]` build and stage commands.
6. **Release notes template**:
```markdown
# AstroPort <version>

Needs AGS with extension protocol 2 and extension jobs (AGS milestone 3 or later: the game files are built by an extension job and launched from `generated/`). Supported game versions: 1.019 (PPSA21567), and 1.018 (less tested).

- **Changes:** ...
- **Known issues:** ...
- **After installing, updating or rolling back:** build the game files again from the game page; AGS refuses to launch game files built before the install.
- **DLSS:** this package contains NVIDIA's DLSS runtime (SDK 310.9.16) under NVIDIA's DLSS SDK licence, which is not compatible with the GPL. DLSS works on NVIDIA GPUs only. A source build has no DLSS unless you fetch the SDK yourself.
- **Source:** <repository URL>/tree/v<version> (GPL-2.0-only). Licences are in `licenses/` inside the package.
- **Checksum:** astroport-<version>-linux-x86_64.tar.zst.sha256
```
For a testing version the title is `# AstroPort <version> (testing)`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `timeout 300 scripts/release/test.sh && timeout 120 scripts/check-repo.sh && bash -n scripts/build-release.sh scripts/release/inside.sh`
Expected: all `ok` lines, `release tests OK: 38`, `repo OK: ...`, no syntax errors.

- [ ] **Step 7: Commit**

```bash
git add scripts/build-release.sh scripts/release/inside.sh scripts/release/Containerfile scripts/release/test.sh docs/release.md
git commit -m "Release: build the package from a tag in a clean container, with DLSS" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Owner-run release build check (not for implementers)

Needs steps 3 and 4 merged (crates), step 5's Linux NGX link (below), network, a running container engine started by the owner, and the owner's OK. Implementers skip this task.

- [ ] **Step 1:** `git tag v0.1.0-testing.1` (local only) and `timeout 60 scripts/build-release.sh --check` — expected first line `release v0.1.0-testing.1: channel testing, ...`.
- [ ] **Step 2:** `timeout 14400 scripts/build-release.sh` — expected last lines the `.tar.zst` and `.sha256` paths; `sha256sum -c` OK. A missing container package is added to `scripts/release/Containerfile` and committed:

```bash
git add scripts/release/Containerfile
git commit -m "Release: container packages the clean build needs" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
- [ ] **Step 3:** `tar -I zstd -tf build/release/out/v0.1.0-testing.1/astroport-0.1.0-testing.1-linux-x86_64.tar.zst | grep -c '^./anyps5/libs/.*\.prx$'` — expected about 370; `grep -q libnvidia-ngx-dlss` on the same listing succeeds. Nothing is pushed or uploaded; the local tag is deleted (`git tag -d v0.1.0-testing.1`) unless the owner keeps it.

---

## Interfaces with other steps

**Install folder (release package root)**, produced by `scripts/release/stage.sh`, unchanged from the shared layout plus two additions:

```
ags-extension.toml
bin/astroport-adapter            0755
bin/astroport-run                0755
lib/libastro_patcher.so
lib/libkyty_gpu.so               stripped
lib/libnvidia-ngx-dlss.so.310.9.16   ADDED, DLSS packages only
anyps5/relinker                  0755
anyps5/libs/*.prx                system modules, without libAstroHybridHost.prx
anyps5/libAstroHybridHost.prx
shims/libSceAgc.prx shims/libSceAgcDriver.prx shims/libSceVideoOut.prx
presets/u59-preset.json presets/u59-preset.PPSA21567.json
patches/*.json                   all of patches/, including settings.json and versions.json
assets/*                         all of assets/
licenses/**                      ADDED: LICENSE, NOTICE.md, kyty/, anyps5/ (engine and 3rdparty licences), NVIDIA-DLSS-SDK-LICENSE.txt (DLSS packages only)
```
Reasons: the DLSS runtime is what NGX loads at run time and the design ships it "where the SDK licence allows it"; `licenses/` because GPL-2.0 section 1 and the bundled MIT/BSD licences require the notices to travel with the binaries. Every file is regular (no links). Step 4 reads all paths relative to the install folder AGS gives it and must not rely on anything else in it.

What AGS does with the installed folder (m2-m5 sheet):
- The build job's `argv[0]` is the absolute path of `bin/astroport-run` in this folder: `job.plan` requires `argv[0]` absolute and a file inside the install folder (m2-m5 §3.2, `JobPlan::validate`). So `bin/astroport-run` must be in every package, release and source build (`stage.sh` fails without it, `package.sh` checks it again for the release).
- AGS runs it with only `PATH`, `HOME`, `LANG`, the plan's `env` (no `AGS_` key) and `AGS_JOB_PROGRESS_FD`, `AGS_JOB_ID` (m2-m5 §3.3): none of this step's env variables reaches it.
- After each completed job AGS hashes the executables under `<data>/generated/` into `<ext>/.generated.json`; an install, repair or rollback of the extension removes that file, and a launch whose `argv[0]` is under `<data>/generated/` (step 3's `<run>/AstroBot`) then fails with `generated-changed` until the build job runs again (m2-m5 §3.5, code `generated-changed` in §9). The release-notes line "After installing, updating or rolling back" says so.

**Builds consumed by name (release fails clearly if absent):**
- Step 3: crate `astroport-adapter` at `adapter/` (`Cargo.toml`, `Cargo.lock`), binary `adapter/target/release/astroport-adapter`.
- Step 4: crate `astroport-run` at `runner/` (`Cargo.toml`, `Cargo.lock`), binary `runner/target/release/astroport-run`. Step 2 does not call it.
- Both are built with `cargo build --release --locked --manifest-path <crate>/Cargo.toml` on Rust 1.100.0 in the container; their `Cargo.lock` must be committed.

**Handed to step 3 for `ags-extension.toml`** (STANDARD.md sections 2-4; unchanged in protocol 2 per the m1 sheet section 2.1). Step 3 owns the other keys, among them `id = "astroport"`, `protocol = 2` and `roles = ["launcher"]`; `package.sh` refuses any `protocol` but `2` because AGS's `prepare_release` refuses it and stops offering the tag (m1 §2.1):

```toml
version = "0.1.0"                 # X.Y.Z or X.Y.Z-testing.N; build-release.sh reads this line and needs tag v<version>
adapter = "bin/astroport-adapter"

[install.release]
asset = "astroport-{version}-linux-{arch}.tar.zst"
sha256_asset = "{asset}.sha256"

[install.source]
depends = ["cmake", "ninja", "clang", "clang++", "gcc", "g++", "python3", "git", "cargo", "nm", "strip"]
build = [
  ["scripts/build-engines.sh"],
  ["cargo", "build", "--release", "--locked", "--manifest-path", "patcher/Cargo.toml"],
  ["cargo", "build", "--release", "--locked", "--manifest-path", "adapter/Cargo.toml"],
  ["cargo", "build", "--release", "--locked", "--manifest-path", "runner/Cargo.toml"],
]
stage = [
  ["scripts/release/stage.sh", "{stage}"],
]

[[channels]]
id = "stable"
name = "Stable"
tag = '^v\d+\.\d+\.\d+$'
prerelease = false
default = true

[[channels]]
id = "testing"
name = "Testing"
tag = '^v\d+\.\d+\.\d+-testing\.\d+$'
prerelease = true
```
`version` must be a single line `version = "<value>"` (read by `sed`, as KytyPS5's packager does). `[assets]` paths are step 3's; `package.sh` checks whatever `icon`, `logo`, `badge` name. The source build has no DLSS (AGS gives source builds only `PATH`, `HOME`, `LANG` and no options). `stage.sh` writes `ags-extension.toml` into `{stage}`; AGS copies it there too, same content.

**Needed from step 5 (engines/kyty)**, without which `inside.sh` fails with `the renderer was built without DLSS` (planned as step 5 Task 7):
- NEEDS step 5: `engines/kyty/CMakeLists.txt` links `${KYTY_NGX_SDK_DIR}/lib/Linux_x86_64/libnvsdk_ngx.a` (plus `${CMAKE_DL_LIBS}`) when not on Windows; today it links only `lib/Windows_x86_64/nvsdk_ngx_d.lib`.
- NEEDS step 5: `hybridDlss.cpp` passes a writable Linux folder instead of the hard-coded `E:\AstroHybrid-Work\ngx-data`, and the folder holding `libkyty_gpu.so` in `NVSDK_NGX_FeatureCommonInfo` path list so NGX finds `libnvidia-ngx-dlss.so.*` next to it.

**Needed from step 4 (runner):** NEEDS step 4 (planned in step 4 Task 4 `install_libs`): copy `lib/libnvidia-ngx-dlss.so.*`, when present, into the run folder's `libs/` next to `libkyty_gpu.so` (copy-then-rename, as the renderer).

**Env variables** (build time only, none at run time): `ASTROPORT_CONTAINER`, `ASTROPORT_REQUIRE_DLSS`, `SOURCE_DATE_EPOCH`, `DLSS_SDK_URL`, `DLSS_SDK_TAG`, `DLSS_SDK_COMMIT`.

**Socket messages, settings.json, versions.json:** none from this step; `package.sh` only requires `patches/settings.json` and `patches/versions.json` to exist.

**Sheet items** (AGS m1 and m2-m5 interface sheets):
- Resolved: manifest keys `[install.release]`, `[install.source]`, `[[channels]]`, `[assets]` unchanged in protocol 2 (m1 §2.1); `protocol = 2` required in the packaged manifest, now checked by `package.sh` (m1 §2.1); id `astroport` not reserved (m2-m5 §3.6); job `argv[0]` `bin/astroport-run` inside the install folder and the job's environment (m2-m5 §3.2, §3.3); install, repair and rollback drop `.generated.json`, so built game files must be rebuilt (m2-m5 §3.5). Step 2 consumes no unified-settings (m4) row.
- SHEET: m2-m5 §3.5 names install, repair and rollback; that an update from a release channel counts as an install is read from the m1 installer, not written in the sheet.
- SHEET: m2-m5 §2.1 adds the standard row `perf.upscaler` (value `dlss`); whether step 3 declares it instead of, or beside, `graphics.dlss` is step 3's choice, not fixed by the sheet. This step only ships the runtime either way.

## Prelude and wrap-up needs

- `.gitignore`: nothing new (`/scripts/` and `/docs/` are whitelisted; `build/release/` falls under `/build/`).
- `scripts/check-repo.sh`, required tracked files: add `scripts/build-release.sh`, `scripts/release/dlss.env`, `scripts/release/fetch-dlss.sh`, `scripts/release/stage.sh`, `scripts/release/package.sh`, `scripts/release/inside.sh`, `scripts/release/Containerfile`, `scripts/release/test.sh`, `docs/release.md`.
- `scripts/check-repo.sh`, forbidden tracked paths: add `*nvsdk_ngx*` and `*libnvidia-ngx-*`; should-be-ignored probe: `build/release/dlss-sdk/x`.
- `README.md`: one line under the build section: releases are built with `scripts/build-release.sh`, see `docs/release.md`.
- `NOTICE.md` section 5: name the SDK version (310.9.16) and that packages carry `licenses/NVIDIA-DLSS-SDK-LICENSE.txt`.
- Controller: step 5 Task 7 holds the two engines/kyty NGX items and step 4 Task 4 the DLSS runtime copy; step 3's `graphics.dlss` (NVIDIA only) sets `ASTROHYBRID_DLSS`.
