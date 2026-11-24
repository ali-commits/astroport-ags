# Functional specification: 14 engine features to re-create

Status: clean-room specification. It says what each feature must do and why. It contains no code
from the third-party patches; only names of functions, files and environment variables, and numeric
constants that are tuning facts. The implementer must not look at the original patches.

Conventions used below:

- **[read]**: taken from the patch text or its commit message. **[inferred]**: my conclusion, not stated
  in the patch. **[tree]**: checked in our current tree (`engines/kyty`, `engines/anyps5`) on 2026-11-24.
- "Upstream" means KytyPS5 (Jetsku) or AnyPS5 (boykopovar) at the base named in each `UPSTREAM.md`.
  "Third-party" means the AstroHybrid additions (bridge `src/hybrid/`, `frameGen.cpp`, `dlss.cpp`,
  `hybridDlss.cpp`, `lowLatency.cpp`, FSR upscaling `fsr.inc`), which are to be removed from our tree.
- The third-party environment variables use the prefix `ASTROHYBRID_`. Our implementation should choose
  its own names; the names are given here only so that behaviour can be compared. Defaults and ranges
  are part of the required behaviour, names are not.
- Guest errno numbers are the PS5 (FreeBSD) ones: 2 ENOENT, 5 EIO, 9 EBADF, 17 EEXIST, 21 EISDIR,
  22 EINVAL, 30 EROFS, 84 EOVERFLOW.

Dependency overview:

| Feature | Depends on |
|---|---|
| A1 odx lookup (0028) | upstream AnyPS5 `app0.zar` layer (ZArchive) |
| A2 archive stdio (0029) | A1 for one test only; upstream archive descriptor layer in libkernel |
| A3 inline archive flag (0033) | A2 |
| A4 audio paths (0034) | none |
| A5 avplayer notify (0035) | K9; a renderer export (third-party bridge on Windows) |
| K1 gyro offset (0054) | none |
| K2 FSR frame generation (0055) | third-party frame-generation hooks, FSR upscaling loader, AMD FSR SDK |
| K3 icon fallback (0056) | none |
| K4 pipeline list title/cross/eboot (0057) | upstream pipeline list |
| K5 evict before spill (0058) | a memory-pressure callback that is not in our tree |
| K6 validator option (0059) | none; already in our tree |
| K7 hidden window during prepare (0060) | upstream "prepare shaders" run |
| K8 cooldown and draw guard (0061) | K5 for the cooldown part |
| K9 60 FPS during video (0062) | A5; third-party frame limiter |

---

# Part A: AnyPS5 (paths relative to `engines/anyps5`)

## A1. Archived app0: exact lookup first, `.odx` falls back to `.odxb` (patch 0028)

### Purpose

The game's files can be shipped as one ZArchive file `app0.zar` beside the `app0/` folder, as a
read-only layer under that folder. Two problems appear with real archives:

1. The game asks for asset files ending in `.odx`, but packed builds hold the binary form ending in
   `.odxb`. Without a fallback the file is "not found" and the asset is missing (the reported case is
   an icon, `hero_respawn_icon_01`). [read] That `.odxb` is the compiled form of `.odx` is [inferred].
2. An archive built from the parent folder has every entry under `app0/...` instead of at the root.
   Lookups then silently fail for every file, and nothing tells the user why.

Recognise it by: missing textures/icons with an archive present but not with loose files; or a game
that finds no file at all in a freshly made archive.

### Behaviour required

Archive layout check, done once when the archive is opened:

- Archive entry names are relative to the archive root: guest `/app0/sce_sys/param.sfx` is the archive
  entry `sce_sys/param.sfx`.
- If the archive root holds exactly one entry, that entry is a directory, and its name equals `app0`
  ignoring ASCII case, opening fails with an error. The message must say that the archive has a
  wrapped `app0/` root, that the contents of app0 must be stored at the archive root without an `app0/`
  prefix, and must contain the archive's path.
- If the root directory cannot be read, opening fails with an error saying the archive is damaged.
- A directory named `app0` beside other root entries is legal (`/app0/app0/Nested.bin` must resolve).
- An unreadable or non-ZArchive `app0.zar` already fails with an error upstream; keep that.

Lookup of one host path under the app0 folder:

