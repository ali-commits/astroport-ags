# AstroPort step 3: manifest and adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** AstroPort becomes a protocol 2 AGS extension: a manifest, and a Rust adapter `astroport-adapter` that finds the user's Astro Bot dump, checks its version, lists one game, describes the game page and the AstroPort settings, maps every setting to patches and variables through `patches/settings.json`, and answers `launch.plan` (and `job.plan build`, which only returns the runner command).

**Architecture:** One standalone Rust crate in `adapter/`, a synchronous loop that reads length-prefixed JSON frames on stdin and answers on stdout (one request at a time, as the KytyPS5 protocol 2 adapter does). Data lives in two reviewed JSON files (`patches/versions.json`, `patches/settings.json`); the code is small modules around them. Everything that depends on AGS milestones 3 to 5 sits in one module, `adapter/src/sheet.rs`; each spot cites its section of the milestone 2 to 5 interface sheet (`// sheet §n.m:`), and the few the sheet still leaves open are marked `// SHEET:`.

**Tech Stack:** Rust 2021 (as `patcher/Cargo.toml`), `serde`/`serde_json` (as the patcher), `sha2 = "0.10"`; dev: `toml = "0.8"`, `tempfile = "3"`.

**Spec:** `~/Games/astrobot-pc/native/docs/superpowers/specs/2026-11-21-astroport-design.md` (binding: sections 5.1, 5.3, 5.4, 6, 9 step 3). Wire: `~/projects/AGS/.claude/worktrees/m2-m5-plans/docs/superpowers/plans/2026-11-21-m1-interfaces.md` (sections 1.2, 1.3, 2.1 to 2.10, 2.12) and the protocol 2 design `~/projects/AGS/.claude/worktrees/extension-protocol-2/docs/superpowers/specs/2026-11-21-extension-protocol-2-design.md` (sections 5, 6, 7, 9). Jobs (M3), the `standard` map (M4) and live (M5) follow the milestone 2 to 5 interface sheet `~/projects/AGS/.claude/worktrees/m2-m5-plans/docs/superpowers/plans/2026-11-21-m2-m5-interfaces.md` (written "M2-5 sheet §n"; sections 1.1, 1.2, 1.4, 2.1 to 2.5, 3.1 to 3.5, 3.8, 3.9, 6.1 to 6.6); "sheet" alone in Global Constraints means the m1 sheet. Anything that depends on M3 or M4 consumes only the M2-5 sheet.

All commands run from the repository root (the worktree of `astroport-ags`).

## Global Constraints

