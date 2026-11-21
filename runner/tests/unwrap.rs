mod common;

use astroport_run::unwrap::{unwrap, unwrap_file};
use common::{build_self, high_entropy, low_entropy, temp_dir, Seg};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, input: &[u8]) -> Self {
        let fixture = Self(temp_dir(name));
        fs::write(fixture.input(), input).unwrap();
        fixture
    }

    fn input(&self) -> PathBuf {
        self.0.join("in.self")
    }

    fn python(&self) -> Option<Output> {
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../engines/anyps5/tools/unwrap_self.py");
        let output = Command::new("timeout")
            .args(["60", "python3", "-I"])
            .arg(script)
            .args(["in.self", "py.elf"])
            .current_dir(&self.0)
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        if output.status.code() == Some(127)
            && String::from_utf8_lossy(&output.stderr).contains("failed to run command 'python3'")
        {
            eprintln!("python3 missing, parity skipped");
            return None;
        }
        Some(output)
    }

    fn assert_python_matches(&self, rust_output: &[u8]) {
        if let Some(output) = self.python() {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(rust_output, fs::read(self.0.join("py.elf")).unwrap());
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn segment(kind: u32, vaddr: u64, data: Vec<u8>, align: u64, recorded: bool) -> Seg {
    Seg {
        kind,
        vaddr,
        memsz: data.len() as u64,
        data,
        align,
        recorded,
    }
}

fn loads() -> Vec<Seg> {
    vec![
        segment(1, 0, low_entropy(0x1008, 5), 0x4000, true),
        segment(1, 0x8000, low_entropy(0x1207, 13), 0x4000, true),
    ]
}

fn phdr_field(elf: &[u8], index: usize, field: usize) -> u64 {
    let start = 0x40 + index * 56 + field;
    u64::from_le_bytes(elf[start..start + 8].try_into().unwrap())
}

#[test]
fn loads_dynlib_and_note_match_python() {
    let mut segs = loads();
    segs.push(segment(0x61000000, 0, low_entropy(0x31, 2), 0x10, true));
    segs.push(segment(4, 0, low_entropy(0x10, 0), 4, false));
    let data = build_self(&segs);
    let fixture = Fixture::new("loads-dynlib-note", &data);
    let output = unwrap(&data).unwrap();
    fixture.assert_python_matches(&output);
    for index in 0..2 {
        let alignment = phdr_field(&output, index, 48);
        assert!(phdr_field(&output, index, 32) > 0);
        assert_eq!(
            phdr_field(&output, index, 8) % alignment,
            phdr_field(&output, index, 16) % alignment
        );
    }
    assert_eq!(phdr_field(&output, 3, 32), 0);
}

#[test]
fn tls_inside_a_load_matches_python() {
    let mut segs = loads();
    segs.push(segment(
        7,
        0x8020,
        segs[1].data[0x20..0x40].to_vec(),
        0x20,
        false,
    ));
    let data = build_self(&segs);
    let fixture = Fixture::new("tls", &data);
    let output = unwrap(&data).unwrap();
    fixture.assert_python_matches(&output);
    assert_eq!(phdr_field(&output, 2, 8), phdr_field(&output, 1, 8) + 0x20);
}

#[test]
fn truncated_version_segment_matches_python() {
    let mut segs = loads();
    segs.push(segment(0x61000000, 0, low_entropy(0x31, 2), 0x10, true));
    segs.push(segment(0x6FFFFF01, 0, low_entropy(0x30, 0), 0x10, false));
    let data = build_self(&segs);
    let fixture = Fixture::new("truncated-version", &data);
    let output = unwrap(&data).unwrap();
    fixture.assert_python_matches(&output);
    assert_eq!(phdr_field(&output, 3, 32), 0);
}

#[test]
fn encrypted_loads_are_refused() {
    let data = build_self(&[
        segment(1, 0, high_entropy(0x10000, 53), 0x4000, true),
        segment(1, 0x10000, high_entropy(0x10000, 97), 0x4000, true),
    ]);
    let fixture = Fixture::new("encrypted", &data);
    let refusal = unwrap(&data).unwrap_err();
    assert!(refusal.starts_with("every load segment looks encrypted: "));
    if let Some(output) = fixture.python() {
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(
            String::from_utf8(output.stderr).unwrap().trim_end(),
            format!("FAIL: in.self: {refusal}")
        );
    }
}

#[test]
fn not_a_self_is_refused() {
    let data = vec![0; 0x400];
    let _fixture = Fixture::new("not-a-self", &data);
    assert_eq!(
        unwrap(&data).unwrap_err(),
        "not a SELF container (magic 0x0)"
    );
}

#[test]
fn plain_elf_is_copied() {
    let mut data = b"\x7fELF".to_vec();
    data.extend(low_entropy(0x500, 7));
    let fixture = Fixture::new("plain-elf", &data);
    let output = fixture.0.join("out.elf");
    assert_eq!(
        unwrap_file(&fixture.input(), &output).unwrap(),
        data.len() as u64
    );
    assert_eq!(fs::read(output).unwrap(), data);
}

#[test]
fn output_never_aliases_input() {
    let data = build_self(&loads());
    let fixture = Fixture::new("alias", &data);
    let input = fixture.input();
    let hard_link = fixture.0.join("hard-link");
    fs::hard_link(&input, &hard_link).unwrap();
    let symlink = fixture.0.join("symlink");
    std::os::unix::fs::symlink("in.self", &symlink).unwrap();

    for output in [&input, &hard_link, &symlink] {
        let error = unwrap_file(&input, output).unwrap_err();
        assert_eq!(error.category, "integrity");
        assert_eq!(error.message, "in.self: output aliases the input container");
        assert_eq!(fs::read(&input).unwrap(), data);
    }
}