1. Look up the exact archive-relative name (ZArchive's own matching is ASCII case-insensitive). If
   found, use it, whether file or directory. An exact `.odx` entry always wins over an `.odxb` one,
   also when the exact entry is a directory.
2. Only if the exact name is missing, try the fallback, and only when all of these hold:
   - the name ends in `.odx`, compared ignoring ASCII case, as the terminal extension of the last
     path component (not `.odx.bin`, not a middle component, not a path with a trailing separator);
   - nothing exists on disk at the requested host path (a loose file or folder of that name, or any
     error other than "does not exist" when checking, means: no fallback);
   - the archive holds the name with `b` appended, and that entry is a regular file (a directory
     named `x.odxb` is never used);
   - nothing on disk shadows the `.odxb` name: neither an entry of exactly that name nor an entry in
     the same host folder whose name equals it ignoring ASCII case, of any type. A failure to list
     the folder, other than "does not exist", also counts as shadowed.
3. The result reports the `.odxb` entry's handle, size and "is a file". Reads go through the normal
   archive read path, so a damaged compressed block gives the same "archive is damaged" error as for
   any other file.

Things that must not change:

- Directory listings show real names only (`.odxb`), never the alias.
- No aliasing without an archive: a loose `x.odxb` does not answer a request for `x.odx`.
- No aliasing outside the app0 mount (`/Assets/...`, `/app0x/...`).
- The generic path resolver still refuses archive-only paths for callers that are not archive-aware,
  including aliased ones.
- A loose file always has priority over the archive, for the exact name and for the alias.
- The "archive only" test (path is in the archive and nothing is on disk) uses the same "exists on
  disk" rule as above, so both stay consistent.

Threading: lookups are read-only on an archive opened once; no new locking beyond what the upstream
archive layer has. [inferred]

### Where it hooks in

- `core/libs/prx/libc/src/AppArchive.cpp`: the function that opens the archive (static `Opened`) and
  `AppArchiveLookup_nid_no_patch`, `AppArchiveOnly_nid_no_patch`. Upstream AnyPS5. [tree: file and the
  case-folding helper `SameComponent` exist; the odx fallback and the wrapped-root check do not.]
- Tests: `core/libs/tests/GuestAppArchive.cpp` and `AppArchiveFixture.hpp` (upstream, exist).

### Dependencies

- ZArchive reader (`3rdparty/ZArchive`, upstream; "MIT No Attribution" licence [tree]).
- None on other features in this list.

### Platform notes

None. The disk checks must use case-insensitive comparison explicitly, because Linux file systems
are case-sensitive while the archive is not.

### Size / risk / test

Small. Risk: an alias that hides a loose override; the shadow rules above exist to prevent that.

Test with a real archive written by the ZArchive writer: root-relative `param.sfx` found; request
`.../X.ODX` returns the `.odxb` entry with the right size and bytes; exact `.odx` wins; directories
never aliased; middle components and other extensions untouched; each loose-file shadow case; no
archive present; a damaged block in the aliased file gives the damage error; a wrapped archive gives
the layout error with the path in it.

---

## A2. Read-only stdio streams on archived app0 files (patch 0029)

### Purpose

Upstream AnyPS5 reads archived files only through the archive-aware libkernel calls (`sceKernelOpen`,
read, lseek). The C stdio family (`fopen`, `fread`, `fgets`, `fscanf`, ...) throws for a path that
exists only in `app0.zar`, and `fdopen` of an archived descriptor throws. A game that reads any file
with stdio therefore stops at start with "not implemented" as soon as its files are packed.
Reproduce: pack app0, remove the loose copy of a file the game opens with `fopen`.

### Model

- An **archive stream** is a guest `FILE` whose bytes come from the archive through the existing
  per-descriptor archive cursor in libkernel (`AppArchiveAttach/Share/Detach`,
  `AppArchiveDescriptor`, `AppArchiveDescriptorRead`, `AppArchiveDescriptorSeek`). There is one
  position per open archive description, shared by the stream, the kernel descriptor and its `dup`
  copies.
- The stream still owns a host `FILE` object, opened on the null device, used only for three things:
  to own a real descriptor number (so `fileno` works and the number can be passed to kernel calls),
  for its lifetime, and for its lock. No byte is ever read from or written to that host stream.
- No read-ahead: an archive stream is unbuffered, apart from one byte of push-back. This is what makes
  the shared position exact after every stdio call.
- Files are never extracted to disk.

### Loose files take priority

`fopen`/`freopen` first resolve the guest path with the archive-aware variant of the resolver (so an
archive-only path does not throw). The archive is used only when the path is "archive only": present
in the archive (A1 rules included) and nothing on disk at that host path. In every other case the
existing native code path runs unchanged, with a normal host stream. A path missing in both places
fails with errno 2 as before. Without an archive nothing changes.

### "Guest FILE prefix" preservation

The game's code is compiled against the console's `FILE` layout and reads some fields directly
(inline `getc`, `feof`, `ferror` macros). AnyPS5's `FileStream` object therefore begins with a
fixed-layout block, `GuestFilePrefix` (flags at byte 16, descriptor at byte 18, buffer pointer at
byte 24, and so on; the object is 256 bytes as the guest sees it). Required for an archive stream:

- position pointer null, read-remaining 0, write-remaining 0, buffer null, buffer size 0. This forces
  the guest's inline fast paths to call into the library for every character, because there is
  never buffered data;
- the flags field has bit value 4 set while the stream is an archive stream (a plain open stream has
  0x10) [read; the meaning of bit 4 as "read mode, unbuffered" is inferred]; the end-of-file bit
  (0x20) and error bit (0x40) are published into the same field exactly as for native streams;
- every byte of the 256 after the prefix structure, as the guest sees them, stays zero;
- the descriptor field is filled by `fileno` as for native streams.

### Operations that must work

Opening and closing:

- `fopen(path, mode)` on an archive-only path: mode with exclusive flag fails with errno 17; any
  writing mode (`w`, `a`, `+`) fails with errno 30; a directory fails with errno 21; otherwise
  returns an archive stream at position 0.
- `fdopen(fd, mode)`: negative descriptor fails with errno 9; invalid mode errno 22; an archived
  descriptor with a writing mode fails with errno 9; with a reading mode it returns an archive stream
  that shares the descriptor's current position (bytes already read through the kernel are skipped,
  and later kernel `lseek` on a `dup` sees the stream's reads).
- `freopen(path, mode, stream)`: if the stream is an archive stream or the new path is archive-only,
  the new target is opened first, the old stream is closed (or detached, for a host standard stream),
  and the same guest `FILE` object adopts the new one, keeping its kind: archive, random device, or
  directory stream. If the new open fails, the old stream is closed, a dynamically allocated stream
  object is freed, errno is that of the failed open, and null is returned. Reopening an archive
  stream onto a loose file gives a normal native stream, and the archive attachment of the old
  descriptor is released.
- `fclose`: releases the archive attachment of the descriptor, then closes the host stream. After
  close the descriptor number is no longer an archive descriptor. Other `dup` copies keep working.

Reading:

- `fread(buf, size, count)`: returns whole elements read; a partial last element is consumed but not
  counted (11 bytes read as 4 elements of 2 gives 4, then 1). `size*count` overflow sets the error
  flag, errno 22, returns 0, position unchanged. `size` or `count` 0 returns 0 without changing flags.
- `fgetc`, `getc`, `getc_unlocked`, the guest's refill entry (`__srget`): one byte as unsigned, or EOF.
- `fgets(buf, n)`: `n <= 0` fails with errno 22 and sets the error flag; `n == 1` stores an empty
  string and reads nothing; otherwise reads up to `n-1` bytes, stops after a newline, terminates the
  string; returns null if nothing was read or if the error flag is set; a last line without newline
  is returned and sets end-of-file. The destination must be checked as writable guest memory.
- `ungetc` (and `ungetwc`, which forwards to it): exactly one byte of push-back. A second push-back
  before a read, or pushing EOF, returns EOF. A push-back clears end-of-file. `fpurge` discards it.
- `fscanf`, `vfscanf`, `fscanf_s` and their `v` forms, narrow only: see "Scanning" below.

Position:

- `ftell`, `ftello`, `fgetpos`: the shared cursor position, minus 1 while a pushed-back byte is
  pending. A position above the signed 64-bit range fails with errno 84.
- `fseek`, `fseeko`, `fsetpos`, `rewind`: origin must be SET, CUR or END, else errno 22. CUR is
  relative to the logical position (so the pending push-back byte is accounted for; the most
  negative 64-bit offset with a pending byte fails with errno 22). Seeking beyond the end is allowed
  and the next read gives EOF. A resulting negative position, or an overflow, fails with errno 22 and
  leaves the position unchanged. A successful seek discards the push-back byte and clears
  end-of-file. `rewind` also clears the error flag.
- `fflush`: succeeds; it discards the push-back byte (position stays the logical one) and, unlike a
  seek, keeps the end-of-file flag as it was.

Status:

- End-of-file is set when a read returns fewer bytes than asked, or a peek/read finds no byte. It is
  not set at open of an empty file, and not set by reading exactly up to the end.
- `feof`, `ferror` and their unlocked forms report the flags; `clearerr` clears both.
- A read from a damaged compressed block (the archive layer throws) must not throw out of stdio: the
  call returns what it had (0 bytes / EOF), sets the error flag and errno 5, does **not** set
  end-of-file, and leaves the position where it was. A descriptor that is no longer an archive
  descriptor gives errno 9 and the error flag.

Read-only refusals:

- Every writing call (`fwrite` with non-zero size and count, `fputc`, `putc`, `putc_unlocked`,
  `fputs`, `fprintf`/`vfprintf`, the secure print forms) returns its failure value (0 or EOF/-1), sets
  errno 9 and the stream's error flag, and does not move the position.

Buffering and the rest:

- `setvbuf` with the unbuffered mode and `setbuf(stream, null)` succeed. Full or line buffering is
  refused with a "not implemented" error (thrown, as AnyPS5 does for unimplemented behaviour). An
  invalid mode value fails with errno 22.
- `fileno` and `fileno_unlocked` return the real descriptor number.
- Any stdio function that was not adapted and needs the host stream's bytes (the accessor
  `GetNativeStream`) must throw "not implemented on an app0.zar stream", never read the null device.
  Wide-character scanning (`vfwscanf`, `vfwscanf_s`) is refused the same way.

### Scanning (narrow `fscanf` family)

Input is consumed one byte at a time from the stream with one byte of look-ahead, so that after the
call the stream position is exactly after the last consumed byte, and equals the kernel position.

- Supported: literals and white space in the format; `%%`; assignment suppression `*`; a field width;
  conversions `d i o u x X p`, `a A e E f F g G`, `s`, `c`, `[...]` (with `^`, a leading `]`, and
  ranges), and `n`. Length modifiers `hh h l ll j z t` for integers and `n`; none, `l` or `L` for
  floating point; none for `s c [`.
- `%n` stores the number of bytes consumed so far, in the size chosen by its length modifier.
- Numeric fields: collect the longest prefix that can be a number of that kind (sign, `0x` prefix,
  digits of the base, fraction, exponent, `inf`/`infinity`/`nan`, `(nil)` for `%p`), limited by the
  width, then convert that text. If the collected text is not a complete number (for example `100e`
  followed by `X`), the conversion fails, returns the count so far, and the consumed bytes stay
  consumed (position 4 in that example).