- Never start the game, AnyPS5, AGS or any service in any step. Tests use fixtures. The one opt-in test (Task 2) may READ the owner's dump read-only and writes only into a temp folder.
- Every test command has a hard timeout: `timeout -k 10 600 cargo ...`. The test helper kills the adapter child on drop and fails a call after 10 s.
- Never modify game dumps. Never add game files, firmware `.prx`, keys or `ngx-sdk` to the repo. No push and no release without the owner's OK.
- No file in the repo may contain the owner's home path (`scripts/check-repo.sh` rule); fixtures use temp folders, docs use `~`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Implementers (Codex, in a worktree) do not commit; the controller runs each commit step exactly as written. The controller applies the `.gitignore` lines of "Prelude and wrap-up needs" before Task 1's commit (until then `git add adapter/` is ignored).
- File ownership: this plan creates or edits only `ags-extension.toml`, `adapter/**`, `patches/settings.json`, `patches/versions.json`, `assets/**`. Never `.gitignore`, `scripts/check-repo.sh`, `README.md`, `NOTICE.md`, `engines/*/UPSTREAM.md`, or another step's files.
- Standalone crate: `adapter/Cargo.toml` and `adapter/Cargo.lock`, no root workspace. `edition = "2021"`, `license = "GPL-2.0-only"`, deps written as in `patcher/Cargo.toml` (`serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`).
- Manifest id `astroport`, name `AstroPort`, licence `GPL-2.0-only`, `protocol = 2`, `roles = ["launcher"]` (design 1, protocol design 9.1).
- Title `PPSA21567`; supported versions `1.018` (`01.018.000`, version only) and `1.019` (`01.019.000`, eboot SHA-256 `e88e19a688289e1ea557a6100b9b7d89fb3041875a7aa8471b361bfedf59fa86`, hashed read-only from the owner's dump `~/Games/ps5/Games/PPSA21567-01.019/PPSA21567-app/eboot.bin`, the dump the current run folder links to).
- Unsupported text, exact: `Version 1.0xx is not supported. AstroPort runs 1.018 and 1.019.` (with the real version and the list from `versions.json`).
- Launch never sets `LD_PRELOAD` (it overrides AnyPS5's symbols) and never `LD_LIBRARY_PATH`. The patcher loads through `LD_AUDIT`.
- Every patch of a group is set explicitly (chosen ones in `ASTRO_PATCH_ENABLE`, the others in `ASTRO_PATCH_SKIP`); no launch relies on a patch file's `enabled` flag (design 6).
- Data folder `<data>` is the adapter's working directory (AGS starts it there). Run folders `<data>/generated/run-<version>/` (`run-1.019`), saves `<data>/saves/`. Install folder = the folder two levels above the adapter binary (`bin/astroport-adapter`).
- Item ids match `^[a-z0-9][a-z0-9._-]{0,63}$`, unique per description; section titles 1 to 40 characters, labels 1 to 80, help and info text at most 200 (sheet 1.3). A breach makes AGS refuse the whole page.
- Adapter error codes: `unknown-method`, `invalid-params`, `not-found`, `failed` (sheet 2.12); `message` is one user sentence.

## Review Focus

1. `dump_folder` set to the folder above the dump (`PPSA21567-01.019/` holding `PPSA21567-app/`): the dump is found (the folder itself, then its direct children). Test in Task 2.
2. A folder with a broken or foreign `param.json` (bad JSON, another `titleId`, no `contentVersion`): no game, the extension page says so, the adapter keeps running. Tests in Tasks 2 and 3.
3. AGS sends `display.fps` outside 30/60 (144, 0, absent, a string): 144 maps to 60, anything else below 60 or unreadable to 30, and `applied` reports what the game really gets. Test in Task 7.
4. The dump changes after a build (run folder built for 1.018, dump now 1.019, or the eboot hash changed): the game shows `installed: false` and the build button, and `launch.plan` refuses with "Build the game files first." instead of starting a stale run folder. Tests in Tasks 6 and 7.
5. Launch values the page no longer offers (an unknown toggle id in `advanced.patches`, a missing item): unknown ids are ignored, missing items take their defaults, never an error that blocks Play. Test in Task 7.

---

## File structure

| File | Responsibility |
| --- | --- |
| `ags-extension.toml` | protocol 2 manifest |
| `assets/icon.svg`, `assets/logo.svg`, `assets/badge.svg` | placeholder art, shapes only (the AGS SVG allowlist has no `<text>`); the owner replaces them |
| `patches/versions.json` | supported versions, hashes, patch file per version |
| `patches/settings.json` | patch ids, groups, fixed env, presets, advanced toggles |
| `adapter/Cargo.toml`, `adapter/Cargo.lock` | crate `astroport-adapter`, binary `astroport-adapter` |
| `adapter/src/main.rs` | `State`, the frame loop, `dispatch` |
| `adapter/src/rpc.rs` | frames, `RpcError` |
| `adapter/src/dump.rs` | find the dump, version detection, hash cache, build state |
| `adapter/src/values.rs` | machine values in `<data>/values.json` |
| `adapter/src/settings.rs` | load and check `settings.json`, select patches and env |
| `adapter/src/device.rs` | GPU, VRAM and screen probe; device defaults |
| `adapter/src/page.rs` | extension page, game page, item defaults |
| `adapter/src/launch.rs` | `launch.plan` |
| `adapter/src/sheet.rs` | `job.plan` (M3) and the `standard` support map (M4), per the M2-5 sheet; open points marked `SHEET:` |
| `adapter/tests/common/mod.rs` | fixture install, data and dump; adapter process helper |
| `adapter/tests/rpc.rs` | end-to-end calls |
| `adapter/tests/manifest.rs` | manifest checks |

---

### Task 1: Crate, frames, `hello`, manifest and assets

**Files:**
- Create: `adapter/Cargo.toml`, `adapter/src/main.rs`, `adapter/src/rpc.rs`, `adapter/tests/common/mod.rs`, `adapter/tests/rpc.rs`, `adapter/tests/manifest.rs`, `ags-extension.toml`, `assets/icon.svg`, `assets/logo.svg`, `assets/badge.svg`

**Interfaces:**
- Produces (rpc.rs):
  - `pub fn read_frame(r: &mut impl std::io::Read) -> std::io::Result<Option<serde_json::Value>>` — 4-byte big-endian length then UTF-8 JSON; `Ok(None)` on clean EOF; a length over `MAX_FRAME = 1 << 20` is an `InvalidData` error.
  - `pub fn write_frame(w: &mut impl std::io::Write, v: &serde_json::Value) -> std::io::Result<()>` (flushes).
  - `pub struct RpcError { pub code: &'static str, pub message: String }` with `pub fn new(code: &'static str, message: impl Into<String>) -> Self`.
- Produces (main.rs): `struct State` (grows in later tasks), `fn dispatch(state: &mut State, method: &str, params: &serde_json::Value) -> Result<serde_json::Value, RpcError>`, and `state.events: Vec<serde_json::Value>` written as frames after each reply. Request `{"id","method","params"}`; reply `{"id","result"}` or `{"id","error":{"code","message"}}`. Unknown method: `unknown-method`, "AstroPort does not know <method>.".
- Produces (tests/common): `pub struct Fixture { pub install: PathBuf, pub data: PathBuf, pub dump: PathBuf, _dirs: Vec<tempfile::TempDir> }` with `Fixture::new() -> Fixture` (Task 1: install has `bin/astroport-adapter` copied from `env!("CARGO_BIN_EXE_astroport-adapter")`; later tasks add files), and `pub struct Adapter` with `Adapter::spawn(f: &Fixture) -> Adapter` (cwd `f.data`, `env_clear` plus `PATH`, `HOME`, `LANG`, as AGS spawns), `call(&mut self, method: &str, params: Value) -> Value` (the whole reply frame; panics after 10 s), `next_frame(&mut self) -> Value` (an event), kill on `Drop`.

- [ ] **Step 1: Write `adapter/Cargo.toml`**

`[package] name = "astroport-adapter"`, `version = "0.1.0"`, `edition = "2021"`, `license = "GPL-2.0-only"`, `description = "AGS extension adapter (protocol 2) for AstroPort"`. `[[bin]] name = "astroport-adapter" path = "src/main.rs"`. Deps `serde`, `serde_json` as the patcher, `sha2 = "0.10"`. Dev-deps `toml = "0.8"`, `tempfile = "3"`. `[profile.release] opt-level = 2`.

- [ ] **Step 2: Write the failing tests**

`adapter/src/rpc.rs` unit tests:

```rust
#[test] fn frame_round_trip() {
    let mut buf = Vec::new();
    write_frame(&mut buf, &json!({"id": 1, "method": "hello"})).unwrap();
    assert_eq!(&buf[..4], &(buf.len() as u32 - 4).to_be_bytes());
    assert_eq!(read_frame(&mut &buf[..]).unwrap(), Some(json!({"id": 1, "method": "hello"})));
    assert_eq!(read_frame(&mut &[][..]).unwrap(), None);
}
#[test] fn frame_too_large() {
    let len = ((1u32 << 20) + 1).to_be_bytes();
    assert!(read_frame(&mut &len[..]).is_err());
}
```

`adapter/tests/rpc.rs`:

```rust
#[test] fn hello_answers_protocol_2() {
    let f = Fixture::new(); let mut a = Adapter::spawn(&f);
    let r = a.call("hello", json!({"protocol": 2, "ags": "0.19.0", "features": []}));
    assert_eq!(r["result"]["protocol"], 2);
    assert_eq!(r["result"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(r["result"]["roles"], json!(["launcher"]));
    assert_eq!(r["result"]["features"], json!(["library", "page", "values"]));
    assert_eq!(r["result"]["controllers"], json!([{"kind": "dualsense",
        "features": ["gyro", "touchpad", "adaptive_triggers", "haptics"]}]));
}
#[test] fn unknown_method() {
    let f = Fixture::new(); let mut a = Adapter::spawn(&f);
    assert_eq!(a.call("nope", json!({}))["error"]["code"], "unknown-method");
}
```

`adapter/tests/manifest.rs` (parses `../ags-extension.toml` with `toml`):

```rust
#[test] fn manifest_is_protocol_2() {
    let m: toml::Table = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../ags-extension.toml")).unwrap().parse().unwrap();
    assert_eq!(m["id"].as_str(), Some("astroport"));
    assert_eq!(m["name"].as_str(), Some("AstroPort"));
    assert_eq!(m["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(m["license"].as_str(), Some("GPL-2.0-only"));
    assert_eq!(m["protocol"].as_integer(), Some(2));
    assert_eq!(m["roles"].as_array().unwrap(), &vec![toml::Value::from("launcher")]);
    assert_eq!(m["adapter"].as_str(), Some("bin/astroport-adapter"));
    assert_eq!(m["install"]["release"]["asset"].as_str(), Some("astroport-{version}-linux-{arch}.tar.zst"));
    let ch: Vec<_> = m["channels"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ch, ["stable", "testing"]);
}
#[test] fn assets_exist_and_are_inert_svg() {
    // every [assets] path exists; no <text>, <script>, <image>, "href=\"http", "on" attributes
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml`
Expected: compile errors (`read_frame` not found, `src/main.rs` missing) or, once files exist, FAIL in `manifest_is_protocol_2` (no `ags-extension.toml`).

- [ ] **Step 4: Implement**

`rpc.rs` as the interface says. `main.rs`: `State { events }`, loop `read_frame(stdin)` → `dispatch` → reply → drain `events`; exit 0 on EOF, exit 1 with one stderr line on a frame error. `hello` returns the result the test pins (version `env!("CARGO_PKG_VERSION")`, `metrics: []`).

`ags-extension.toml`, exact keys:

```toml
# AGS extension manifest (protocol 2). The copy at the tagged commit is what AGS reads.
id = "astroport"
name = "AstroPort"
version = "0.1.0"
license = "GPL-2.0-only"
protocol = 2
roles = ["launcher"]
adapter = "bin/astroport-adapter"

[assets]
icon = "assets/icon.svg"
logo = "assets/logo.svg"
badge = "assets/badge.svg"

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

[install.release]
asset = "astroport-{version}-linux-{arch}.tar.zst"
sha256_asset = "{asset}.sha256"

[install.source]
depends = ["cmake", "ninja", "clang", "clang++", "gcc", "g++", "python3", "git", "cargo", "nm", "strip"]
submodules = false
build = [
  ["scripts/build-engines.sh"],
  ["cargo", "build", "--release", "--locked", "--manifest-path", "patcher/Cargo.toml"],
  ["cargo", "build", "--release", "--locked", "--manifest-path", "adapter/Cargo.toml"],
  ["cargo", "build", "--release", "--locked", "--manifest-path", "runner/Cargo.toml"],
]
stage = [
  ["scripts/release/stage.sh", "{stage}"],
]
```

(`[install.source]` is step 2's "Handed to step 3" block, copied exactly.)

Assets: three small SVGs using only `svg`, `g`, `rect`, `circle`, `path`, `title` (the AGS allowlist), each under 4 KiB, an original shape (a rounded square with a simple rocket outline), `viewBox="0 0 64 64"`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml`
Expected: `test result: ok` for the unit tests, `rpc` and `manifest`; `Cargo.lock` created.

- [ ] **Step 6: Commit**

```bash
git add ags-extension.toml assets/icon.svg assets/logo.svg assets/badge.svg adapter/Cargo.toml adapter/Cargo.lock adapter/src/main.rs adapter/src/rpc.rs adapter/tests/common/mod.rs adapter/tests/rpc.rs adapter/tests/manifest.rs
git commit -m "Adapter: protocol 2 manifest, frames and hello" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: `versions.json` and version detection

**Files:**
- Create: `patches/versions.json`, `adapter/src/dump.rs`
- Modify: `adapter/src/main.rs` (`mod dump;`)

**Interfaces:**
- Consumes: nothing from Task 1 beyond the crate.
- Produces (dump.rs):
  - `#[derive(Deserialize, Clone)] pub struct VersionEntry { pub version: String, pub content_version: String, pub eboot_sha256: Option<String>, pub patches: String }`
  - `#[derive(Deserialize)] pub struct Versions { pub title_id: String, pub versions: Vec<VersionEntry> }`, `pub fn load_versions(path: &Path) -> Result<Versions, String>`
  - `pub enum Detected { NotSet, NotFound, Unsupported { dump: PathBuf, version: String, message: String }, Supported { dump: PathBuf, entry: VersionEntry, eboot_sha256: String } }`
  - `pub fn find_dump(folder: &Path, title_id: &str) -> Option<(PathBuf, serde_json::Value)>` — `folder`, then its direct children sorted by name; a dump has a regular `eboot.bin` and `sce_sys/param.json` whose `titleId` equals `title_id`; returns the dump and the parsed `param.json`.
  - `pub fn display_version(content_version: &str) -> String` — `"01.019.000"` → `"1.019"`; anything not `NN.NNN.NNN` is returned as given.
  - `pub fn unsupported_message(version: &str, v: &Versions) -> String`
  - `#[derive(Default)] pub struct HashCache` with `pub fn sha256(&mut self, path: &Path) -> std::io::Result<String>` (lower-case hex; cached by path, length and mtime).
  - `pub fn detect(folder: Option<&Path>, v: &Versions, cache: &mut HashCache) -> Detected`
  - `pub fn run_folder(data: &Path, version: &str) -> PathBuf` (`<data>/generated/run-<version>`)
  - `pub fn built(data: &Path, version: &str, eboot_sha256: &str, release: &str) -> bool` — true when `<run>/AstroBot` is a regular file, not a link (`symlink_metadata`; M2-5 sheet §3.5 follows no link), and `<run>/build.json` parses with `format == 1`, `version`, `eboot_sha256` and `release` all equal (format in "Interfaces with other steps"); other keys are ignored (step 4 also writes `content_version`, `inputs_sha256`, `to_intel`, `dump`).

`patches/versions.json`, exact:

```json
{
  "title_id": "PPSA21567",
  "versions": [
    {"version": "1.018", "content_version": "01.018.000", "eboot_sha256": null, "patches": "PPSA21567-1018.json"},
    {"version": "1.019", "content_version": "01.019.000", "eboot_sha256": "e88e19a688289e1ea557a6100b9b7d89fb3041875a7aa8471b361bfedf59fa86", "patches": "PPSA21567-1019.json"}
  ]
}
```

(1.018 is matched by version only: its dump was deleted, design 5.1. The 1.018 patch file carries a `source_sha256` field; whether that is the 1.018 eboot hash is for the owner to confirm, it is not used here.)

Detection rule: `NotSet` when `folder` is `None` or empty; `NotFound` when `find_dump` finds nothing; read `contentVersion` (missing: `NotFound`); hash `eboot.bin`; the entry whose `content_version` matches and whose `eboot_sha256` is null or equal is `Supported`; a version that matches an entry with a different hash is `Unsupported` with `This copy of version 1.019 is not supported. AstroPort runs 1.018 and 1.019.`; any other version is `Unsupported` with `unsupported_message`.

- [ ] **Step 1: Write the failing tests** (unit tests in `dump.rs`, fixtures in `tempfile::tempdir()`; a helper writes `eboot.bin` bytes and `sce_sys/param.json`)

```rust
#[test] fn repo_versions_file() {
    let v = load_versions(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../patches/versions.json"))).unwrap();
    assert_eq!(v.title_id, "PPSA21567");
    assert_eq!(v.versions[0].eboot_sha256, None);
    assert_eq!(v.versions[1].eboot_sha256.as_deref(), Some("e88e19a688289e1ea557a6100b9b7d89fb3041875a7aa8471b361bfedf59fa86"));
    for e in &v.versions { assert!(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../patches")).join(&e.patches).is_file()); }
}
#[test] fn display_versions() { assert_eq!(display_version("01.019.000"), "1.019"); assert_eq!(display_version("weird"), "weird"); }
#[test] fn unsupported_text() {
    assert_eq!(unsupported_message("1.020", &repo()), "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019.");
}
#[test] fn finds_dump_in_child_folder() { /* parent/PPSA21567-app/{eboot.bin,sce_sys/param.json}; find_dump(parent) == child */ }
#[test] fn foreign_or_broken_param_json_is_not_found() { /* titleId PPSA00001; "{not json"; no contentVersion -> NotFound */ }
#[test] fn version_only_entry_matches_any_hash() { /* 01.018.000 with any eboot -> Supported 1.018 */ }
#[test] fn hash_must_match_when_listed() {
    /* versions with 1.019 hash = sha256(b"fixture"); eboot b"fixture" -> Supported; b"other" ->
       Unsupported, message "This copy of version 1.019 is not supported. AstroPort runs 1.018 and 1.019." */
}
#[test] fn other_version_unsupported() { /* 01.020.000 -> Unsupported { version: "1.020", .. } */ }
#[test] fn not_set() { assert!(matches!(detect(None, &repo(), &mut HashCache::default()), Detected::NotSet)); }
#[test] fn built_needs_matching_record() {
    /* no folder -> false; AstroBot + build.json {format 1, version 1.019, eboot_sha256 h, release 0.1.0} -> true;
       the same plus step 4's extra keys (content_version, inputs_sha256, to_intel, dump) -> true;
       release "0.0.9" -> false; eboot_sha256 other -> false; version "1.018" -> false;
       AstroBot a symlink to a file -> false (M2-5 sheet §3.5) */
}
#[test] #[ignore] fn owner_dump_is_1019() {
    // Opt-in, reads only: ASTROPORT_TEST_DUMP=<dump folder>. The private src-1019 holds only an
    // unwrapped eboot.elf and no sce_sys, so it cannot be used here.
    let d = std::env::var("ASTROPORT_TEST_DUMP").expect("set ASTROPORT_TEST_DUMP");
    assert!(matches!(detect(Some(Path::new(&d)), &repo(), &mut HashCache::default()),
        Detected::Supported { ref entry, .. } if entry.version == "1.019"));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml dump::`
Expected: compile errors (`load_versions` not found).

- [ ] **Step 3: Implement `dump.rs` and write `patches/versions.json`** as the interface says. Hash with `sha2::Sha256` over a 1 MiB buffered read.

- [ ] **Step 4: Run to verify they pass**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml dump::`
Expected: all pass, `owner_dump_is_1019` ignored.

Run (opt-in, reads the owner's dump only): `ASTROPORT_TEST_DUMP=~/Games/ps5/Games/PPSA21567-01.019 timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml dump::owner_dump_is_1019 -- --ignored`
Expected: `1 passed` (also proves Review Focus 1 on the real layout).

- [ ] **Step 5: Commit**

```bash
git add patches/versions.json adapter/src/dump.rs adapter/src/main.rs
git commit -m "Adapter: dump and version detection with versions.json" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Machine values, `library.list` and the extension page

**Files:**
- Create: `adapter/src/values.rs`, `adapter/src/page.rs` (extension page only here)
- Modify: `adapter/src/main.rs`, `adapter/tests/common/mod.rs` (fixture install gets `patches/` copied from the repo plus a fixture `versions.json`; fixture dump `eboot.bin` = `b"fixture"`, `param.json` `{"titleId":"PPSA21567","contentVersion":"01.019.000"}`, `sce_sys/icon0.png` a 1x1 PNG), `adapter/tests/rpc.rs`

**Interfaces:**
- Consumes: `dump::{load_versions, detect, Detected, HashCache, built, display_version}`.
- Produces (values.rs): `pub struct Store` with `pub fn load(data: &Path) -> Store` (missing or bad file: empty), `pub fn machine(&self) -> &BTreeMap<String, Value>`, `pub fn set_machine(&mut self, values: BTreeMap<String, Value>) -> Result<(), String>` (null removes; only key `dump_folder`, value an absolute path string or `""`; else `Err("Invalid value for <id>.")`; writes `<data>/values.json` `{"machine": {...}}` via temp file and rename), `pub fn dump_folder(&self) -> Option<PathBuf>`.
- Produces (page.rs): `pub const GAME_ID: &str = "PPSA21567";` `pub fn extension_page(store: &Store, detected: &Detected) -> Value`.
- Produces (main.rs): `State { install, data, versions, store, cache, events, .. }`; `fn detect(&mut self) -> Detected` helper.

Methods:
- `library.list {}` → `{"games": []}` unless `Supported` or `Unsupported`; then one entry: `{"id": "PPSA21567", "title": "Astro Bot", "platform": "PS5", "identity": {"scheme": "ps5-title", "value": "PPSA21567"}, "version": "<1.019 or the unsupported version>", "installed": <Supported and built(..., release = CARGO_PKG_VERSION)>}` plus `"cover"` and `"icon"` = `<dump>/sce_sys/icon0.png` only when it is a regular file of at most 2 MiB (AGS rejects the whole entry for bad art).
- `values.get {scope: "machine"}` → `{"values": machine}`; `{scope: "game", game}` → `{"values": {}}`.
- `values.set {scope: "machine", values}` → `{"values": machine after}`; then push events `{"method": "library.changed", "params": {}}` and `{"method": "page.changed", "params": {}}`. Scope `game`: `invalid-params` "AstroPort has no per-game values.". Bad value: `invalid-params` with the store's sentence.
- `page.describe {}` → extension page: one section `{"id": "astroport.dump", "surface": "panel", "title": "Game dump", "items": [...]}` (no `tab`) with `dump_folder` (`kind: "path"`, `pick: "folder"`, `nullable: true`, `null_label: "Not set"`, `scope: "machine"`, `label: "Astro Bot dump folder"`, `help: "The folder with eboot.bin and sce_sys, or the folder above it."`) and `dump.status` (`kind: "info"`, `label: "Status"`, `text` by state: NotSet `Choose the folder of your Astro Bot dump.`; NotFound `No Astro Bot dump in this folder.`; Supported `Found version 1.019.`; Unsupported its message).

- [ ] **Step 1: Write the failing tests** (`adapter/tests/rpc.rs`; `values.rs` unit tests for the store rules)

```rust
#[test] fn no_dump_no_game() {
    let f = Fixture::new(); let mut a = Adapter::spawn(&f);
    assert_eq!(a.call("library.list", json!({}))["result"]["games"], json!([]));
    let p = a.call("page.describe", json!({}))["result"].clone();
    assert_eq!(item(&p, "dump.status")["text"], "Choose the folder of your Astro Bot dump.");
}
#[test] fn set_dump_lists_the_game_and_notifies() {
    let f = Fixture::new(); let mut a = Adapter::spawn(&f);
    let r = a.call("values.set", json!({"scope": "machine", "values": {"dump_folder": f.dump}}));
    assert_eq!(r["result"]["values"]["dump_folder"], json!(f.dump));
    assert_eq!(a.next_frame()["method"], "library.changed");
    assert_eq!(a.next_frame()["method"], "page.changed");
    let g = &a.call("library.list", json!({}))["result"]["games"][0];
    assert_eq!((g["id"].as_str(), g["platform"].as_str(), g["version"].as_str(), g["installed"].as_bool()),
               (Some("PPSA21567"), Some("PS5"), Some("1.019"), Some(false)));
    assert_eq!(g["identity"], json!({"scheme": "ps5-title", "value": "PPSA21567"}));
    assert!(g["cover"].as_str().unwrap().ends_with("sce_sys/icon0.png"));
    // persisted: a new adapter reads it back
    drop(a); let mut b = Adapter::spawn(&f);
    assert_eq!(b.call("values.get", json!({"scope": "machine"}))["result"]["values"]["dump_folder"], json!(f.dump));
}
#[test] fn relative_dump_folder_refused() {
    let f = Fixture::new(); let mut a = Adapter::spawn(&f);
    assert_eq!(a.call("values.set", json!({"scope": "machine", "values": {"dump_folder": "games/astro"}}))["error"]["code"], "invalid-params");
}
#[test] fn broken_param_json_says_not_found() { /* write "{not json" over the fixture param.json; status text "No Astro Bot dump in this folder."; games [] */ }
#[test] fn unsupported_version_listed_not_installed() { /* contentVersion 01.020.000: game version "1.020", installed false; status text the unsupported message */ }
#[test] fn oversized_icon_left_out() { /* icon0.png of 2 MiB + 1 byte: no "cover" or "icon" key */ }
```

- [ ] **Step 2: Run to verify they fail**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml`
Expected: the new `rpc` tests FAIL with `unknown-method`.

- [ ] **Step 3: Implement** `values.rs`, `page::extension_page`, and the three methods in `dispatch`. `features` in `hello` stay `["library", "page", "values"]` until Task 8 adds `jobs`.

- [ ] **Step 4: Run to verify they pass**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml`
Expected: `test result: ok` in every target.

- [ ] **Step 5: Commit**

```bash
git add adapter/src/values.rs adapter/src/page.rs adapter/src/main.rs adapter/tests/common/mod.rs adapter/tests/rpc.rs
git commit -m "Adapter: dump folder setting, library and extension page" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `patches/settings.json` and patch selection

**Files:**
- Create: `patches/settings.json`, `adapter/src/settings.rs`
- Modify: `adapter/src/main.rs` (`mod settings;`, load `<install>/patches/settings.json` into `State.map`; a load or `check` failure is answered as `failed` "The AstroPort installation is damaged. Repair it." on every method but `hello`)

**Interfaces:**
- Produces (settings.rs):
  - `#[derive(Deserialize)] pub struct SettingsMap { pub format: u32, pub fixed_env: BTreeMap<String, String>, pub presets: Vec<String>, pub patches: BTreeMap<String, String>, pub settings: BTreeMap<String, Setting>, pub advanced: Vec<Advanced> }`
  - `#[derive(Deserialize)] pub struct Setting { pub values: BTreeMap<String, Choice> }`
  - `#[derive(Deserialize, Default)] pub struct Choice { #[serde(default)] pub on: Vec<String>, #[serde(default)] pub off: Vec<String>, #[serde(default)] pub env: BTreeMap<String, String> }`
  - `#[derive(Deserialize)] pub struct Advanced { pub id: String, pub label: String, pub default: bool }`
  - `pub fn load(path: &Path) -> Result<SettingsMap, String>` (parses and runs `check`)
  - `impl SettingsMap { pub fn check(&self) -> Result<(), String>; pub fn available(&self, patch_file: &Path) -> Result<BTreeSet<String>, String>; pub fn select(&self, chosen: &BTreeMap<String, String>, advanced_on: &BTreeSet<String>) -> Result<Selection, String>; pub fn patch_env(&self, s: &Selection) -> (String, String) }`
  - `pub struct Selection { pub on: BTreeSet<String>, pub off: BTreeSet<String>, pub env: BTreeMap<String, String> }`
- Rules:
  - `patches`: patch id → lower-case substring of exactly one mod name (the patcher matches `ASTRO_PATCH_ENABLE`/`ASTRO_PATCH_SKIP` entries as comma-separated, lower-cased substrings, `patcher/src/lib.rs`). No match string contains a comma.
  - `check`: every patch id is owned by exactly one place, either one setting's group or one `advanced` entry; within a setting, every value's `on ∪ off` is the same set (the group) and `on ∩ off` is empty; every id named anywhere exists in `patches`.
  - `available`: the ids whose match string is a substring of some lower-cased mod name of that patch file.
  - `select`: `chosen` maps setting key → value key (unknown key or value: `Err`); the chosen value's `on`/`off`/`env` are merged; every `advanced` id goes to `on` when in `advanced_on`, else `off`. A setting missing from `chosen` is an `Err` (callers fill defaults first).
  - `patch_env`: `(ASTRO_PATCH_ENABLE, ASTRO_PATCH_SKIP)` = the match strings of `on` and of `off`, sorted, joined with `,`.

`patches/settings.json` content (ids and match strings exact; the groups follow design 6):

```json
{
  "format": 1,
  "fixed_env": {
    "GLIBC_TUNABLES": "glibc.rtld.optional_static_tls=0x200000",
    "APS5_GPU_ENGINE": "kyty",
    "KYTY_GPU_LIBRARY": "{run}/libs/libkyty_gpu.so",
    "KYTY_GPU_HOST_MODULE": "{run}/libs/libAstroHybridHost.prx",
    "KYTY_HANG_WATCHDOG": "0",
    "ASTROHYBRID_CACHE_TITLE_ID": "PPSA21567",
    "APS5_PAD_TRIGGER_STATE": "1", "ASTROHYBRID_PAD_MOTION": "1", "KYTY_CP_PRIORITY": "2",
    "KYTY_DRAW_PREP_PRIORITY": "2", "KYTY_DRAW_PREP_SPIN_US": "10", "KYTY_DRAW_PREP_COLD_SPIN_US": "5",
    "KYTY_AGC_DONE_MODE": "bounded",
    "ASTROHYBRID_PAD_PREFER": "auto", "ASTROHYBRID_RUMBLE_SCALE": "100", "ASTROHYBRID_RUMBLE_SWITCH_FLOOR": "96",
    "ASTROHYBRID_WINDOW_TITLE": "Astro Bot",
    "ASTROHYBRID_COMMIT_EPOCH_FASTPATH": "0", "KYTY_BUFFER_RANGE_FIXES": "1",
    "APS5_SAVEDATA_TRANSFER_FALLBACK": "1",
    "KYTY_WAYLAND_NATIVE": "1",
    "LD_AUDIT": "{install}/lib/libastro_patcher.so",
    "ASTRO_PATCH_PROCESS": "AstroBot"
  },
  "presets": ["{install}/presets/u59-preset.json", "{install}/presets/u59-preset.PPSA21567.json"],
  "patches": {
    "lighting.deferred": "non-tiled deferred-lighting",
    "lighting.gi": "disable gi probes",
    "timing.decouple": "decouple gameplay timers",
    "timing.no_sleep": "disable game cpu frame-limiter sleep",
    "pad.r1x": "r1+x toggles",
    "cap.30": "precise frame cap 30 fps", "cap.60": "precise frame cap 60 fps", "cap.90": "precise frame cap 90 fps",
    "cap.120": "precise frame cap 120 fps", "cap.144": "precise frame cap 144 fps",
    "render.1080": "fixed render resolution 1920x1080", "render.1368": "fixed render resolution 2432x1368",
    "render.1872": "fixed render resolution 3328x1872", "render.2160": "fixed render resolution 3840x2160",
    "physics.rope": "[physics] rope bridges", "physics.cloth": "[physics] cloth and inflatables",
    "physics.jack": "[physics] jack-in-the-box",
    "fix.havok_contact": "havok contact query", "fix.havok_tree": "havok tree queries",
    "fix.bot_rescue": "[fix] bot rescue",
    "dt.20": "delta time step capped at 1/20 s", "dt.30": "delta time step capped at 1/30 s",
    "mip.1080": "1080p mip bias", "mip.low": "low detail mip bias",
    "tex.2k": "4k textures load at 2k",
    "lod.half": "lod x0.5", "lod.quarter": "lod x0.25",
    "video.speed": "videos play at real speed",
    "intro.skip": "skip the playstation studios intro",
    "ultrawide": "ultrawide:"
  },
  "settings": {
    "display.fps": {"values": {
      "30": {"on": ["cap.30", "timing.decouple", "timing.no_sleep", "dt.20", "physics.rope", "physics.cloth", "physics.jack"],
             "off": ["cap.60", "cap.90", "cap.120", "cap.144", "dt.30"],
             "env": {"ASTROHYBRID_FPS_LIMIT": "30", "ASTROHYBRID_LOW_LATENCY_INTERVAL_US": "33333"}},
      "60": {"on": ["cap.60", "timing.decouple", "timing.no_sleep", "dt.20"],
             "off": ["cap.30", "cap.90", "cap.120", "cap.144", "dt.30", "physics.rope", "physics.cloth", "physics.jack"],
             "env": {"ASTROHYBRID_FPS_LIMIT": "60", "ASTROHYBRID_LOW_LATENCY_INTERVAL_US": "16666"}}}},
    "graphics.render": {"values": {
      "1080p": {"on": ["render.1080"], "off": ["render.1368", "render.1872", "render.2160"]},
      "1368p": {"on": ["render.1368"], "off": ["render.1080", "render.1872", "render.2160"]},
      "1872p": {"on": ["render.1872"], "off": ["render.1080", "render.1368", "render.2160"]},
      "2160p": {"on": ["render.2160"], "off": ["render.1080", "render.1368", "render.1872"]}}},
    "graphics.lighting": {"values": {
      "fast": {"on": ["lighting.deferred", "lighting.gi"]},
      "original": {"off": ["lighting.deferred", "lighting.gi"]}}},
    "graphics.detail": {"values": {
      "high": {"off": ["lod.half", "lod.quarter"]},
      "medium": {"on": ["lod.half"], "off": ["lod.quarter"]},
      "low": {"on": ["lod.quarter"], "off": ["lod.half"]}}},
    "graphics.textures": {"values": {
      "sharp": {"off": ["mip.1080", "mip.low"]},
      "balanced": {"on": ["mip.1080"], "off": ["mip.low"]},
      "soft": {"on": ["mip.low"], "off": ["mip.1080"]}}},
    "graphics.ultrawide": {"values": {
      "true": {"on": ["ultrawide"], "env": {"KYTY_GPU_WIDTH": "3440", "KYTY_GPU_HEIGHT": "1440"}},
      "false": {"off": ["ultrawide"]}}},
    "game.skip_intro": {"values": {"true": {"on": ["intro.skip"]}, "false": {"off": ["intro.skip"]}}},
    "controller.rumble": {"values": {
      "true": {"env": {"ASTROHYBRID_DUALSENSE_RUMBLE": "1"}}, "false": {"env": {"ASTROHYBRID_DUALSENSE_RUMBLE": "0"}}}},
    "perf.low_latency": {"values": {
      "on": {"env": {"ASTROHYBRID_LOW_LATENCY": "1"}}, "off": {"env": {"ASTROHYBRID_LOW_LATENCY": "0"}}}},
    "fullscreen": {"values": {
      "true": {"env": {"KYTY_GPU_FULLSCREEN": "1"}}, "false": {"env": {"KYTY_GPU_FULLSCREEN": "0"}}}},
    "graphics.dlss": {"values": {
      "true": {"env": {"ASTROHYBRID_DLSS": "1"}}, "false": {"env": {"ASTROHYBRID_DLSS": "0"}}}}
  },
  "advanced": [
    {"id": "pad.r1x", "label": "R1 + X switches the game timing", "default": true},
    {"id": "fix.havok_contact", "label": "Physics crash fix: contact query", "default": true},
    {"id": "fix.havok_tree", "label": "Physics crash fix: tree query", "default": true},
    {"id": "fix.bot_rescue", "label": "Bot rescue timing fix", "default": true},
    {"id": "tex.2k", "label": "Load 4K textures at 2K", "default": false},
    {"id": "video.speed", "label": "Videos at real speed", "default": true}
  ]
}
```

OWNER-TEST (not an AGS dependency): at 60, whether `timing.decouple`, `timing.no_sleep` and `dt.20` stay on is settled by the owner's play tests (design 6); the file keeps them on (the 60 FPS cap needs Decouple). Changing that is a data edit of `display.fps.values.60`.

- [ ] **Step 1: Write the failing tests** (unit tests in `settings.rs` against the repo files)

```rust
fn repo() -> SettingsMap { load(&repo_path("patches/settings.json")).unwrap() }
#[test] fn repo_map_passes_check() { repo().check().unwrap(); }
#[test] fn every_mod_owned_once_in_both_versions() {
    // for PPSA21567-1018.json and -1019.json: every mod name is matched by exactly one patch id,
    // and every patch id matches at most one mod (exactly one, except "ultrawide" absent in 1018)
}
#[test] fn ultrawide_only_in_1019() {
    let m = repo();
    assert!(!m.available(&repo_path("patches/PPSA21567-1018.json")).unwrap().contains("ultrawide"));
    assert!(m.available(&repo_path("patches/PPSA21567-1019.json")).unwrap().contains("ultrawide"));
}
#[test] fn one_lod_and_one_cap_at_a_time() {
    let s = repo().select(&chosen(&[("display.fps","60"),("graphics.detail","low"), /* all others */]), &BTreeSet::new()).unwrap();
    assert!(s.on.contains("cap.60") && s.off.contains("cap.30") && s.off.contains("physics.rope"));
    assert!(s.on.contains("lod.quarter") && s.off.contains("lod.half"));
    assert!(s.on.is_disjoint(&s.off));
    assert_eq!(s.on.len() + s.off.len(), repo().patches.len()); // every patch explicit
}
#[test] fn patch_env_joins_match_strings() { /* ENABLE contains "precise frame cap 60 fps", SKIP contains "precise frame cap 30 fps"; no entry has a comma */ }
#[test] fn check_refuses_overlap_and_uneven_groups() {
    // an id in two settings -> Err; a value whose on∪off differs from its sibling -> Err; an unknown id -> Err
}
#[test] fn select_refuses_unknown_value() { /* ("graphics.render", "4320p") -> Err */ }
```

- [ ] **Step 2: Run to verify they fail**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml settings::`
Expected: compile errors (`load` not found in `settings`).

- [ ] **Step 3: Write `patches/settings.json` and implement `settings.rs`** as the interface says.

- [ ] **Step 4: Run to verify they pass**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml settings::`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add patches/settings.json adapter/src/settings.rs adapter/src/main.rs
git commit -m "Settings map: every setting to patches and variables, one patch per group member" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Device defaults

**Files:**
- Create: `adapter/src/device.rs`
- Modify: `adapter/src/main.rs` (`mod device;`, `State.device: Option<Device>` probed once on first use with `probe(Path::new("/sys"), nvidia_smi)`)

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Default, PartialEq)] pub struct Device { pub gpu: String, pub vram_mib: Option<u64>, pub screen: Option<(u32, u32)>, pub nvidia: bool }`
  - `pub fn probe(sys: &Path, nvidia: impl Fn() -> Option<String>) -> Device` — for each `<sys>/class/drm/card<N>/device`: `vendor` `0x10de` → parse `nvidia()` output `"<name>, <MiB>"` (first line); `0x1002` → `mem_info_vram_total` bytes / 1 MiB, `gpu` = `"AMD"`; else `gpu` = `"unknown"`, no VRAM. The card with the most VRAM wins; `nvidia` is true when that card's `vendor` is `0x10de`. Screen: the largest `WxH` (by height) of the first line of `modes` among `<sys>/class/drm/card<N>-*` whose `status` is `connected`.
  - `pub fn nvidia_smi() -> Option<String>` — runs `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`, killed after 2 s (`try_wait` poll); not called by tests.
  - `#[derive(Debug, PartialEq)] pub struct Defaults { pub render: &'static str, pub lighting: &'static str, pub dlss: bool }`, `pub fn defaults(d: &Device) -> Defaults` (`dlss` = `d.nvidia`: DLSS is offered only on NVIDIA GPUs, design 4).
- Table (design 10 leaves it to the builder): VRAM cap: unknown or < 8 GiB → `1080p`; < 12 GiB → `1368p`; < 16 GiB → `1872p`; else `2160p`. Then the largest of `1080p`, `1368p`, `1872p`, `2160p` not above the screen height (1080, 1368, 1872, 2160 lines) and not above the VRAM cap; never below `1080p`. Lighting `original` when VRAM ≥ 16 GiB, else `fast`. A first-run benchmark is out of scope (design 5.4).

- [ ] **Step 1: Write the failing tests** (fake sysfs in a temp dir)

```rust
#[test] fn nvidia_4k() {
    let d = probe(&fake(&[("card1", "0x10de", None)], &[("card1-DP-1", "connected", "3840x2160")]), || Some("NVIDIA GeForce RTX 4080, 16376".into()));
    assert_eq!(d, Device { gpu: "NVIDIA GeForce RTX 4080".into(), vram_mib: Some(16376), screen: Some((3840, 2160)), nvidia: true });
    assert_eq!(defaults(&d), Defaults { render: "1872p", lighting: "fast", dlss: true }); // 16376 MiB < 16 GiB
}
#[test] fn amd_vram_from_sysfs() { /* mem_info_vram_total 17179869184 -> vram 16384, render "2160p" on a 4K screen, lighting "original", dlss false */ }
#[test] fn screen_caps_render() { /* 24 GiB, screen 2560x1440 -> "1368p" */ }
#[test] fn unknown_device_is_safe() { assert_eq!(defaults(&Device::default()), Defaults { render: "1080p", lighting: "fast", dlss: false }); }
#[test] fn disconnected_screen_ignored() { /* status "disconnected" 7680x4320 -> screen None */ }
```

- [ ] **Step 2: Run to verify they fail**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml device::`
Expected: compile errors (`probe` not found).

- [ ] **Step 3: Implement `device.rs`.**

- [ ] **Step 4: Run to verify they pass**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml device::`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add adapter/src/device.rs adapter/src/main.rs
git commit -m "Adapter: render resolution and lighting defaults from the GPU and screen" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: The game page

**Files:**
- Modify: `adapter/src/page.rs`, `adapter/src/main.rs` (`page.describe {game}`), `adapter/tests/rpc.rs`
- Create: `adapter/src/sheet.rs` (only `standard_support` here; Task 8 adds `job_plan`)

**Interfaces:**
- Consumes: `dump::{Detected, built, run_folder}`, `settings::SettingsMap::available`, `device::Defaults`.
- Produces (page.rs):
  - `pub fn item_defaults(map: &SettingsMap, available: &BTreeSet<String>, d: &Defaults) -> BTreeMap<String, Value>` — `graphics.render` = `d.render`, `graphics.lighting` = `d.lighting`, `graphics.detail` `"medium"`, `graphics.textures` `"soft"`, `graphics.ultrawide` `false` (only when `ultrawide` is available), `graphics.dlss` = `d.dlss` (always present, so `select` has a value; `false` forces `ASTROHYBRID_DLSS=0` off NVIDIA), `controller.haptics` `250`, `controller.rumble` `true`, `game.skip_intro` `true`, `advanced.patches` = the available `advanced` ids with `default: true`, in file order.
  - `pub fn game_page(detected: &Detected, is_built: bool, map: &SettingsMap, available: &BTreeSet<String>, d: &Defaults) -> Value`
  - `pub fn check_limits(page: &Value) -> Result<(), String>` — the id, uniqueness and length rules of Global Constraints (used by tests, and by `dispatch` in debug builds as a `debug_assert`).
- Produces (sheet.rs): `pub fn standard_support() -> Value` — added to the game page as top-level `"standard"` (`Description.standard`, M2-5 sheet §1.1 and §2.2: a game page entry replaces the extension page's key by key; ignored by M1 hosts, which ignore unknown keys, m1 sheet 1.2).

Game page, Supported (every value item `scope: "game_on_device"`, `restart: "next_launch"`, `default` from `item_defaults`; sections in this order):

| Section id | Surface / tab | Title | Items |
| --- | --- | --- | --- |
| `astroport.build` | `hero` | (none) | `build`: `button`, label `Build game files`, `job: true`. Only when not `is_built`. |
| `astroport.graphics` | `panel` / `source` | `Graphics` | `graphics.render` choice `1080p`/`1368p`/`1872p`/`2160p` (labels the same), label `Render resolution`; `graphics.lighting` choice `fast` "Fast" / `original` "Original", `presentation: "segmented"`, label `Lighting`; `graphics.detail` choice `high` "High" / `medium` "Medium" / `low` "Low", segmented, label `Detail`; `graphics.textures` choice `sharp` "Sharp" / `balanced` "Balanced" / `soft` "Soft", segmented, label `Texture sharpness`; `graphics.ultrawide` switch, label `Ultrawide (21:9)`, help `Renders at 3440x1440.`, only when `ultrawide` is available; `graphics.dlss` switch, label `DLSS`, help `NVIDIA upscaling.`, only when `d.dlss` (NVIDIA GPU) |
| `astroport.controller` | `panel` / `source` | `Controller` | `controller.haptics` slider `min 100`, `max 400`, `step 25`, `unit "%"`, `stops [{value 100, label "As sent"}]`, label `Haptic strength`; `controller.rumble` switch, label `Rumble` |
| `astroport.game` | `panel` / `source` | `Game` | `game.skip_intro` switch, label `Skip the studio intro` |
| `astroport.advanced` | `panel` / `source` | `Advanced` | `advanced.patches` `toggle_list`, label `Patches`, `entries` = the available `advanced` entries (`id`, `label`) |

Unsupported: one section `astroport.status` (`panel` / `source`, title `Version`) with `status` (`info`, label `Version`, `text` = the message). No hero, no other section. NotSet/NotFound or another game id: `not-found` "AstroPort has no such game.".

`standard_support` (M2-5 sheet §1.2 `Support`, §2.1 value forms, §2.2 levels), exact:

```json
{"display.resolution": {"level": "honoured", "live": "next_launch", "applier": "source"},
 "display.fps": {"level": "honoured", "live": "next_launch", "applier": "source", "values": [30, 60]},
 "perf.low_latency": {"level": "honoured", "live": "next_launch", "applier": "source", "values": ["off", "on"]}}
```

- `perf.frame_generation` is left out on purpose: absent, AGS offers it itself as `{"level": "override", "live": "next_launch", "applier": "ags", "requires": {"play": "local"}, "method": "lsfg"}` (§2.2), which is design 5.4's "does not refuse it". (`requires` keys are only `play` and `capture` with the values of §2.3; `{"capture": "local"}` would be dropped by `sanitize`.)
- `perf.render_scale` is not declared (absent = unsupported): its values are integer percents (§2.1), so AstroPort's named render sizes stay the item `graphics.render`.
- `display.fps` and `perf.low_latency` values: the host clamps (§2.4 step 4): an `ORDERED` key gets the largest allowed value not above the request, else the smallest (144 → 60, 0 → 30); `perf.low_latency` `boost` is not offered and clamps to the first allowed value, `off`. The adapter's own mapping in Task 7 stays for M1 hosts, which do not clamp.
- No item id equals a key of `standard::KEYS` (§1.4: such an item is dropped).

- [ ] **Step 1: Write the failing tests** (`adapter/tests/rpc.rs`; `page.rs` unit tests for `check_limits`)

```rust
#[test] fn game_page_supported_not_built() {
    let f = Fixture::new(); let mut a = Adapter::spawn(&f); set_dump(&mut a, &f);
    let p = a.call("page.describe", json!({"game": "PPSA21567"}))["result"].clone();
    let ids: Vec<_> = p["sections"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["astroport.build", "astroport.graphics", "astroport.controller", "astroport.game", "astroport.advanced"]);
    assert_eq!(item(&p, "build")["job"], true);
    assert!(item(&p, "graphics.ultrawide").is_object());              // 1.019
    let opts: Vec<_> = item(&p, "graphics.render")["options"].as_array().unwrap().iter().map(|o| o["value"].clone()).collect();
    assert!(opts.contains(&item(&p, "graphics.render")["default"]));    // device default is an option
    assert_eq!(item(&p, "advanced.patches")["default"], json!(["pad.r1x", "fix.havok_contact", "fix.havok_tree", "fix.bot_rescue", "video.speed"]));
    assert_eq!(p["standard"]["display.fps"]["values"], json!([30, 60]));   // M2-5 sheet §2.2
    assert!(p["standard"].get("perf.frame_generation").is_none());       // AGS default applies
    page::check_limits(&p).unwrap();
}
#[test] fn built_run_folder_hides_the_build_button() { /* write <data>/generated/run-1.019/{AstroBot, build.json matching}: no astroport.build; library installed true */ }
#[test] fn rebuilt_dump_shows_build_again() { /* build.json with eboot_sha256 of another eboot: build button back, installed false (Review Focus 4) */ }
#[test] fn dlss_only_on_nvidia() {
    /* page.rs unit test: game_page with Defaults { dlss: true } has item graphics.dlss with default true;
       with Defaults { dlss: false } it has no graphics.dlss item and item_defaults["graphics.dlss"] == false */
}
#[test] fn version_1018_has_no_ultrawide() { /* contentVersion 01.018.000: no graphics.ultrawide, no "ultrawide" in defaults */ }
#[test] fn unsupported_page_has_only_the_message() {
    /* 01.020.000: sections == [astroport.status], status text "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019." */
}
#[test] fn other_game_not_found() { /* {"game": "CUSA00001"} -> error not-found */ }
```

- [ ] **Step 2: Run to verify they fail**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml`
Expected: the new tests FAIL (`page.describe {game}` answers `not-found`).

- [ ] **Step 3: Implement** `item_defaults`, `game_page`, `check_limits`, `sheet::standard_support`, and the dispatch branch (available ids from `<install>/patches/<entry.patches>`; defaults from `State.device`).

- [ ] **Step 4: Run to verify they pass**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml`
Expected: `test result: ok` in every target.

- [ ] **Step 5: Commit**

```bash
git add adapter/src/page.rs adapter/src/sheet.rs adapter/src/main.rs adapter/tests/rpc.rs
git commit -m "Adapter: game page with the AstroPort settings and the build button" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: `launch.plan`

**Files:**
- Create: `adapter/src/launch.rs`
- Modify: `adapter/src/main.rs`, `adapter/tests/common/mod.rs` (fixture install gets `presets/u59-preset.json` and `presets/u59-preset.PPSA21567.json` copied from `engines/kyty/tools/`, and an empty `lib/libastro_patcher.so`), `adapter/tests/rpc.rs`

**Interfaces:**
- Consumes: `dump::{Detected, built, run_folder}`, `settings::{SettingsMap, Selection}`, `page::item_defaults`, `device::Device`.
- Produces:
  - `pub fn language_index(name: &str) -> Option<u32>` — registry (Steam) names in PS5 order, the same table as the KytyPS5 protocol 2 adapter (`src/agsAdapter/launchPlan.cpp` `LanguageIndex`): `["japanese","english","french","spanish","german","italian","dutch","portuguese","russian","koreana","tchinese","schinese","finnish","swedish","danish","norwegian","polish","brazilian","-","turkish","latam","arabic","-","czech","hungarian","greek","romanian","thai","vietnamese","indonesian"]`; `"-"` never matches; case-insensitive.
  - `pub fn read_preset(path: &Path) -> Result<BTreeMap<String, String>, String>` — string values of keys not starting with `_` (as `play-hybrid.sh`).
  - `pub struct Launch<'a> { pub install: &'a Path, pub data: &'a Path, pub entry: &'a VersionEntry, pub map: &'a SettingsMap, pub available: &'a BTreeSet<String>, pub defaults: BTreeMap<String, Value>, pub screen: Option<(u32, u32)> }`
  - `pub fn plan(l: &Launch, params: &Value) -> Result<Value, String>` (the `String` is the `failed` sentence).

Rules (mirroring `scripts/play-hybrid.sh`):
- Refuse (`failed`): not `Supported` → its message (or "Choose the folder of your Astro Bot dump." / "No Astro Bot dump in this folder."); not `built` → `Build the game files first.`; a preset file missing → "The AstroPort installation is damaged. Repair it.".
- Values: `params.values` over `item_defaults` per id; a value of the wrong type or not an option takes the default; `advanced.patches` ids not in `advanced` are dropped.
- `display.fps` (from `params.standard`): integer ≥ 60 → `"60"`, else `"30"`; not an integer → `"30"`. `display.resolution` `"WxH"` (both 2 to 8192) → `KYTY_GPU_WIDTH`/`HEIGHT`; absent or bad → `screen`, else 3840x2160. `perf.low_latency` (sent from M4 because it is declared `honoured` with `applier: source`, M2-5 sheet §2.5; M1 hosts do not send it) `off` → `off`, `on`/`boost`/absent → `on`. On an M4 host the key is always present and starts at the registry default `"off"` (§2.1), so low latency is off until the owner turns it on; absent → `on` only happens on M1 hosts. New `launch.plan` params `play` (`"local"`/`"stream"`, §2.5) and the `standard` keys the adapter did not declare are ignored. `fullscreen` bool, absent → true. `language` → `ANYPS5_LANGUAGE` = `language_index`, unknown or absent → `1`. `graphics.ultrawide` counts only when `ultrawide` is available. `graphics.dlss` counts only when the device is NVIDIA (`Defaults.dlss`); elsewhere it is forced `false`, so `ASTROHYBRID_DLSS=0`.
- `controller.haptics` (100 to 400) → `APS5_PAD_VIBRATION_GAIN` = value / 100 formatted with the shortest decimal (`250` → `"2.5"`, `100` → `"1"`).
- Env, in this order, later keys replacing earlier ones: `fixed_env` (with `{run}` and `{install}` replaced by the absolute folders), then each preset file (so `KYTY_HANG_WATCHDOG` ends as the preset's `auto`, as in `play-hybrid.sh`), then `Selection.env` and the computed `KYTY_GPU_WIDTH`, `KYTY_GPU_HEIGHT`, `APS5_PAD_VIBRATION_GAIN`, `ANYPS5_LANGUAGE` (the ultrawide 3440x1440 from `Selection.env` wins over the standard resolution), then `ASTRO_PATCHES` = `<install>/patches/<entry.patches>`, `ASTRO_PATCH_ENABLE`, `ASTRO_PATCH_SKIP` from `patch_env`.
- Result: `{"argv": ["<run>/AstroBot"], "env": {...}, "cwd": "<run>", "writable": ["<run>", "<data>/saves"], "native_frames": false, "controllers": ["dualsense"], "applied": {"display.fps": <30|60>, "display.resolution": "<W>x<H>"}}`. `applied` has exactly the shape of the typed `ags_page::standard::Applied` (M2-5 sheet §1.2, §2.5: `display.fps` an integer, `display.resolution` `"WxH"`; the host builds the session config from it). `argv[0]` in `<data>/generated/` (M2-5 sheet §3.5): from M3 the host accepts it only when its canonical path is listed in `<ext>/.generated.json` with the file's SHA-256, which the host writes after every completed Extension job; otherwise the launch fails with `generated-changed`. So a run folder counts only when the build job made it (not a manual `astroport-run`), and an extension install, repair or rollback removes `.generated.json` (a rebuild, as the adapter's `release` check already asks). M1 and M2 hosts accept only the install folder (m1 sheet 2.8), so a launch there fails; nothing in the adapter changes for it.

- [ ] **Step 1: Write the failing tests** (`launch.rs` unit tests for the pure helpers; `adapter/tests/rpc.rs` for the call with a built fixture run folder)

```rust
#[test] fn languages() { assert_eq!(language_index("english"), Some(1)); assert_eq!(language_index("Koreana"), Some(9)); assert_eq!(language_index("-"), None); }

#[test] fn launch_env_matches_play_hybrid() {
    let f = Fixture::built(); let mut a = Adapter::spawn(&f); set_dump(&mut a, &f);
    let r = a.call("launch.plan", json!({"game": "PPSA21567", "standard": {"display.fps": 30, "display.resolution": "3840x2160", "fullscreen": true, "language": "english"},
        "values": {}, "capture": "native", "controllers": ["dualsense"]}))["result"].clone();
    let run = f.data.join("generated/run-1.019");
    assert_eq!(r["argv"], json!([run.join("AstroBot")]));
    assert_eq!(r["cwd"], json!(run));
    let e = &r["env"];
    assert_eq!(e["GLIBC_TUNABLES"], "glibc.rtld.optional_static_tls=0x200000");
    assert_eq!(e["KYTY_GPU_LIBRARY"], json!(run.join("libs/libkyty_gpu.so")));
    assert_eq!(e["KYTY_GPU_HOST_MODULE"], json!(run.join("libs/libAstroHybridHost.prx")));
    assert_eq!(e["LD_AUDIT"], json!(f.install.join("lib/libastro_patcher.so")));
    assert_eq!(e["ASTRO_PATCHES"], json!(f.install.join("patches/PPSA21567-1019.json")));
    assert_eq!(e["KYTY_HANG_WATCHDOG"], "auto");
    assert_eq!((e["ASTROHYBRID_FPS_LIMIT"].as_str(), e["ASTROHYBRID_LOW_LATENCY_INTERVAL_US"].as_str()), (Some("30"), Some("33333")));
    assert_eq!((e["KYTY_GPU_WIDTH"].as_str(), e["KYTY_GPU_HEIGHT"].as_str(), e["KYTY_GPU_FULLSCREEN"].as_str()), (Some("3840"), Some("2160"), Some("1")));
    assert_eq!((e["APS5_PAD_VIBRATION_GAIN"].as_str(), e["ANYPS5_LANGUAGE"].as_str(), e["KYTY_WAYLAND_NATIVE"].as_str()), (Some("2.5"), Some("1"), Some("1")));
    assert!(e["ASTRO_PATCH_ENABLE"].as_str().unwrap().contains("precise frame cap 30 fps"));
    assert!(e["ASTRO_PATCH_SKIP"].as_str().unwrap().contains("precise frame cap 60 fps"));
    assert!(e.get("LD_PRELOAD").is_none() && e.get("LD_LIBRARY_PATH").is_none());
    // DLSS on exactly when the page offers it (NVIDIA): machine-independent check
    let page = a.call("page.describe", json!({"game": "PPSA21567"}))["result"].clone();
    assert_eq!(e["ASTROHYBRID_DLSS"], if item(&page, "graphics.dlss").is_object() { "1" } else { "0" });
    assert_eq!(r["writable"], json!([run, f.data.join("saves")]));
    assert_eq!(r["applied"], json!({"display.fps": 30, "display.resolution": "3840x2160"}));
}
#[test] fn fps_outside_the_offer() {
    // 144 -> "60" and applied 60; 0, absent, "fast" -> "30" and applied 30 (Review Focus 3)
}
#[test] fn ultrawide_sets_3440x1440() { /* values {"graphics.ultrawide": true}: ENABLE contains "ultrawide:", width 3440, height 1440, applied "3440x1440" */ }
#[test] fn stale_values_take_defaults() {
    // values {"advanced.patches": ["pad.r1x", "gone.patch"], "graphics.render": "4320p", "controller.haptics": "loud"}:
    // plan succeeds; render default used; "gone.patch" ignored; SKIP contains "havok contact query" (Review Focus 5)
}
#[test] fn not_built_refused() { /* Fixture::new() (no run folder): error code "failed", message "Build the game files first." */ }
#[test] fn stale_build_refused() { /* run folder built for 1.018 while the dump is 1.019: "Build the game files first." (Review Focus 4) */ }
#[test] fn unsupported_refused() { /* 01.020.000: message "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019." */ }
```

`Fixture::built()` = `Fixture::new()` plus `<data>/generated/run-1.019/AstroBot` (empty file) and a matching `build.json` (release `env!("CARGO_PKG_VERSION")`).

- [ ] **Step 2: Run to verify they fail**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml`
Expected: the new tests FAIL (`launch.plan`: `unknown-method`; `language_index` not found).

