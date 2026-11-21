use std::fs;
use std::path::PathBuf;
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