- A numeric field longer than 4096 bytes is refused with "not implemented" after consuming 4096 bytes.
  Text fields (`s`, `[`, `c`) have no such limit.
- Refused before any input is consumed, with "not implemented": wide conversions (`ls`, `lc`, `l[`,
  `S`, `C`), positional arguments (`%1$d`), allocation (`%ms`), unknown conversions, an unterminated
  scanset, a width of zero, a width above INT_MAX, unsupported length modifiers, floating-point
  conversions when the host locale's decimal point is not `.`, and `%Lf` on Windows.
- Return value: number of assigned fields; EOF when input ends, or a read error occurs, before the
  first conversion. A read error also sets the error flag and errno 5, without end-of-file.
- Secure forms take a buffer capacity after each `s c [` argument; if the field does not fit, the
  buffer's first byte is set to zero and scanning stops.
- All stores into guest memory (fields, `%n`, terminators) must first check that the destination
  range is writable guest memory and throw if not, as the patch does with the guest arena's write
  guard. [read]
- The unsecured `fscanf` entry is reduced to a call of `vfscanf`. [read]
- Number conversion uses the host C library on the collected text, so rounding and overflow follow
  the host, not the console. [read: stated as known debt]

### Backend transitions and threading

- A stream's kind is decided by its descriptor, at each call: if the game replaces the stream's
  descriptor with `dup2` so that it now refers to an archived file, the stream becomes an archive
  stream (push-back and end-of-file reset, prefix flags to the archive value); if `dup2` puts a
  native file there, it becomes a native stream again (prefix flags back to plain open). Position is
  the descriptor's.
- Every adapted call holds the stream's lock (the host `FILE` lock, `flockfile` on Linux) for its
  whole duration; the `_unlocked` forms do not take it. The kind check itself takes the lock, so the
  lock must be recursive (it is, for `flockfile`).
- Four threads calling `fgetc` on one archive stream until EOF must together receive every byte of
  the file exactly once, with no error flag, and the final position equal to the file size.

### Where it hooks in

All upstream AnyPS5 files, in `core/libs/prx/libc/`:

- `include/FileStream.hpp`: class `FileStream` (new per-stream archive state: descriptor, push-back,
  end flag; status publishing in `SyncStatus`/`SyncUnlockedStatus`; `ClearError`, `Close`, `Detach`,
  the reset in the adopt path; a new "release the host handle without closing" operation for
  `freopen`), `GetNativeStream`, and a small scoped stream lock.
- `src/FileAndHeap.cpp`: `fopen`, `fdopen`, `freopen`, `fread`, `fwrite`, `fseeko`, `ftello`, `fputs`,
  `fflush`.
- `src/StandardStreams.cpp`: `fgetc`, `getc_unlocked`, `ungetc`, `ungetwc`, `fgets`, `feof`, `ferror`,
  `clearerr`, `fileno`, `setvbuf`, `fputc`, `putc_unlocked`.
- `src/Formatting.cpp`: the scanning template `ScanSource` gets a new stream-backed source; `fscanf`,
  `vfscanf`, `vfprintf`, the secure writer.
- `src/ScanningWide.cpp`, `src/StreamPurge.cpp`.
- New files for the archive stream operations and the incremental field scanner.
- `core/libs/prx/libkernel/File/src/ArchivedFile.cpp` (upstream) already attaches and shares the
  archive cursor per descriptor; `AppArchiveRefuseDescriptor` is no longer used by `fdopen`.
- `ResolvePathError_nid_no_patch` (`include/FilesystemError.hpp`) is called with its existing
  `archived` flag set. [tree: the parameter exists]
- `docs/dev/debt/libc.md`: the debt note for `app0.zar` should be updated to list what is adapted and
  what still throws (`stat`, `access`, `opendir` of archive-only paths remain refused).

### Dependencies

- Upstream archive layer and ZArchive. A1 only for the test that opens an aliased `.odx` through
  `fopen`.
- A3 fixes a link problem in this feature; implement A2 with A3's rule from the start.

### Platform notes

The patch has Windows branches (`_fileno`, `_lock_file`, `NUL`, a Windows-only directory-stream
case). Linux needs: `/dev/null`, `fileno`, `flockfile`/`funlockfile`. The patch's own note says the
Windows runtime branches are unverified; the Linux side is the one the tests exercised. [read]

### Size / risk / test

Large (about 800 lines of logic plus about 400 of tests in the patch). Risks: the guest reading
prefix fields directly (layout must be exact); position drift between stdio and kernel descriptor;
deadlock from the kind check taking the stream lock inside a locked call; scanning differences from
the console.

Test (three modes, each on a fresh folder): with an archive: reads, element counting, all position
calls, push-back, status flags, each refusal with its errno, the prefix layout, lines, the scanning
cases above, shared descriptors with `dup`/`dup2`/`fdopen`, `freopen` in every direction, and the
four-thread read. Without an archive: loose file works, archive-only path gives errno 2. With a
damaged archive: `fread`, `fgetc`, `fscanf` give errno 5, error flag, no end-of-file, position 0.

---

## A3. Archive flag readable without an exported method (patch 0033)

### Purpose

After A2, `libSceLibcInternal` (a second library that includes the `FileStream` header) failed to
link or load, because inline code in the header (`GetNativeStream`, the status publishing functions)
called the out-of-line method that asks "is this an archive stream?", and that method is defined in
`libc` and not exported from it. Symptom: unresolved symbol for `FileStream::IsArchive` when building
or loading `libSceLibcInternal`. [read]

### Behaviour required

- Provide an inline, side-effect-free query that answers from the stream's own cached state (the
  cached archive descriptor is set), with no call into the archive layer and no lock.
- All inline code in the header that other libraries compile must use this query: the native-stream
  accessor's refusal, and both status publishing functions.
- The full check (which asks the archive layer and updates the cached state after a `dup2`) stays
  out of line and is used only inside `libc`.
- Consequence to accept: the inline query can be stale until the next adapted call refreshes the
  cache. It is used only to choose which status to publish and to refuse native access. [inferred]

### Where it hooks in

`core/libs/prx/libc/include/FileStream.hpp` (upstream file; the archive members come from A2).

### Dependencies

A2. Alternative with the same effect: export the method from `libc`; the inline query is smaller.

### Platform notes

The reported failure is a Windows DLL import. On Linux the same split matters if `libc` is built with
hidden visibility. [inferred]

### Size / risk / test

Small. Test: build and load `libSceLibcInternal`; the A2 tests still pass.

---

## A4. libSceAudioPropagation: accept rays and audio paths the stub does not model (patch 0034)

### Purpose

AnyPS5's `libSceAudioPropagation` is a "no propagation" stub. When the game submits a non-empty set
of rays or audio paths, a guard throws ("rays or audio paths are outside the no-propagation model")
and the game terminates. Reported in the level "Slo-Mo Casino" (Serpent Starway, world 3). [read]

### Behaviour required

- A call that submits a non-zero count of rays or audio paths must succeed (return 0) and ignore the
  data. No sound propagation is computed; sources play as they do without it.
- Argument validation that exists today stays: a null pointer with a non-zero count is still an
  error; the system handle is still checked.
- Suggested addition, not in the patch: log the first occurrence once, so that the missing
  simulation is visible in a log. The patch simply turns the guard into a no-op. [inferred]

### Where it hooks in

`core/libs/prx/libSceAudioPropagation/Export.cpp`, helper `requireNoPropagation` and its callers.
[tree: one caller remains, `sceAudioPropagationSystemSetRays`; an earlier change of ours already
accepts path calculation without rays and audio path entries.] The library is upstream AnyPS5; the
file has carried changes of ours.