- [ ] **Step 3: Implement `launch.rs`** and the dispatch branch.

- [ ] **Step 4: Run to verify they pass**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml`
Expected: `test result: ok` in every target.

- [ ] **Step 5: Commit**

```bash
git add adapter/src/launch.rs adapter/src/main.rs adapter/tests/common/mod.rs adapter/tests/rpc.rs
git commit -m "Adapter: launch.plan with the play-hybrid environment and the settings map" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: `job.plan build` (AGS milestone 3, M2-5 sheet §3)

**Files:**
- Modify: `adapter/src/sheet.rs`, `adapter/src/main.rs`, `adapter/tests/rpc.rs`

**Interfaces:**
- Produces: `pub fn job_plan(install: &Path, data: &Path, dump: &Path) -> Value`.
- Params (M2-5 sheet §3.2 `JobPlanParams`): `{"job": <button id>, "game": <adapter game id or absent>, "params": {}, "cleanup": bool, "resume": null | {"step", "done", "total"}}`. `job` is the id of the `button` with `job: true` that was pressed (here `"build"`, the hero item of the game page); `game` is the adapter's game id `PPSA21567`. `params` is always `{}` from M3 hosts; `cleanup` and `resume` are never set for this plan (it says `cleanup: false`, `resumable: false`, §3.4) and are ignored.
- Dispatch: `job.plan {job: "build", game: "PPSA21567"}` when `Supported` → `job_plan(...)`; other `job` → `not-found` "AstroPort has no such job."; other or absent `game` → `not-found` "AstroPort has no such game."; not `Supported` → `failed` with the detection message. The call has the host's 5 s timeout (§3.2): `detect` hashes `eboot.bin` only when the hash cache misses, which stays well under it.
- `hello` `features` gain `"jobs"` (§3.1): an M3 host then serves the `job: true` button and calls `job.plan`; M1 and M2 hosts ignore an unknown feature with a warning (m1 sheet 2.3) and drop the button as before. The `hello_answers_protocol_2` assert becomes `json!(["library", "page", "values", "jobs"])`.

