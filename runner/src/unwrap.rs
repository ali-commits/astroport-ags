//! Port of AnyPS5's tools/unwrap_self.py.

use crate::JobError;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

const SELF_MAGIC: u32 = 0x1D3D154F;
const SELF_MAGIC_KERNEL: u32 = 0xEEF51454;
const RECORD_OFFSET: usize = 0x40;
const RECORD_SIZE: usize = 0x20;
const RECORD_HAS_SEGMENT: u64 = 0x800;
const RECORD_SEGMENT_SHIFT: u32 = 20;
const FIRST_REGION: usize = 0x400;
const MAX_ALIGNMENT_PADDING: u128 = 64 * 1024 * 1024;
const MAX_OUTPUT_FACTOR: u128 = 4;
const MIN_OUTPUT_LIMIT: u128 = 256 * 1024 * 1024;
const PT_LOAD: u32 = 1;
const PT_NOTE: u32 = 4;
const PT_TLS: u32 = 7;
const ENTROPY_LIMIT: f64 = 7.5;
const ENTROPY_SAMPLE: u64 = 0x10000;
const ELF_HEADER_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;
const ELF_MAGIC: &[u8; 4] = b"\x7fELF";

struct Record {
    flags: u64,
    source: u64,
    size: u64,
}

struct Phdr {
    kind: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    size: u64,
    memory: u64,
    align: u64,
    // Offsets and their shifts may temporarily fall outside the ELF64 range.
    source: i128,
    host: Option<usize>,
}