### Dependencies

None.

### Platform notes

None.

### Size / risk / test

Small. Risk: none for stability; audio in that level lacks occlusion/reverb paths. Test: unit test
calling the function with a non-zero count and a valid pointer returns 0
(`core/libs/tests/GuestAudioPropagation.cpp` exists); play the named level past the crash point.

---

## A5. libSceAvPlayer tells the renderer while video is playing (patch 0035)

### Purpose

Supplies the signal that K9 consumes. In-game videos (the 60 FPS studio intro) advance one video
frame per display flip while audio runs in real time; with a frame limit other than 60 the video
runs slow or fast against its sound. The player must tell the renderer exactly when a video is
actively being shown.

### Behaviour required

- Each player object has a boolean "counts as active". A process-wide atomic counter holds the number
  of active players.
- A player becomes active when a call that fetches a video frame returns a frame **and** the player
  is in the playing state. It does not become active on start alone. [read]
- A player becomes inactive on: stop, pause, reaching end of file, an error state being entered, and
  destruction of the player.
- Setting the same value twice does nothing. The renderer is notified only on the counter's edges:
  0 to 1 sends "active", 1 to 0 sends "inactive".
- The per-player flag changes only under the player's own mutex; the counter is atomic.
- The notification is a call to one exported renderer function taking an integer (1 active, 0
  inactive); in the patch it is `KytyGpuSetVideoPlaybackActive`. It is looked up at run time by name
  in the already-loaded renderer library, the result is cached, and if the library or the symbol is
  absent nothing happens (a build without the renderer must keep working). A failed lookup is retried
  on the next notification. [read]
- Note: the frame-fetch entry with the extended frame structure sets the flag; whether the plain
  variant forwards to it should be checked so both set it. [inferred]

### Where it hooks in

- `core/libs/prx/libSceAvPlayer/include/AvPlayer.hpp`, class `Player`; `src/Player.cpp`: `~Player`,
  `deliver` (error), `checkEndOfFile`, `stopLocked`, `Pause`, `GetVideoData`. Upstream AnyPS5.

### Dependencies

- K9 (the receiver). The export lives in the third-party renderer bridge
  (`src/hybrid/kytyGpuBridge.cpp` and, on Windows, the export list `kyty_gpu.def`); after the bridge
  is replaced, our own renderer interface must offer the same one-integer call.

### Platform notes

The patch implements the lookup for Windows only (module handle of `kyty_gpu.dll`, then the symbol);
on other systems it is an empty function. Linux needs a symbol lookup in the global scope
(`dlsym` with the default handle), which is the pattern our tree already uses for the pad-audio
exports in `libSceAudioOut/src/AudioOut2PadMix.cpp`. [tree] The export must have default visibility
in `libkyty_gpu.so`.

### Size / risk / test

Small. Risk: a flag left on (the game stays at 60 FPS); every exit path above must clear it.
Test: unit test with a fake notification sink: two players, check edges only; play, fetch frame,
pause, resume, end of file, destroy while active. In game: the limiter log of K9.

---

# Part K: Kyty (paths relative to `engines/kyty`)

## K1. Gyro zero-rate offset learned at rest (patch 0054)

### Purpose

Some controllers report a constant angular velocity while lying still. SDL's Nintendo Switch driver
reads the pad's factory zero-rate offset but never subtracts it, and nothing later does. A genuine
Switch Pro Controller on a table measured +5.0, -0.9, -1.9 deg/s with 0.05 deg/s noise. The game sees
a constant rotation: motion-controlled aiming drifts up/down while left/right is fine. [read]
Recognise it by: slow drift of anything gyro-controlled while the pad lies on a table.

### Behaviour required

A small estimator per controller, fed with every gyro sample (rad/s), the most recent accelerometer
reading (in G), and the sample's sensor timestamp (microseconds):

- Gravity is smoothed with an exponential filter: the newest accelerometer reading has weight 0.02
  per sample; the first sample initialises the filter. The filter counts as settled after 50 samples
  (about 0.25 s at 200 Hz). No rest window is used before it has settled.
- A **rest window** continues with a sample when all hold: the filter is settled; the window already
  has a sample; the timestamp is not earlier than the previous one and at most 100 000 us after it;
  every gyro axis is within 0.0175 rad/s (1 deg/s) of the window's first gyro sample; every axis of
  smoothed gravity is within 0.01 G of its value at the window's start (about 0.6 degrees of tilt).
  Otherwise a new window starts at this sample.
- When a window has lasted 1 000 000 us, the mean of its gyro samples becomes the offset, the
  estimator is marked valid, and a new window begins. So the offset is refreshed about once per
  second while the pad stays still (offsets drift with temperature).
- Once valid, the offset is subtracted from every gyro sample **before** the sample is stored as the
  reported angular velocity and before it enters the orientation integration. Before the first
  offset is learned, samples pass unchanged.
- The estimator is reset when the set of devices or the active controller changes.
- The first learned offset per controller is logged once, in deg/s, with the controller id.
- No configuration. Runs under the controller object's existing mutex in the motion callback.

Known limit, to keep as a documented shortcut: a steady turn slower than 1 deg/s about the vertical
axis, or a steady tilt slower than about 0.75 deg/s, held for a whole second, is learned as offset.
The upgrade, if ever needed, is an explicit "calibrate now" action. [read]

### Where it hooks in

`src/libs/controller.cpp`, class `GameController`: `Motion` (gyro branch, where the sample is copied
into the state before the tilt correction) and `SetDevices` (reset). Upstream KytyPS5. [tree: both
exist, no bias handling.] The estimator is a new header-only class.

### Dependencies

None. If motion is relayed to the AnyPS5 host pad library by the bridge, the correction applies to
the relayed values too, since it changes the stored state. [inferred]

### Platform notes

None; it is device behaviour, the same under SDL on Linux.

### Size / risk / test

Small. Risk: learning a wrong offset during very slow deliberate motion (bounded by the thresholds).
Test: unit test of the estimator with synthetic data: constant bias plus noise converges within
1.3 s and is removed; rotation above threshold never learns; a timestamp gap restarts the window;
tilt change restarts it. Hardware: pad on a table, log line appears, reported gyro near zero.

---

## K2. AMD FSR frame generation through the AMD FSR SDK runtime (patch 0055)

### Purpose

Show one generated (interpolated) frame between every two rendered frames, to double the displayed
frame rate on any vendor's GPU. Without it only NVIDIA cards have frame generation (through the
third-party DLSS-G path). There is no defect to reproduce; this is a new, opt-in feature.

### Behaviour required

**Activation**

- Off by default. Requested by one environment variable equal to `1` (patch: `ASTROHYBRID_FSRFG`).
  If DLSS frame generation is also requested, DLSS-G wins and FSR frame generation stays off.
- Needs: a build with the SDK headers; the two runtime libraries present; three unused queues in the
  graphics queue family beyond those the renderer already creates (the game queue and the optional
  side queue). If the family has too few queues, log the counts and stay off. If a runtime file is
  missing, cannot be loaded, or lacks an entry point, log which and stay off. In every "off" case the
  game runs normally with the driver's swapchain.
- A build without the SDK logs "not built in" once when requested.

**Options** (all read once at load)

| Patch name | Default | Meaning |
|---|---|---|
| `ASTROHYBRID_FSRFG` | 0 | 1 turns the feature on |
| `ASTROHYBRID_FSR_PATH` | unset | folder holding the runtime; default is the folder of the renderer library |
| `ASTROHYBRID_FSRFG_ASYNC` | 1 | 0 runs interpolation on the game queue instead of the async compute queue |
| `ASTROHYBRID_FSRFG_DEBUG` | 0 | 1 draws the runtime's debug view and tear lines, enables its debug checking |
| `ASTROHYBRID_FSRFG_HUD` | 1 | 0 turns the HUD-less capture off |
| `ASTROHYBRID_FSR_DEPTH_INVERTED` | value of `ASTROHYBRID_DLSS_DEPTH_INVERTED`, else 0 | depth is reversed-Z |
| `ASTROHYBRID_DLSS_JITTER_SIGN` | +1 | negative value flips the sign of the jitter |