Result, exact keys (M2-5 sheet §3.2 `JobPlan`; every field the sheet lists, written out):

```json
{"label": "Build game files",
 "steps": ["Check the game dump", "Unwrap the game files", "Convert for Linux", "Set up the game folder", "Record the result"],
 "argv": ["<install>/bin/astroport-run", "build", "--dump", "<dump>", "--install", "<install>", "--data", "<data>"],
 "env": {},
 "cwd": "<data>",
 "writable": ["<data>/generated", "<data>/saves"],
 "admission": {"blocks_launch": true, "pauses_while_playing": false, "runs_while_playing": false, "counts_against_downloads": false},
 "notify": {"completed": true, "failed": true},
 "changes_software": true,
 "resumable": false,
 "cleanup": false}
```

Host checks this plan passes (§3.2): `label` 1 to 80 characters; 1 to 16 `steps` of 1 to 40 characters; `argv[0]` an absolute file inside the install folder; no `AGS_` key in `env`; `cwd`/`writable` as `LaunchPlan`. Effects (§3.9, §3.2): `blocks_launch` refuses a launch of Astro Bot with `job-blocks-launch` while the build runs; `changes_software` makes it exclusive (no game starts while it runs) and refuses it from a paired device (`owner-only`), so the `build` button keeps the default `remote: false`. The host fills the button's `job_id` and, while the job runs, `disabled_reason` (§3.8), and sends `page.changed` for the game when the job starts or ends.

