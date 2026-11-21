use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Seg {
    pub kind: u32,
    pub vaddr: u64,
    pub data: Vec<u8>,
    pub memsz: u64,
    pub align: u64,
    pub recorded: bool,
}

pub fn build_self(segs: &[Seg]) -> Vec<u8> {
    const INNER: usize = 0x300;
    const PHDR: usize = 0x340;
    let headers_end = PHDR + segs.len() * 56;
    // With four or more headers, the table extends into the segment area.
    let mut result = vec![0; 0x400.max(headers_end.next_multiple_of(0x10))];
    put(&mut result, 0, 0xEEF51454u32.to_le_bytes());
    put(&mut result, INNER, *b"\x7fELF\x02\x01\x01\x09");
    put(&mut result, INNER + 0x10, 0xFE10u16.to_le_bytes());
    put(&mut result, INNER + 0x12, 62u16.to_le_bytes());
    put(&mut result, INNER + 0x14, 1u32.to_le_bytes());
    put(&mut result, INNER + 0x20, 0x40u64.to_le_bytes());
    put(&mut result, INNER + 0x34, 64u16.to_le_bytes());
    put(&mut result, INNER + 0x36, 56u16.to_le_bytes());
    put(
        &mut result,
        INNER + 0x38,
        u16::try_from(segs.len()).unwrap().to_le_bytes(),
    );

    let mut record = 0x40;
    let mut elf_offset = (headers_end - INNER) as u64;
    for (index, seg) in segs.iter().enumerate() {
        let phdr = PHDR + index * 56;
        let size = seg.data.len() as u64;
        put(&mut result, phdr, seg.kind.to_le_bytes());
        put(&mut result, phdr + 8, elf_offset.to_le_bytes());
        put(&mut result, phdr + 16, seg.vaddr.to_le_bytes());
        put(&mut result, phdr + 32, size.to_le_bytes());
        put(&mut result, phdr + 40, seg.memsz.to_le_bytes());
        put(&mut result, phdr + 48, seg.align.to_le_bytes());
        elf_offset += size;

        if seg.recorded {
            assert!(record + 0x20 < INNER);
            let offset = result.len().next_multiple_of(0x10);
            result.resize(offset, 0);
            let flags = 0x800 | (index as u64) << 20;
            put(&mut result, record, flags.to_le_bytes());
            put(&mut result, record + 8, (offset as u64).to_le_bytes());
            put(&mut result, record + 16, size.to_le_bytes());
            put(&mut result, record + 24, size.to_le_bytes());
            record += 0x20;
            result.extend_from_slice(&seg.data);
        }
    }
    result
}

fn put<const N: usize>(data: &mut [u8], offset: usize, bytes: [u8; N]) {
    data[offset..offset + N].copy_from_slice(&bytes);
}

pub fn low_entropy(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| seed.wrapping_add((i % 16) as u8)).collect()
}

pub fn high_entropy(n: usize, mut seed: u64) -> Vec<u8> {
    (0..n)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect()
}

pub fn temp_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "astroport-run-test-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    match fs::remove_dir_all(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("{}: {error}", path.display()),
    }
    fs::create_dir(&path).unwrap();
    path
}

#[allow(dead_code)]
pub fn fake_dump(
    root: &Path,
    content_version: &str,
    eboot: &[u8],
    modules: &[(&str, Vec<u8>)],
) -> PathBuf {
    fs::create_dir_all(root.join("sce_sys")).unwrap();
    fs::create_dir_all(root.join("sce_module")).unwrap();
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join("eboot.bin"), eboot).unwrap();
    fs::write(
        root.join("sce_sys/param.json"),
        serde_json::json!({"titleId": "PPSA21567", "contentVersion": content_version}).to_string(),
    )
    .unwrap();
    for (name, bytes) in modules {
        fs::write(root.join("sce_module").join(name), bytes).unwrap();
    }
    fs::write(root.join("args.txt"), "-package -sequence product").unwrap();
    fs::write(root.join("data/x.bin"), b"data").unwrap();
    fs::write(root.join("param.sfx"), b"param").unwrap();
    root.to_path_buf()
}

#[allow(dead_code)]
pub fn versions_json(path: &Path, eboot_1019_sha256: Option<&str>) {
    let versions = serde_json::json!({
        "title_id": "PPSA21567",
        "versions": [
            {
                "version": "1.018",
                "content_version": "01.018.000",
                "eboot_sha256": null,
                "patches": "PPSA21567-1018.json",
            },
            {
                "version": "1.019",
                "content_version": "01.019.000",
                "eboot_sha256": eboot_1019_sha256,
                "patches": "PPSA21567-1019.json",
            },
        ],
    });
    fs::write(path, versions.to_string()).unwrap();
}

#[allow(dead_code)]
pub fn fake_install(root: &Path) -> PathBuf {
    for dir in ["anyps5/libs", "shims", "lib", "patches"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    for (dir, source, names) in [
        (
            "anyps5/libs",
            "anyps5",
            ["libkernel.prx", "libc.prx", "libSceAgc.prx"],
        ),
        (
            "shims",
            "shim",
            ["libSceAgc.prx", "libSceAgcDriver.prx", "libSceVideoOut.prx"],
        ),
    ] {
        for name in names {
            fs::write(root.join(dir).join(name), format!("{source} {name}")).unwrap();
        }
    }
    for (path, content) in [
        ("anyps5/libAstroHybridHost.prx", "host"),
        ("anyps5/relinker", ""),
        ("lib/libkyty_gpu.so", "gpu"),
        ("lib/libnvidia-ngx-dlss.so.310.9.16", "dlss"),
        ("ags-extension.toml", "version = \"0.1.0\"\n"),
    ] {
        fs::write(root.join(path), content).unwrap();
    }
    versions_json(&root.join("patches/versions.json"), None);
    root.to_path_buf()
}

#[allow(dead_code)]
pub fn fake_relinker(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(
        path,
        r#"#!/bin/sh
exec timeout --foreground -k 10 30 /bin/sh -s -- "$0" "$@" <<'SCRIPT'
set -eu
script=$1
shift
[ ! -f "$(dirname "$script")/fail" ] || exit 3
to_intel=
if [ "$1" = --to-intel ]; then
    to_intel=$1
    shift
fi
[ "$#" -eq 3 ] && [ "$1" = --registry ] || exit 2
input=$2
output=$3
out=$(dirname "$output")
{
    if [ -n "$to_intel" ]; then printf "%s\n" "$to_intel"; fi
    printf "%s\n" "$@"
} > "$out/args.txt"
[ -f "$input" ]
mkdir -p "$out/app0/sce_module"
printf 'fake executable\n' > "$output"
printf '{}\n' > "$output.registry.json"
printf '{}\n' > "$output.needed.json"
for module in "$(dirname "$input")"/sce_module/*.prx; do
    [ -f "$module" ] || continue
    name=${module##*/}
    cp "$module" "$out/app0/sce_module/$name.guest.prx"
    printf '{}\n' > "$output.$name.guest.prx.registry.json"
done
SCRIPT
"#,
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