**Runtime loading (exact)**

- Files, both in the same folder: the generic loader `amd_fidelityfx_loader_vk.dll` and the effect
  `amd_fidelityfx_framegeneration_vk.dll`. The effect file is only checked for existence; the loader
  library is the one loaded (with the "altered search path" flag so that it finds its neighbour), and
  it loads the effect itself. It is the same loader module FSR upscaling uses; contexts are separate.
- Entry points resolved by name from the loader: `ffxCreateContext`, `ffxDestroyContext`,
  `ffxConfigure`, `ffxQuery`, `ffxDispatch`. All five are required.
- Directly after loading, a global debug configuration is sent for the frame-generation effect with a
  message callback at warning level; at most 40 runtime messages/errors are logged in total.
- Build time: headers only, from the SDK's `api/include` and `framegeneration/include` folders
  (CMake cache variable `KYTY_FFX_SDK_DIR`, compile definition `KYTY_FFX`). Nothing is linked.
- Structures used from the SDK's "ffx API": creation of a frame-generation swapchain context for
  Vulkan (wrapping the swapchain create-info) with its version structure chained; a query for the
  swapchain replacement functions (get images, acquire, present, destroy); creation of a
  frame-generation context with a Vulkan backend description and its version structure chained (the
  version structures are required since SDK 2.1); configure-frame-generation; dispatch "prepare V2";
  dispatch "wait for presents".

**Device**

- Three more queues of the graphics family are created at device creation and handed over as: async
  compute, present/composition, image acquisition. The game queue is also given, with a submit
  function that takes the renderer's queue mutex (`GraphicContext::queue_mutex`) around the submit;
  the lock helper must be re-entrant per thread.
- The device-function lookup callback given to the runtime must, when a name ending in `KHR` is not
  found, retry the name without that suffix. Reason: the runtime asks for some functions by their
  extension names without a null check; on a Vulkan 1.1+ device these exist only under the core name,
  and the first run crashed on the buffer-memory-requirements query. [read]
- While the feature is requested, the video-memory budget that the caches plan with keeps a second
  reserve of the same size as the existing one (the smaller of one eighth of the budget and 1 GiB) on
  discrete cards, because the runtime's own allocations are outside the caches.
- The vendor low-latency extension path stays off while any frame generation is requested.

**Swapchain**

- The window's swapchain is created through the runtime's frame-interpolation swapchain instead of
  the driver call. If creation or the function query fails, log it and create the normal swapchain.
  Only one such swapchain exists; a second swapchain is created normally.
- For that swapchain, image enumeration, image acquisition and present go through the replacement
  functions. Present must be called **without** holding the queue mutex (the runtime submits to the
  game queue through the locking submit function and waits for its next image); the existing
  drain-ready-submissions step still runs under the mutex before it.
- The image handed to present is left in the shader-read layout instead of the present-source
  layout, in every path that prepares it (plain blit, filtered draw, overlay draw).
- The swapchain create-info's extent, format and colour space are remembered; a colour space other
  than sRGB-nonlinear marks the output as HDR for the frame-generation context.
- Teardown order (SDK 2.4): wait for presents; configure frame generation off on the swapchain;
  destroy the frame-generation context; destroy the swapchain context; then destroy the swapchain
  handle through the runtime's destroy function. All under the queue lock.

**Per-frame inputs**

- A frame counter: "current frame" is the last frame whose render commands were submitted, plus one.
- Camera: the game's per-view constants are captured when wanted (once per tagged frame): a 4x4
  projection matrix followed by a 4x4 view matrix, row-major with row vectors. From them: near and far
  planes from the projection's depth terms (take the smaller as near; keep previous values if not
  finite or not positive; defaults 0.1 and 1000), vertical field of view from the projection's
  Y-scale (default 1 rad), and camera right/up/forward/position in world space from the view matrix
  treated as rotation plus translation. The first captured projection is logged once.
- At the game's temporal upscale pass (the same point the DLSS-G path uses), once per frame (the
  first such pass wins): copy depth and motion vectors into images owned by this module, because the
  game's images are valid only at that point.
  - Depth source must be a 32-bit float depth format (with or without stencil). It is copied to a
    single-channel 32-bit float colour image through a staging buffer (a direct depth-to-colour image
    copy would need a newer Vulkan maintenance extension). Source layouts are restored.
  - Motion vectors are copied in their own format, which the runtime must know.
  - Inputs whose sizes differ from the pass size, or with unsupported formats, are refused; this is
    logged once.
  - A ring of 3 slots indexed by frame number. Images only grow: the game renders at a dynamic
    resolution, so each slot keeps the largest size seen and records the part in use. Replaced images
    and the replaced staging buffer are **not** destroyed while the process runs (a submitted command
    buffer may still use them). This is a deliberate leak bounded by the number of size increases.
  - With each slot: jitter in pixels (jitter in UV times render size, times the sign option), frame
    time in milliseconds since the previous tag clamped to 1..250 (16.7 for the first), and the
    camera.

**Generation**

- Before each present of a new frame: make sure a frame-generation context exists whose maximum
  render size covers this frame's render size (display size is the swapchain extent; if a larger
  render size appears, wait for presents, destroy and recreate). Context flags: async workload
  support, motion-vector jitter cancellation, plus depth-inverted, HDR and debug checking as
  applicable. If creation fails, log it and do not try again for this swapchain.
- Then configure: generation enabled, the callback below, async allowed per option, generation
  rectangle the whole display, the frame number, "present only generated" off, and the HUD-less image
  if one was captured for exactly this frame at display size and format. The first configure call is
  made under the queue lock (turning interpolation on waits for idle queues). A present that shows
  no new frame keeps the last configuration.
- The runtime calls the frame-generation callback inside present, on the present thread, with a
  command list. There: if there is no context, or this frame number was already generated, or the
  frame's slot has no valid depth and motion vectors, or "prepare" or the interpolation dispatch
  fails, then copy the presented colour image into the output, so the real frame is shown twice,
  and count the reason (no-context, repeat, no-inputs, prepare, dispatch). Otherwise dispatch
  "prepare V2" with the slot's data (motion-vector scale is minus the render width and minus the
  render height; view-space-to-metres factor 1; reset never set, because a gap in frame numbers
  resets the history by itself), then the interpolation with the parameters the runtime passed in.
- "Frames shown per game frame" is exposed as a smoothed value (each present moves it one tenth of
  the way to 2 if a frame was generated, else to 1) for the overlay and the frame pacing that the
  DLSS-G path already feeds.
- One log line every 10 s: game presents, generated frames, each not-generated reason count, input
  tags, last render size, jitter, frame time, camera values, HUD-less counters, and video memory
  used and budget on device-local heaps (from the memory-budget extension; zeros without it).

**HUD-less colour (optional part; the patch says its capture point does not trigger in Astro Bot yet)**

- Purpose: without it the HUD is interpolated with the picture and smears when the camera moves.
- Detection: remember the guest addresses of the last 4 display buffers that were flipped. While a
  frame is drawn, watch each draw's first colour target and whether it blends (a blend of "source
  one, destination zero" counts as not blending). The first blended draw into a display buffer that
  follows a non-blended draw into the same buffer since the last flip marks the start of the HUD; at
  most one capture per flip.