`sheet.rs` carries one comment line per point, each resolved one starting `// sheet §` and each still open one starting `// SHEET:`:

- `// sheet §3.2: job.plan params {job, game, params, cleanup, resume} and result JobPlan {label, steps, argv, env, cwd, writable, admission (four bools), notify, changes_software, resumable, cleanup}.`
- `// sheet §3.2: job is the id of the button with job: true that was pressed ("build"); game is the adapter game id.`
- `// sheet §3.1: hello lists "jobs"; without it the host drops the job button.`
- `// sheet §3.3: progress "step" is a 0-based index into steps (0..=4), past the end clamped to the last; astroport-run sends 0-based.`
- `// sheet §3.3: a worker exit without a final outcome or error line fails as internal "The job stopped before it finished."`
- `// sheet §3.5: argv[0] <data>/generated/run-<v>/AstroBot is launched only when listed with its SHA-256 in <ext>/.generated.json, written after a completed job (else generated-changed).`
- `// sheet §1.2, §2.2: standard is a map key -> Support {level, live, applier, values, min, max, requires, method}; level honoured|override|fixed|unsupported, live a restart word, applier source|ags.`
- `// sheet §2.1: perf.render_scale values are integer percents, so graphics.render stays an AstroPort item and the key is not declared.`
- `// sheet §2.5: perf.low_latency reaches launch.plan standard from M4 (honoured, applier source); its registry default is "off".`
- `// sheet §2.2: perf.frame_generation absent = AGS offers lsfg for local play; not declaring it is "does not refuse it".`
- `// SHEET: whether the host calls library.list again after a completed job is not in the M2-5 sheet (§3.8 sends only page.changed); installed flips on the next library.list.`
- `// SHEET: an overlay section for controller.haptics (design 5.4): M2-5 sheet §6.6 draws overlay sections with PageItemRow but gives no rule for an item shown in both the panel and the overlay, and an id may appear once per description.`
- `// SHEET: library entry with installed false: no sheet defines how an unsupported entry hides Play (design 5.1 "no Play button"); the M1 host shows its Install chrome.`