struct Region {
    old: u64,
    size: u64,
    vaddr: i128,
    align: u64,
    load: Option<usize>,
    new: u128,
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

// read_records
fn read_records(data: &[u8]) -> Vec<Record> {
    let mut records = Vec::new();
    for offset in (RECORD_OFFSET..FIRST_REGION).step_by(RECORD_SIZE) {
        let flags = read_u64(data, offset);
        let source = read_u64(data, offset + 8);
        let size = read_u64(data, offset + 16);
        let size_again = read_u64(data, offset + 24);
        if size == 0
            || size != size_again
            || source >= data.len() as u64
            || source < 0x350
            || source % 0x10 != 0
            || size > data.len() as u64
        {
            break;
        }
        records.push(Record {
            flags,
            source,
            size,
        });
    }
    records
}

// parse_elf
fn parse_elf(data: &[u8]) -> Result<(&[u8], usize, Vec<Phdr>), String> {
    let inner = data
        .windows(ELF_MAGIC.len())
        .position(|window| window == ELF_MAGIC)
        .ok_or("no inner ELF header")?;
    let elf = &data[inner..];
    let header = elf
        .get(..ELF_HEADER_SIZE)
        .ok_or("inner ELF header is truncated")?;
    if header[4] != 2 || header[5] != 1 {
        return Err("inner ELF is not 64-bit little-endian".into());
    }
    let version = read_u32(header, 0x14);
    if header[6] != 1 || version != 1 {
        return Err(format!(
            "unsupported inner ELF version (identification {}, object {version})",
            header[6]
        ));
    }
    let program_offset = read_u64(header, 0x20);
    let machine = read_u16(header, 0x12);
    if machine != 62 {
        return Err(format!(
            "unsupported inner ELF machine {machine} (expected 62)"
        ));
    }
    let header_size = read_u16(header, 0x34);
    if header_size != 64 {
        return Err(format!(
            "invalid inner ELF header size {header_size} (expected 64)"
        ));
    }
    let entry_size = usize::from(read_u16(header, 0x36));
    let count = usize::from(read_u16(header, 0x38));
    if entry_size != PHDR_SIZE || count == 0 {
        return Err(format!(
            "program header table is {count} entries of {entry_size} bytes"
        ));
    }
    if program_offset < ELF_HEADER_SIZE as u64 {
        return Err("ELF program header table overlaps the ELF header".into());
    }
    if u128::from(program_offset) + (count * entry_size) as u128 > elf.len() as u128 {
        return Err("ELF program header table does not fit the file".into());
    }

    let program_offset = program_offset as usize;
    let mut headers = Vec::with_capacity(count);
    for index in 0..count {
        let entry = &elf[program_offset + index * PHDR_SIZE..];
        let kind = read_u32(entry, 0);
        let flags = read_u32(entry, 4);
        let offset = read_u64(entry, 8);
        let vaddr = read_u64(entry, 16);
        let size = read_u64(entry, 32);
        let memory = read_u64(entry, 40);
        let align = read_u64(entry, 48);

        if matches!(kind, PT_LOAD | PT_TLS) {
            if size > memory {
                return Err(format!("segment {index} file size exceeds memory size"));
            }
            if align != 0 && !align.is_power_of_two() {
                return Err(format!("segment {index} alignment is not a power of two"));
            }
        }
        if kind == PT_TLS {
            if align > 8192 {
                return Err(format!("segment {index} TLS alignment exceeds 8192"));
            }
            if memory > 0x7FFF0000 {
                return Err(format!(
                    "segment {index} TLS memory size exceeds 0x7fff0000"
                ));
            }
        }
        if kind == PT_LOAD && u128::from(vaddr) + u128::from(memory) > (1u128 << 64) {
            return Err(format!("segment {index} memory range exceeds ELF64"));
        }
        headers.push(Phdr {
            kind,
            flags,
            offset,
            vaddr,
            size,
            memory,
            align,
            source: i128::from(offset),
            host: None,
        });
    }
    Ok((header, program_offset, headers))
}

// load_for: the last matching load wins when memory ranges overlap.
fn load_for(headers: &[Phdr], loads: &[usize], header: &Phdr) -> Option<usize> {
    loads.iter().rev().copied().find(|&index| {
        let load = &headers[index];
        load.memory != 0
            && header.vaddr.checked_sub(load.vaddr).is_some_and(|offset| {
                offset < load.memory || (header.size == 0 && offset == load.memory)
            })
    })
}

// place_segments
fn place_segments(data: &[u8], headers: &mut [Phdr], records: &[Record]) -> Result<(), String> {
    let length = data.len() as i128;
    let loads: Vec<_> = (0..headers.len())
        .filter(|&index| headers[index].kind == PT_LOAD)
        .collect();
    let mut used = vec![false; records.len()];
    let mut placed = vec![false; headers.len()];
    for (index, record) in records.iter().enumerate() {
        let target = record.flags >> RECORD_SEGMENT_SHIFT;
        if record.flags & RECORD_HAS_SEGMENT != 0 && target < headers.len() as u64 {
            let target = target as usize;
            if !placed[target] && headers[target].size == record.size {
                headers[target].source = i128::from(record.source);
                placed[target] = true;
                used[index] = true;
            }
        }
    }

    // Python's max keeps the first segment when offsets tie.
    let last = headers
        .iter()
        .enumerate()
        .rev()
        .filter(|(index, header)| placed[*index] && header.vaddr == 0 && header.kind != PT_LOAD)
        .map(|(_, header)| header)
        .max_by_key(|header| header.offset);
    if let Some(last) = last {
        let mut cursor = last.source + i128::from(last.size);
        let mut trailing: Vec<_> = (0..headers.len())
            .filter(|&index| {
                let header = &headers[index];
                !placed[index]
                    && header.vaddr == 0
                    && header.kind != PT_LOAD
                    && header.size != 0
                    && header.offset > last.offset
            })
            .collect();
        trailing.sort_by_key(|&index| headers[index].offset);
        for index in trailing {
            let header = &mut headers[index];
            placed[index] = true;
            if header.kind == PT_NOTE {
                header.source = 0;
                header.size = 0;
                continue;
            }
            header.source = cursor;
            cursor += i128::from(header.size);
            if cursor > length {
                header.source = 0;
                header.size = 0;
            }
        }
    }

    for &index in &loads {
        let header = &mut headers[index];
        if header.size == 0 || placed[index] {
            continue;
        }
        if let Some((record_index, record)) = records
            .iter()
            .enumerate()
            .find(|(record_index, record)| !used[*record_index] && record.size == header.size)
        {
            used[record_index] = true;
            header.source = i128::from(record.source);
        }
    }

    let mut tail: Option<(u64, i128)> = None;
    for index in 0..headers.len() {
        let header = &headers[index];
        if header.kind == PT_LOAD {
            continue;
        }
        if placed[index] {
            let host = load_for(headers, &loads, header).filter(|&host| {
                let load = &headers[host];
                let offset = header.vaddr - load.vaddr;
                load.size != 0
                    && offset <= load.size
                    && header.size <= load.size - offset
                    && header.source == load.source + i128::from(offset)
            });
            if header.size != 0 && tail.is_none_or(|(offset, _)| header.offset > offset) {
                tail = Some((header.offset, header.source - i128::from(header.offset)));
            }
            headers[index].host = host;
        } else if header.vaddr != 0 {
            let Some(host) = load_for(headers, &loads, header) else {
                if header.size != 0 {
                    return Err(format!(
                        "segment {index} vaddr {:#x} is in no PT_LOAD",
                        header.vaddr
                    ));
                }
                headers[index].source = 0;
                continue;
            };
            let load = &headers[host];
            let offset = header.vaddr - load.vaddr;
            if header.size != 0 && (offset > load.size || header.size > load.size - offset) {
                return Err(format!(
                    "segment {index} does not fit the file-backed range of PT_LOAD {host}"
                ));
            }
            if load.size == 0 || offset > load.size {
                headers[index].source = 0;
                continue;
            }
            let source = load.source + i128::from(offset);
            if tail.is_none_or(|(offset, _)| header.offset > offset) {
                tail = Some((header.offset, source - i128::from(header.offset)));
            }
            headers[index].host = Some(host);
            headers[index].source = source;
        } else if header.size != 0 {
            let header = &mut headers[index];
            header.source =
                (i128::from(header.offset) + tail.map_or(0, |(_, shift)| shift)).min(length);
            let keep = (length - header.source) as u64;
            header.size = header.size.min(keep);
            header.memory = header.memory.min(keep);
        }
    }

    for (index, header) in headers.iter().enumerate() {
        if header.kind == PT_LOAD && header.size == 0 {
            continue;
        }
        if header.source < 0 || header.source + i128::from(header.size) > length {
            return Err(format!("segment {index} does not fit the file"));
        }
    }
    Ok(())
}

// entropy
fn entropy(data: &[u8]) -> f64 {
    let mut counts = [0u32; 256];
    let mut order = Vec::new();
    for &byte in data {
        let count = &mut counts[usize::from(byte)];
        if *count == 0 {
            order.push(byte);
        }
        *count += 1;
    }
    -order
        .iter()
        .map(|&byte| {
            let probability = f64::from(counts[usize::from(byte)]) / data.len() as f64;
            probability * probability.log2()
        })
        .sum::<f64>()
}

// check_decrypted
fn check_decrypted(data: &[u8], headers: &[Phdr]) -> Result<(), String> {
    let loads = headers
        .iter()
        .enumerate()
        .filter(|(_, header)| header.kind == PT_LOAD && header.size != 0);
    let mut encrypted = Vec::new();
    for (index, header) in loads {
        let start = header.source as usize;
        let value = entropy(&data[start..start + header.size.min(ENTROPY_SAMPLE) as usize]);
        if value < ENTROPY_LIMIT {
            return Ok(());
        }
        encrypted.push(format!(
            "segment {index} at {start:#x} ({value:.2} bits per byte)"
        ));
    }
    if encrypted.is_empty() {
        return Ok(());
    }
    Err(format!(
        "every load segment looks encrypted: {}",
        encrypted.join(", ")
    ))
}

// region_for
fn region_for(regions: &[Region], header: &Phdr) -> Option<usize> {
    regions.iter().position(|region| {
        let old = i128::from(region.old);
        old <= header.source
            && header.source + i128::from(header.size) <= old + i128::from(region.size)
    })
}

// output_region
fn output_region(
    regions: &[Region],
    load_regions: &[Option<usize>],
    index: usize,
    header: &Phdr,
) -> Option<usize> {
    let host = if header.kind == PT_LOAD {
        Some(index)
    } else {
        header.host
    };
    host.and_then(|host| load_regions[host])
        .or_else(|| region_for(regions, header))
}

fn region(header: &Phdr, load: Option<usize>) -> Region {
    Region {
        old: header.source as u64,
        size: header.size,
        vaddr: i128::from(header.vaddr),
        align: header.align,
        load,
        new: 0,
    }
}

fn normalized_alignment(align: u64) -> i128 {
    if align.is_power_of_two() {
        i128::from(align)
    } else {
        1
    }
}

pub fn unwrap(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < FIRST_REGION {
        return Err("file is smaller than the SELF header area".into());
    }
    let magic = read_u32(data, 0);
    if !matches!(magic, SELF_MAGIC | SELF_MAGIC_KERNEL) {
        return Err(format!("not a SELF container (magic {magic:#x})"));
    }
    let records = read_records(data);
    let (elf_header, program_offset, mut headers) = parse_elf(data)?;
    place_segments(data, &mut headers, &records)?;
    check_decrypted(data, &headers)?;

    let mut regions: Vec<_> = headers
        .iter()
        .enumerate()
        .filter(|(_, header)| header.kind == PT_LOAD && header.size != 0)
        .map(|(index, header)| region(header, Some(index)))
        .collect();
    for header in &headers {
        if header.kind != PT_LOAD && header.size != 0 && region_for(&regions, header).is_none() {
            regions.push(region(header, None));
        }
    }
    regions.sort_by_key(|region| region.old);
    let mut load_regions = vec![None; headers.len()];
    for (index, region) in regions.iter().enumerate() {
        if let Some(load) = region.load {
            load_regions[load] = Some(index);
        }
    }

    for (index, header) in headers.iter().enumerate() {
        if header.size == 0 && (header.kind == PT_LOAD || header.source == 0) {
            continue;
        }
        if header.align <= 1 || !header.align.is_power_of_two() {
            continue;
        }
        let Some(region_index) = output_region(&regions, &load_regions, index, header) else {
            continue;
        };
        let region = &mut regions[region_index];
        let current = normalized_alignment(region.align);
        let alignment = i128::from(header.align);
        let address = i128::from(header.vaddr) - (header.source - i128::from(region.old));
        if (address - region.vaddr) % current.min(alignment) != 0 {
            return Err(format!(
                "segment {index} has incompatible alignment within its output region"
            ));
        }
        if alignment > current {
            region.align = header.align;
            region.vaddr = address;
        }
    }

    let mut cursor = FIRST_REGION.max(program_offset + headers.len() * PHDR_SIZE) as u128;
    let mut padding = 0;
    for region in &mut regions {
        let alignment = normalized_alignment(region.align);
        let pad = (region.vaddr - cursor as i128).rem_euclid(alignment) as u128;
        padding += pad;
        region.new = cursor + pad;
        cursor = region.new + u128::from(region.size);
    }
    let mut empty_end = cursor;
    for header in &headers {
        if header.kind == PT_LOAD && header.size == 0 {
            empty_end = empty_end.max(u128::from(header.vaddr % header.align.max(1)));
        }
    }
    padding += empty_end - cursor;
    if padding > MAX_ALIGNMENT_PADDING {
        return Err(format!(
            "segment alignment requires more than {MAX_ALIGNMENT_PADDING} bytes of output padding"
        ));
    }
    let output_limit = (MAX_OUTPUT_FACTOR * data.len() as u128).max(MIN_OUTPUT_LIMIT);
    if empty_end > output_limit {
        return Err(format!(
            "the load segments need {empty_end} bytes of output, more than the {output_limit} allowed for a {} byte container",
            data.len()
        ));
    }

    let mut result = vec![0; empty_end as usize];
    for region in &regions {
        let new = region.new as usize;
        let old = region.old as usize;
        let size = region.size as usize;
        result[new..new + size].copy_from_slice(&data[old..old + size]);
    }

    // Rewrite the ELF header and program headers with the relocated offsets.
    result[..ELF_HEADER_SIZE].copy_from_slice(elf_header);
    result[0x28..0x30].fill(0);
    result[0x3C..0x40].fill(0);
    for (index, header) in headers.iter().enumerate() {
        let new = if header.size == 0 && (header.kind == PT_LOAD || header.source == 0) {
            u128::from(if header.kind == PT_LOAD {
                header.vaddr % header.align.max(1)
            } else {
                0
            })
        } else {
            output_region(&regions, &load_regions, index, header).map_or(
                header.source as u128,
                |region_index| {
                    let region = &regions[region_index];
                    region.new + (header.source - i128::from(region.old)) as u128
                },
            )
        };
        if new + u128::from(header.size) > empty_end {
            return Err(format!("segment {index} escaped the output"));
        }
        let start = program_offset + index * PHDR_SIZE;
        let entry = &mut result[start..start + PHDR_SIZE];
        entry[..4].copy_from_slice(&header.kind.to_le_bytes());
        entry[4..8].copy_from_slice(&header.flags.to_le_bytes());
        for (slot, field) in entry[8..].chunks_exact_mut(8).zip([
            new as u64,
            header.vaddr,
            header.vaddr,
            header.size,
            header.memory,
            header.align,
        ]) {
            slot.copy_from_slice(&field.to_le_bytes());
        }
    }
    Ok(result)
}

pub fn unwrap_file(src: &Path, dst: &Path) -> Result<u64, JobError> {
    let refusal = |reason| JobError {
        category: "integrity",
        message: format!(
            "{}: {reason}",
            src.file_name().unwrap_or(src.as_os_str()).to_string_lossy()
        ),
    };

    let mut input = File::open(src)?;
    let source = input.metadata()?;
    match fs::metadata(dst) {
        Ok(destination)
            if (source.dev(), source.ino()) == (destination.dev(), destination.ino()) =>
        {
            return Err(refusal("output aliases the input container".into()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut data = Vec::new();
    input.read_to_end(&mut data)?;
    if !data.starts_with(ELF_MAGIC) {
        data = unwrap(&data).map_err(refusal)?;
    }

    let parent = dst
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let (temporary, mut file) = loop {
        let serial = COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".unwrap-self-{}-{serial}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let written = file.write_all(&data);
    drop(file);
    if let Err(error) = written.and_then(|()| fs::rename(&temporary, dst)) {
        let _ = fs::remove_file(temporary);
        return Err(error.into());
    }
    Ok(data.len() as u64)
}