- Capture, recorded before that draw (rendering is ended first, and no draw-run record continues
  across it): scale the display buffer into an image of the swapchain's size and format, in the same
  two steps the presenter uses later (copy into the presenter's frame format if it differs, then a
  linear-filtered blit), so that pixels match wherever the HUD does not draw. The source image's
  layout and access are restored. Only a single-sample, single-layer cache image with whole-image
  tracked state can be captured; otherwise count "unavailable".
- A ring of 4 captured images; each capture gets a serial number. At the flip, the presenter's frame
  records the serial of the capture taken from its display buffer since the previous flip (0 if
  none; never for a headset frame or a blank frame). At present, the serial is used only if the
  present uses the plain blit path (not the filtered downscale).
- Once per distinct "shape" of frame end (up to 16), log the last 10 runs of draws by target with
  their blended/unblended counts and whether the capture was taken.

**Threading summary**

Render thread: camera capture, input copies, HUD detection and capture. Present thread: configure,
callback, statistics. Shared state is guarded by three mutexes (contexts; input slots; HUD ring) and
atomics for the flags and counters. The queue mutex is taken for runtime calls that touch the game
queue (swapchain context create/destroy, first configure, wait-for-presents).

### Where it hooks in

Upstream KytyPS5 (exist in our tree):

- `src/graphics/presentation/window/vulkanWindow.cpp`: `VulkanCreateDevice` (queue count),
  `WindowContext::CreateVulkan` (queues handed over after device creation).
- `src/graphics/presentation/window/swapchain.cpp`: `Swapchain::Create`, `Destroy`,
  `AcquireNextImage`, `RecordPresentCommands`, `Present`; `Presenter::PrepareFrame`,
  `PrepareBlankFrame`; struct `Presenter::Frame`.
- `src/graphics/host_gpu/vma.cpp`: `GraphicContext::GetTotalMemoryBudget`.
- `src/graphics/host_gpu/renderer/renderDraw.cpp`: `RenderExecutor::ExecutePreparedDraw`,
  `DrawIndex`, `DrawAuto`, `DrawIndirectNative`; `render.h`.
- `CMakeLists.txt`.

Third-party additions the patch builds on:

- `src/graphics/host_gpu/renderer/frameGen.cpp/.h` (DLSS-G through Streamline): its hook interface is
  reused and forwarded to: `Requested`, `Active`, `RenderSubmitted`, `PresentStart`, `PresentEnd`,
  `CurrentFrame`, `CameraWanted`, `SetCamera`, `TagFrame`, `ShownPerGameFrame`, and the input
  description type `ImageInput`.
- `src/graphics/host_gpu/renderer/dlss.cpp`: `RenderExecutor::NoteFrameGenCamera`, `TagFrameGen`,
  `NoteDlssTargets` find the camera constants and the upscale pass's depth and motion vectors.
- `src/graphics/presentation/window/lowLatency.cpp`: `WantExtensions`.
- FSR upscaling (`fsr.inc`, `Fsr::Requested`) and its CMake block: [tree: not in our tree at all].

### Dependencies

- **Third-party code**: the frame-generation hook points listed above. They will be removed from our
  tree, so our rewrite must first provide its own equivalents: a per-frame counter, capture of the
  per-view constants, identification of the temporal upscale pass with its depth, motion vectors and
  jitter, and present start/end callbacks. This is the larger part of the work and is specific to
  the game's render passes.
- **External runtime**: AMD FSR SDK 2.4 (the "ffx API"), providing FSR Frame Generation 3.1.6 with
  its frame-interpolation swapchain 3.1.7, and FSR 4 versions where the runtime offers them. [read]
  Licence: not stated in the patch; AMD publishes the SDK source under the MIT licence and ships
  signed prebuilt binaries [inferred, verify before use]. Linux build: none as far as the patch
  shows: only Windows DLL names, Windows loading calls, and an unconditional Windows header.
- Other features in this list: none. K5/K8 touch the same memory functions in `vma.cpp`.

### Platform notes

- Everything about loading is Windows-only: the two `.dll` files, loading with altered search path,
  finding the renderer module's own folder, the file-existence check, wide-character message strings.
- Linux needs one of: (a) the SDK's Vulkan backend and frame-generation effect built from source as
  shared objects, if AMD's sources build on Linux. Unknown; the Vulkan frame-interpolation swapchain
  uses its own threads and may use Windows primitives [inferred]; or (b) our own interpolation, or
  (c) a driver- or compositor-level solution outside the engine. This must be investigated before any
  estimate is trusted. Loading itself would be `dlopen`/`dlsym` and finding the module folder via the
  dynamic loader.
- The Vulkan-side requirements (three spare queues, shader-read layout at present, core-name
  fallback) are platform-neutral.

### Size / risk / test

Large (about 1400 new lines plus changes in 8 files in the patch, not counting the hook layer we must
rebuild). Risks: no Linux runtime; depends on game-specific pass detection; queue-mutex deadlocks
around present; deliberate non-freeing of replaced images; extra video memory (the added reserve);
HUD smearing while the HUD-less capture does not trigger; swapchain recreation (resize, HDR switch)
ordering.

Test: with the feature off, no behaviour change (present layout, queue count). On: the 10 s line
shows about 2.0 shown per game frame during play and reason counts during menus/loading; resize the
window and toggle fullscreen repeatedly without validation errors; run with Vulkan validation layers;
remove a runtime file and confirm a clean fallback; run on a GPU with too few queues.

---

## K3. Window icon fallback (patch 0056)

### Purpose

The window icon comes from the game's system content (`icon0.png`), which the standalone loader
knows. In the hybrid setup Kyty does not load the game's system content, so the window and task bar
show the default icon.

### Behaviour required

- Unchanged first choice: the icon path reported by the loader (`Loader::SystemContentGetIconPath`).
- If that gave no icon, try in order, stopping at the first that loads as PNG:
  1. the file named by an environment variable (patch: `ASTROHYBRID_WINDOW_ICON`), if set and not
     empty; the value is a UTF-8 path;
  2. `app0/sce_sys/icon0.png` relative to the current working directory.
- A candidate that does not exist or fails to decode is skipped silently. No icon is not an error.
- Evaluated once per process (the existing one-time load), on the thread that updates the window.

### Where it hooks in

`src/graphics/presentation/window/window.cpp`: `WindowContext::UpdateIcon`, helper
`WindowLoadPngIcon`. Upstream KytyPS5. [tree: both exist, no fallback.]

### Dependencies

None. With `app0.zar` (A1/A2) the icon may exist only inside the archive; the fallback reads the
disk only. [inferred]

### Platform notes

None in the code. On Wayland the window icon is normally taken from the desktop entry, not from the
window, so the effect may be visible only under X11/XWayland. [inferred] Our launcher already has its
own icon handling; check that this is still wanted.

### Size / risk / test

Small. Test: run from a folder with `app0/sce_sys/icon0.png`; then with the variable pointing to
another PNG; then to a missing file.

---

## K4. Pipeline list: title id override, cross-version lists, default executable (patch 0057)

### Purpose

The "pipeline list" is a file of known pipelines that can be built ahead of play ("prepare shaders",
`KYTY_PIPELINE_LIST_PREPARE=1`) to avoid stutter. It is matched to the game by title id, app version
and the game executable. In the hybrid setup Kyty has no loader metadata, so the title id is empty
and the executable path is unset: the list is never used and the prepare run does nothing. Also, a
list made for another app version of the same title was rejected outright.
Recognise it by: no "Pipeline list" log lines at all in hybrid mode, or the line saying the list is
for another version and is ignored.

### Behaviour required

1. Title id for the pipeline list (both when loading the list and when deciding whether to run the
   prepare step) is: the loader's title id if it has one; otherwise an environment variable (patch:
   `ASTROHYBRID_CACHE_TITLE_ID`) accepted only if it is exactly `PPSA` followed by five decimal
   digits. Otherwise empty, and the list stays off. This is the same rule the cache file names
   already use.