- [ ] **Step 1: Write the failing test**

```rust
#[test] fn job_plan_build_returns_the_runner() {
    let f = Fixture::new(); let mut a = Adapter::spawn(&f); set_dump(&mut a, &f);
    let r = a.call("job.plan", json!({"job": "build", "game": "PPSA21567", "params": {}, "cleanup": false, "resume": null}))["result"].clone();
    assert_eq!(r["argv"], json!([f.install.join("bin/astroport-run"), "build", "--dump", f.dump,
        "--install", f.install, "--data", f.data]));
    assert_eq!(r["steps"], json!(["Check the game dump", "Unwrap the game files", "Convert for Linux", "Set up the game folder", "Record the result"]));
    assert!(r["steps"].as_array().unwrap().iter().all(|s| (1..=40).contains(&s.as_str().unwrap().chars().count())));
    assert_eq!(r["cwd"], json!(f.data));
    assert_eq!(r["writable"], json!([f.data.join("generated"), f.data.join("saves")]));
    assert_eq!(r["admission"], json!({"blocks_launch": true, "pauses_while_playing": false, "runs_while_playing": false, "counts_against_downloads": false}));
    assert_eq!(r["notify"], json!({"completed": true, "failed": true}));
    assert_eq!((r["changes_software"].as_bool(), r["resumable"].as_bool(), r["cleanup"].as_bool()), (Some(true), Some(false), Some(false)));
    assert_eq!(a.call("job.plan", json!({"job": "other", "game": "PPSA21567", "params": {}}))["error"]["code"], "not-found");
    assert_eq!(a.call("job.plan", json!({"job": "build", "game": "CUSA00001", "params": {}}))["error"]["code"], "not-found");
    assert_eq!(a.call("hello", json!({"protocol": 2, "ags": "0.20.0", "features": ["jobs"]}))["result"]["features"],
        json!(["library", "page", "values", "jobs"]));
}
#[test] fn sheet_points_are_marked() {
    let s = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/sheet.rs")).unwrap();
    let n = |p: &str| s.lines().filter(|l| l.trim_start().starts_with(p)).count();
    assert!(n("// sheet §") >= 10);
    assert_eq!(n("// SHEET:"), 3); // the points the M2-5 sheet leaves open
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml job_plan`
Expected: FAIL with `unknown-method`.