2. Version match: a list whose title id differs is always ignored. A list whose app version differs
   is ignored unless `KYTY_PIPELINE_LIST_CROSS` is non-zero (default 0); then it is used best-effort.
   Each list entry is still matched to the game's executable by shader hashes, so entries that do
   not fit are simply not built. [read; the hash matching is existing behaviour]
3. Executable: if no executable path was set by the emulator, use `app0/eboot.bin` relative to the
   current working directory. If it cannot be read, the existing "cannot be read: ignored" log line
   and early return apply.

### Where it hooks in

`src/graphics/host_gpu/renderer/pipeline/pipelineCache.cpp`: `PipelineCache::InitializePipelineList`,
the `PipelineCache` constructor (gate for `RunListPrepare`), `PipelineCacheTitleId`,
`PipelineCacheFileTitleId`, `PipelineListExecutable`. Upstream KytyPS5 for the list itself; the
override function is a carried third-party change.

[tree] State of our tree: the override function exists (used for cache file names), but
`InitializePipelineList` and the constructor's prepare gate both still use the loader-only title id;
`KYTY_PIPELINE_LIST_CROSS` exists and is **wider** than the patch (it also accepts another title id,
"another version or SKU"); there is no `app0/eboot.bin` default. So the remaining work is items 1
and 3. Decide whether to keep our wider cross rule; this spec recommends keeping what is in the tree.

### Dependencies

Upstream pipeline list and prepare run. K7 is the companion for the prepare run.

### Platform notes

None. The relative path assumes the process's working directory is the game folder, which holds for
our launcher. [inferred] With an archive-only `eboot.bin` the default path does not exist; our tree's
executable reader already understands an archive member path, which the default does not use.

### Size / risk / test

Small. Risk: using a wrong executable silently (bounded by hash matching). Test: hybrid run with the
title id variable set and no loader metadata: the list loads and the prepare run starts; with a list
of another app version: ignored by default, used with the cross switch; with no executable set: the
default is read.

---

## K5. Evict unused cached images before spilling to system memory (patch 0058)

### Purpose

When video memory is full, image creation falls back to system memory (`KYTY_IMAGE_SYSMEM_FALLBACK`,
default on). Such images are much slower to sample, and they stay there. On 6 to 8 GB cards this
shows as a lasting drop in frame rate after a heavy scene. Often the image cache still holds images
no longer in use; freeing those first keeps the new image in video memory.

### Behaviour required

- A memory-pressure hook on the graphics context: a callable taking a byte count ("free at least
  about this much") and returning the number of bytes it released (0 if nothing). It is optional;
  when unset, behaviour is unchanged.
- In both places where an image's memory is allocated in video memory (creation of a normal image,
  and binding memory to levels of a sparse image), the order on an out-of-memory failure becomes:
  1. existing step: release retained/retired objects, retry;
  2. **new**: if still failing and the hook is set, call it; if it returned non-zero, retry once;
  3. existing step: if still failing and the fallback is enabled, allocate from any memory type.
- Requested byte count: at least 64 MiB. For sparse levels, the larger of 64 MiB and the bytes being
  bound. For normal images, the larger of 64 MiB and an estimate: width x height x depth (min 1) x
  array layers (min 1) x 4 bytes (mip levels and the real format size are ignored).
- The hook's implementation (not in the patch) must evict images that no submitted or pending work
  uses, and must be safe to call from inside image creation: no recursion into image creation, and
  no lock-order inversion with the cache's locks. It must return the bytes really freed, so that a
  return of 0 skips the pointless retry.

### Where it hooks in

`src/graphics/host_gpu/vma.cpp`: `GraphicContext::CreateImage` and
`GraphicContext::BindSparseImageLevels` (both upstream KytyPS5, exist in our tree), helper
`ImageSystemMemoryFallbackEnabled`.

Not in our tree: the hook member itself (`relieve_memory_pressure` in the patch's context) and
whatever sets it. It comes from third-party code that the patch does not show. Our tree's texture
cache has garbage collection and eviction of its own (`src/graphics/host_gpu/renderer/cache/textureCache.h`)
from which the hook should be built. Also, in our tree `CreateImage` reacts only to "out of device
memory"; the patch's context treats four results as memory failures (out of device memory, out of
host memory, fragmented pool, fragmentation). Adopt that wider set only on purpose. [tree]

### Dependencies

A texture-cache eviction entry point to implement the hook (to be written by us). K8 builds on this
sequence.

### Platform notes

None. Whether "any memory type" means system memory depends on the driver (stated for NVIDIA). [read]

### Size / risk / test

Medium (the call sites are small; a correct, re-entrancy-safe eviction function is the work).
Risk: evicting an image still referenced by recorded commands; deadlock when called under cache
locks. Test: cap the planned budget with the existing budget-limit switch and play a heavy scene:
count of images placed in system memory with and without the hook (add a counter); unit test of the
ordering with a fake allocator that fails N times.

---

## K6. spirv-val: uniform buffer standard layout (patch 0059)

### Purpose

With the optimisation that shrinks function-local arrays (`KYTY_FUNCTION_ARRAY_SHRINK`), and for the
recompiler's uniform-block path, generated SPIR-V lays out arrays of 32-bit words in uniform blocks
with the relaxed ("standard", std430-like) rules. The device enables the matching Vulkan feature
whenever that path is allowed, but the validator was run with default rules, so valid shaders were
rejected. Result: the shrunk shader is discarded (optimisation lost), or the shader fails validation,
with validator messages about uniform block layout/array stride in the log.

### Behaviour required