- [ ] **Step 3: Implement `job_plan`, the dispatch branch, `"jobs"` in `hello` (and its assert in `hello_answers_protocol_2`), and the `sheet §`/`SHEET:` lines.**

- [ ] **Step 4: Run to verify it passes**

Run: `timeout -k 10 600 cargo test --manifest-path adapter/Cargo.toml`
Expected: `test result: ok` in every target.

- [ ] **Step 5: Commit**

```bash
git add adapter/src/sheet.rs adapter/src/main.rs adapter/tests/rpc.rs
git commit -m "Adapter: job.plan build returns the runner command" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Whole-crate verification

**Files:** none new.

- [ ] **Step 1: Release build, tests and lints**

Run: `timeout -k 10 900 cargo build --release --locked --manifest-path adapter/Cargo.toml && timeout -k 10 600 cargo test --locked --manifest-path adapter/Cargo.toml && timeout -k 10 600 cargo clippy --locked --manifest-path adapter/Cargo.toml --all-targets -- -D warnings`
Expected: build finished, `test result: ok` in every target, clippy no warnings.

- [ ] **Step 2: Repository rules** (after the controller applied the prelude below)

Run: `timeout -k 10 120 scripts/check-repo.sh`
Expected: `repo OK: ...`.

- [ ] **Step 3: No forbidden variables or paths**

Run: `grep -rn 'LD_PRELOAD\|LD_LIBRARY_PATH' adapter/src patches/settings.json | grep -v '^adapter/src/launch.rs:.*// ' ; grep -rlF "$HOME" adapter ags-extension.toml patches/settings.json patches/versions.json assets docs/superpowers/plans/2026-11-21-astroport-step3-adapter.md`
Expected: no output (the only mentions are the comment that says they are never set, and the test that asserts their absence; adjust the grep exclusion to those lines only).

- [ ] **Step 4: Commit** (only if Steps 1 to 3 changed something, for example a clippy fix)

```bash
git add adapter
git commit -m "Adapter: lint fixes" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Interfaces with other steps

**Install folder (step 2 produces; unchanged from the shared layout).** The adapter reads: `bin/astroport-adapter` (its own path gives `<install>`), `bin/astroport-run` (named in `job.plan`), `lib/libastro_patcher.so` (`LD_AUDIT`), `patches/PPSA21567-1018.json`, `patches/PPSA21567-1019.json`, `patches/settings.json`, `patches/versions.json`, `presets/u59-preset.json`, `presets/u59-preset.PPSA21567.json` (copies of `engines/kyty/tools/`), `assets/icon.svg`, `assets/logo.svg`, `assets/badge.svg`, `ags-extension.toml`. Requests to step 2: build the adapter with `cargo build --release --locked --manifest-path adapter/Cargo.toml` and stage `adapter/target/release/astroport-adapter` as `bin/astroport-adapter`; the manifest's `[install.source]` is step 2's block (`scripts/build-engines.sh`, the three cargo builds, `scripts/release/stage.sh {stage}`); the release asset name is `astroport-{version}-linux-{arch}.tar.zst` with `{arch}` = `x86_64` and `{asset}.sha256` beside it; the manifest `version` and `adapter/Cargo.toml` `version` move together (a test pins them equal).

**Runner CLI (step 4 implements; fixed here).**
- `astroport-run build --dump <abs dump folder> --install <abs install> --data <abs data>` (no `--version`: the runner detects it from the dump); cwd `<data>`; exit 0 on success, non-zero on failure.
- Progress (M2-5 sheet §3.3): UTF-8 JSON lines, each at most 4096 bytes, on the fd in `AGS_JOB_PROGRESS_FD` (AGS also sets `AGS_JOB_ID`): `{"step": <0-based index into the five steps, 0..=4>, "done": n, "total": n, "unit": "files", "message": "<one line, cut to 200>"}`, then `{"outcome": "completed"}` or `{"error": {"category": "<temporary|account|storage|integrity|unsupported|conflict|cancelled|internal>", "message": "<one sentence, at most 200 characters>"}}`. The sheet fixes the step index as 0-based (a value past the end is clamped to the last), so the earlier 1-based numbering (1..=5) changes to 0..=4; the five steps and their order do not change. An exit without a final `outcome` or `error` line is a failure, `internal` (also with exit 0). Cancel is SIGTERM to the runner's process group, SIGKILL after 10 s (§3.4); `cleanup: false` means AGS runs no cleanup pass, so the runner's temp-folder-then-rename keeps a cancelled build harmless. The five steps, in order (step 4 owns them, `runner/src/progress.rs` `STEPS`): `Check the game dump`, `Unwrap the game files`, `Convert for Linux`, `Set up the game folder`, `Record the result`. Missing Noto fonts: error message `Install the Noto fonts.`.
- Run folder `<data>/generated/run-<version>/` (`run-1.018`, `run-1.019`) that the adapter consumes: `AstroBot` (the relinked game, executable, that name), `app0/` (with `args.txt` written from the dump's `args.txt`), `libs/libkyty_gpu.so`, `libs/libAstroHybridHost.prx`, `libs/*.prx`, the save folders linked to `<data>/saves/` (layout of the links is step 4's), and last `build.json`, written only when everything else is in place (build into a temp folder, rename at the end):

```json
{"format": 1, "version": "1.019", "eboot_sha256": "<lower-case hex of the dump's eboot.bin>", "release": "<AstroPort version from ags-extension.toml>"}
```

  The adapter calls the run folder built when `AstroBot` is a regular file (not a link) and all four fields match the current dump and the installed version; any AstroPort update therefore offers a rebuild (design 5.2 "rebuilt after an update", simplified to every update).
- Launchable from M3 (M2-5 sheet §3.5): after the job completes AGS scans `<data>/generated/` (no link followed, regular files with an execute bit, at most 4096 files, both run folders together) and records each one's SHA-256 in `<ext>/.generated.json`; `launch.plan`'s `argv[0]` `<run>/AstroBot` must be one of them. So `AstroBot` is a regular executable file, not a link; other files in `generated/` keep no execute bit (game files copied from the dump included), so the 4096 limit never hides `AstroBot`; and nothing rewrites `AstroBot` after the job (a changed hash fails the launch with `generated-changed`).

**`patches/versions.json`** (schema): `{"title_id": string, "versions": [{"version": "1.019", "content_version": "01.019.000", "eboot_sha256": hex string or null (version-only match), "patches": "<file name in patches/>"}]}`. Step 4 reads it for the same check in "Check the game dump".

**`patches/settings.json`** (schema): `format` (1); `fixed_env` (name → value, `{run}` and `{install}` replaced with absolute folders); `presets` (paths with the same placeholders, applied in order after `fixed_env`); `patches` (patch id → lower-case substring of exactly one mod name, no commas); `settings` (setting key → `values` → value key → `{on: [patch ids], off: [patch ids], env: {name: value}}`, every value of a setting naming the same set of ids); `advanced` (`[{id, label, default}]`, the toggle list). Step 5 uses `settings["display.fps"].values` to know which frame-cap group members to swap live, and `patches` for their match strings.

**Setting ids and values** (game page, scope `game_on_device`): `graphics.render` (`1080p`, `1368p`, `1872p`, `2160p`), `graphics.lighting` (`fast`, `original`), `graphics.detail` (`high`, `medium`, `low`), `graphics.textures` (`sharp`, `balanced`, `soft`), `graphics.ultrawide` (bool, 1.019 only), `graphics.dlss` (bool, NVIDIA GPUs only; `ASTROHYBRID_DLSS`), `controller.haptics` (integer percent 100 to 400; gain = value / 100), `controller.rumble` (bool), `game.skip_intro` (bool), `advanced.patches` (array of `advanced` ids). Extension page, scope `machine`: `dump_folder` (absolute path or `""`). Standard keys read from `launch.plan`: `display.fps`, `display.resolution`, `fullscreen`, `language`, `perf.low_latency` (M4, M2-5 sheet §2.5).

**Environment set by `launch.plan`:** every `fixed_env` name above, the preset variables, `ASTROHYBRID_FPS_LIMIT`, `ASTROHYBRID_LOW_LATENCY_INTERVAL_US`, `ASTROHYBRID_LOW_LATENCY`, `ASTROHYBRID_DUALSENSE_RUMBLE`, `ASTROHYBRID_DLSS`, `KYTY_GPU_WIDTH`, `KYTY_GPU_HEIGHT`, `KYTY_GPU_FULLSCREEN`, `APS5_PAD_VIBRATION_GAIN`, `ANYPS5_LANGUAGE`, `ASTRO_PATCHES`, `ASTRO_PATCH_ENABLE`, `ASTRO_PATCH_SKIP`, `ASTRO_PATCH_PROCESS`, `LD_AUDIT`. Never `LD_PRELOAD`, never `LD_LIBRARY_PATH`. Not set by the adapter (AGS sets them, step 5 reads them): `AGS_SESSION_PROTOCOL` and the session socket variables, `AGS_CONTROLLER_AUDIO_SINK`.

**Step 5 (live):** the session socket v2 messages are those of M2-5 sheet §6.1 (`AGS_SESSION_SOCKET`, `AGS_SESSION_PROTOCOL=2`; the renderer's `hello` has `"version": 2`, or `"frames": false` for a frame-less producer). `settings.apply {id, values}` values use the game page item ids whose `restart` is `live` (`controller.haptics` as integer percent) and the standard keys (`display.fps`, `display.resolution`) whose support has `live: "live"`; the two share one namespace (§1.4). The renderer answers `settings.applied {id, applied, values, refused}`: `applied` the typed standard keys (`ags_page::standard::Applied`), `values` the item ids it took, `refused` key to one line of at most 80 characters. The adapter declares every row `next_launch` today; when step 5's live paths land, the controller changes `controller.haptics` `restart` to `live` in `adapter/src/page.rs` and `display.fps` and `display.resolution` `live` to `live` in `adapter/src/sheet.rs` (this plan's files; §6.1 sends only keys whose live class is `live`, so the frame-cap swap needs `display.fps` live too), so no setting claims to be live before it is. AGS itself forces `display.resolution` and `display.fps` back to `next_launch` under Gamescope or layer capture (§2.4 step 3), so the adapter declares nothing per capture. A further controller edit at the same time: `hello` adds `live` (§6.2: without it AGS binds no session socket for local play), `native_frames` and `metrics` to `features` and sets `metrics` to step 5's catalog (`[{"key":"game_fps","label":"Game FPS","unit":"fps","type":"number","min_interval_ms":1000},{"key":"frame_time_ms","label":"Frame time","unit":"ms","type":"number","min_interval_ms":1000}]`), and `launch.plan` returns `"native_frames": true`; the `hello_answers_protocol_2` features assert and a `native_frames` assert in `launch_env_matches_play_hybrid` change with it (not done earlier because the M1 sheet does not say the host falls back to capture when a renderer without the socket client declares `native_frames`).

**Overlay (design 5.4), owned by this step:** a named follow-up for the controller with step 5's live edits: the game page adds an overlay section (`surface: "overlay"`, no `tab`, a title, m1 sheet 1.2.2) with `controller.haptics` (`restart: "live"`, integer percent 100 to 400, the same id step 5's renderer reads). The frame-rate cap and output resolution are not overlay items: AGS draws Resolution and Frame rate in its overlay from the served `standard[key].live` (M2-5 sheet §6.6). The overlay draws page items not `live` read-only with "Next start" (§6.6). How `controller.haptics` appears both in the panel and in the overlay is still open (the Task 8 overlay SHEET line). Step 5 adds no overlay item of its own.

**Socket messages:** none sent or received by the adapter (the session socket is the renderer's, step 5).

## Deviations from the design, for the owner

- Language is taken only from AGS's standard `language` (design.md "one place per choice"); the Game section has no second language row.
- Hide cursor: the port has no variable for it; Kyty hides the cursor in fullscreen (`kytyGpuBridge.cpp`), so `display.hide_cursor` is not mapped.
- Overlay haptic strength and every "live" claim wait for AGS milestone 5 and step 5 (the overlay SHEET line in Task 8; the live and overlay follow-ups in "Interfaces with other steps").
- Low latency on an M4 host starts off: AGS always sends `perf.low_latency` with the registry default `"off"` (M2-5 sheet §2.1, §2.5), where `play-hybrid.sh` and M1 hosts run it on.
- The opt-in dump test reads `ASTROPORT_TEST_DUMP` (the 1.019 dump folder) because `src-1019` holds only an unwrapped `eboot.elf` and no `sce_sys/param.json`.

## Prelude and wrap-up needs

For the controller, applied once (this plan never edits these files):

- `.gitignore` (whitelist): add `!/adapter/`, `!/assets/`, `!/ags-extension.toml`, and the ignore `/adapter/target/`. Needed before Task 1's commit.
- `scripts/check-repo.sh`: rule 7 (missing tracked files) gains `ags-extension.toml adapter/Cargo.toml adapter/Cargo.lock adapter/src/main.rs patches/settings.json patches/versions.json assets/icon.svg assets/logo.svg assets/badge.svg`; "should be ignored" gains `adapter/target/x`; "should not be ignored" gains `adapter/src/main.rs ags-extension.toml`.
- `README.md`: a short "Install through AGS" part (add the repository URL in AGS, set "Astro Bot dump folder", press "Build game files"); supported versions 1.018 (less tested, matched by version only) and 1.019.
- `NOTICE.md`: nothing new (the adapter is the port's own code; the language table is a list of names shared with the owner's KytyPS5-AGS adapter).
- Owner questions: replace the placeholder SVGs in `assets/`; confirm whether `source_sha256` in `PPSA21567-1018.json` is the 1.018 `eboot.bin` hash (then it goes into `versions.json`); the 60 FPS play tests for Decouple, the sleep removal and the delta-time cap.