- Every run of the SPIR-V validator on generated shaders passes the option "uniform buffer standard
  layout" set to the same boolean the code generator uses to decide that uniform blocks may hold
  word arrays (the host buffer robustness flag `uniform_dword_loads_return_zero`, set at device
  creation from the device's features).
- This applies to: validation of emitted shaders in the pipeline cache, and validation of the result
  of the array-shrinking pass.
- When the flag is false, the validator keeps the strict rules.

### Where it hooks in

`src/graphics/host_gpu/renderer/pipeline/pipelineCache.cpp` (`ValidateShaderSpirv`),
`src/graphics/host_gpu/vulkanCommon.cpp` (`ShrinkFunctionArrays`); flag from
`ShaderRecompiler::Spirv::GetHostBufferRobustness()` in
`src/graphics/shader/recompiler/backend/spirv/SpirvEmitter.h`. All upstream KytyPS5.

[tree] **Already present in our tree** at all validator call sites (pipeline cache twice, the shrink
pass, and `aaMaskCoverage.cpp`). No work is needed beyond confirming this after the rewrite and
checking that no new validator call site is added without the option.

### Dependencies

None.

### Platform notes

None.

### Size / risk / test

Small (zero for us today). Test: run with the array-shrink switch on and the uniform path enabled;
no validator rejections about block layout in the log; grep that every validator call passes options.

---

## K7. Keep the window hidden during the "prepare shaders" run (patch 0060)

### Purpose

`KYTY_PIPELINE_LIST_PREPARE` runs the engine only to build pipelines and then stops; a launcher shows
its own progress. During that run the game window appeared (possibly fullscreen, black), which looks
like a hung game and steals focus.

### Behaviour required

- When the environment variable `KYTY_PIPELINE_LIST_PREPARE` is set, non-empty and not exactly `0`,
  the window is created hidden and the fullscreen flag is removed, whatever the configuration says.
- Otherwise window creation is unchanged.
- The window is never shown later in that run: the run ends by itself. Any code that would show or
  raise the window during the run must not do so. [inferred]
- Note the test differs from the pipeline cache's own (which parses a number); keep both meaning
  "on" for the same values, ideally through one shared helper.

### Where it hooks in

`src/graphics/presentation/window/window.cpp`: static `WindowCreate`, just before the SDL window is
created, where the window flags are assembled. Upstream KytyPS5. [tree: exists, no such check.]

### Dependencies

Upstream prepare run; K4 makes that run work in hybrid mode.

### Platform notes

A hidden window must still provide a Vulkan surface and swapchain. Under X11 this works. Under
native Wayland a surface that is never mapped may never deliver frame callbacks, and presenting in
FIFO mode can block forever; our tree has a native-Wayland option (`KYTY_WAYLAND_NATIVE`). The Linux
implementation must make sure the prepare run does not wait on presents (or forces a non-blocking
present mode) when hidden. [inferred; must be tested on both X11 and Wayland]

### Size / risk / test

Small. Risk: the prepare run hanging on a hidden Wayland surface. Test: run the prepare step from
the launcher on X11 and on Wayland: no window appears, the run finishes, the progress events still
arrive; then a normal start shows the window as before.

---

## K8. VRAM allocation cooldown and absurd-draw guard (patch 0061)

Two independent stability measures.

### K8a. Cooldown after video memory is exhausted

**Purpose.** During a burst of texture creation with video memory full (6 to 8 GB cards), every new
image goes through the whole failing sequence of K5: failing allocation, release of retained
objects, eviction under the cache lock, another failing allocation, and only then the system-memory
fallback. Repeated for every texture this gives long hitches.

**Behaviour required.**

- Process-wide state: a deadline on a monotonic clock, and a minimum size.
- Recording: when an image allocation reaches the system-memory fallback step (video memory failed
  even after K5's eviction, and the fallback is enabled), set the deadline to now + 250 ms. If no
  cooldown was running, the minimum size becomes this request's size; if one was running, the
  minimum size becomes the smaller of the two.
- Applying: at the start of an image allocation, if the fallback is enabled, a cooldown is running,
  and this request's size is at least the minimum size, allocate at once with "video memory
  preferred, not required" (so the driver may place it in system memory) and skip the failing
  sequence. Requests smaller than the minimum size still try video memory strictly, since they may
  fit. For normal images this shortcut applies only when the request would otherwise require video
  memory.
- Sizes: for sparse levels the bytes being bound; for normal images the estimate of K5 (without the
  64 MiB floor).
- The state is atomic with relaxed ordering; small races are acceptable (worst case one extra strict
  attempt or one extra relaxed one). With the fallback disabled, nothing changes.
- No configuration; 250 ms is the tuning constant.

**Hooks.** `src/graphics/host_gpu/vma.cpp`: `GraphicContext::CreateImage`,
`GraphicContext::BindSparseImageLevels`, beside `ImageSystemMemoryFallbackEnabled`. Upstream KytyPS5.

**Dependencies.** K5 (the step order it shortcuts). Works without K5, with less benefit.

### K8b. Guard against absurd draws

**Purpose.** A corrupted or misread draw command with a huge vertex or instance count makes the
host GPU run for seconds or hang, which ends in a device-lost error or a watchdog kill. [inferred
from the commit title "stability"; the patch gives no incident]

**Behaviour required.**

- Before an indexed draw and before a non-indexed ("auto") draw is processed, compute count x
  instance count in 64 bits (index count or vertex count). If the product is 2^30 (1 073 741 824) or
  more, drop the draw: return as for a draw with zero count.
- Log the first 8 dropped draws to standard error with the kind of draw, both counts and the
  product; later ones are dropped silently. The counter is atomic.
- Indirect draws are not covered (counts are not known on the CPU). [read: only the two entry points
  are changed]
- No configuration.

**Hooks.** `src/graphics/host_gpu/renderer/renderDraw.cpp`: `RenderExecutor::DrawIndex` and
`RenderExecutor::DrawAuto`, at the existing zero-count early return. Upstream KytyPS5. [tree: both
exist without the guard.] Check that dropping at that point leaves the draw-run bookkeeping that
precedes it consistent (the zero-count path already returns there).

**Dependencies.** None.

### Platform notes

None for either part.

### Size / risk / test

Small each. Risks: (a) images that would have fitted go to system memory for 250 ms after a failure;
(b) a legitimate draw above 2^30 vertex invocations is dropped (not expected in a game frame).
Tests: (a) unit test of the cooldown state machine with an injected clock: record, apply by size,
shrink of the minimum, expiry; in game, hitch length under a budget cap with and without it;
(b) unit test of the predicate at 2^30 - 1 and 2^30, including counts whose 32-bit product would
overflow; a synthetic draw with a huge instance count is dropped and logged at most 8 times.

---

## K9. Lock the flip rate to 60 FPS during video playback (patch 0062)

### Purpose

In-game videos advance one video frame per display flip while their audio plays in real time. With
the frame limiter at 30 FPS the 60 FPS studio intro runs at half speed with sound ahead; with a limit
above 60 or no limit it runs fast and loses sync. Reproduce: set the limit to 30 and watch the intro.

### Behaviour required

- The renderer holds one atomic boolean "video playback active", set by an exported function taking
  an integer (non-zero = active); see A5. Initially false.
- In the present loop, each iteration: the configured limit is computed as today (disabled, a preset,
  or the custom value). The **effective** limit is 60 while the flag is set, otherwise the configured
  one.
- When the effective limit differs from the one used in the previous iteration, the pacing state
  (next flip time) is reset, so the change takes effect at once in both directions.
- The choice of timing mode that today depends on "a limit is enabled and the present mode is
  immediate" must treat "video active" as "a limit is enabled".
- When the flag clears, the user's configured limit applies again from the next iteration. The
  user's setting is never modified, and the label shown for the limit is unchanged.
- Applies also when the configured limit is "off" (video then runs at 60) and when it is above 60.
- 60 is a constant in the patch; a video of another frame rate is not handled. A better version
  passes the video's frame rate with the signal. [inferred improvement]
- Interaction with frame generation or a game-speed step that derives from the limit is not
  addressed by the patch; the effective limit must be the value those consumers see. [inferred]

### Where it hooks in

- `src/graphics/presentation/videoOut.cpp`: `VideoOutDriver::Impl::PresentThread` and a new setter
  next to `CycleFrameLimit`; declaration in `videoOut.h`. The present loop is upstream KytyPS5; the
  frame limiter inside it (`FrameLimitChoice`, `FRAME_LIMITS`, custom limit, pacing) is a third-party
  addition. [tree: our loop has moved on: it also tracks the previous FPS value and a
  "ready flips" switch, so the change-detection must be written against the current code.]
- Export: third-party bridge `src/hybrid/kytyGpuBridge.cpp` plus the Windows export list
  `src/hybrid/kyty_gpu.def`.

### Dependencies

A5 (sender). Third-party frame limiter and bridge; after the rewrite, our own limiter and renderer
interface must carry the flag.

### Platform notes

The `.def` export list is Windows-only. On Linux the function needs default visibility in
`libkyty_gpu.so` (the bridge header already defines an export macro for that). [tree]

### Size / risk / test

Small. Risk: the flag stuck on (A5's exit paths), or pacing hiccup at the switch. Test: unit test of
"effective limit" selection; in game with limits 30, 120 and off: the intro plays at correct speed
in sync, and the limit returns immediately after the video ends or is skipped; log each transition
once while testing.

---

# Suggested order

1. K6 (verify only), A4, K3, K7, K4 (items 1 and 3), K8b, K1: independent and small.
2. A1, then A2 with A3: archive support, in that order.
3. K9 with A5: needs our own limiter and renderer export first.
4. K5, then K8a: needs the eviction entry point.
5. K2 last: blocked on a Linux runtime decision and on our own frame hooks.
