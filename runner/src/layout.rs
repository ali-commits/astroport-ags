use crate::JobError;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{self, Path, PathBuf};

pub const NOTO_FONTS: [&str; 24] = [
    "NotoSans-Light.ttf",
    "NotoSans-Regular.ttf",
    "NotoSans-Medium.ttf",
    "NotoSans-Bold.ttf",
    "NotoSans-LightItalic.ttf",
    "NotoSans-Italic.ttf",
    "NotoSans-MediumItalic.ttf",
    "NotoSans-BoldItalic.ttf",
    "NotoSansMono-Light.ttf",
    "NotoSansMono-Regular.ttf",
    "NotoSansMono-Medium.ttf",
    "NotoSansMono-Bold.ttf",
    "NotoSansThai-Light.ttf",
    "NotoSansThai-Regular.ttf",
    "NotoSansThai-Medium.ttf",
    "NotoSansThai-Bold.ttf",
    "NotoSansCJK-Light.ttc",
    "NotoSansCJK-Regular.ttc",
    "NotoSansCJK-Medium.ttc",
    "NotoSansCJK-Bold.ttc",
    "NotoSansArabic-Light.ttf",
    "NotoSansArabic-Regular.ttf",
    "NotoSansArabic-Medium.ttf",
    "NotoSansArabic-Bold.ttf",
];

pub fn default_font_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];
    let user_data = env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        });
    if let Some(dir) = user_data {
        roots.push(dir.join("fonts"));
    }
    roots
}

pub fn find_fonts(roots: &[PathBuf]) -> Result<Vec<PathBuf>, JobError> {
    let mut found = std::array::from_fn(|_| None);
    for root in roots {
        scan_fonts(root, &mut found)?;
        if found.iter().all(Option::is_some) {
            break;
        }
    }
    let missing: Vec<_> = NOTO_FONTS
        .into_iter()
        .zip(&found)
        .filter_map(|(name, path)| path.is_none().then_some(name))
        .collect();
    if !missing.is_empty() {
        return Err(JobError {
            category: "unsupported",
            message: format!("Install the Noto fonts (missing: {}).", missing.join(", ")),
        });
    }
    Ok(found.into_iter().flatten().collect())
}

fn scan_fonts(dir: &Path, found: &mut [Option<PathBuf>; NOTO_FONTS.len()]) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            scan_fonts(&path, found)?;
        } else if let Some(index) = NOTO_FONTS
            .iter()
            .position(|name| entry.file_name() == *name)
        {
            if found[index].is_none() && path.is_file() {
                found[index] = Some(path);
            }
        }
    }
    Ok(())
}

pub fn link_fonts(fonts: &[PathBuf], run: &Path) -> io::Result<()> {
    ensure_dir(run)?;
    let dest = run.join("anyps5-fonts");
    ensure_dir(&dest)?;
    for font in fonts {
        replace_link(&path::absolute(font)?, &dest.join(file_name(font)?))?;
    }
    Ok(())
}

pub fn link_app0(dump: &Path, run: &Path) -> io::Result<()> {
    let dump = path::absolute(dump)?;
    let entries = fs::read_dir(&dump)?;
    ensure_dir(run)?;
    let app0 = run.join("app0");
    ensure_dir(&app0)?;
    for entry in fs::read_dir(&app0)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            fs::remove_file(entry.path())?;
        }
    }
    ensure_dir(&app0.join("sce_module"))?;
    let args = dump.join("args.txt");
    match fs::symlink_metadata(&args) {
        Ok(_) => replace_file(&args, &app0.join("args.txt"))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if name != "sce_module" && name != "args.txt" {
            symlink(entry.path(), app0.join(name))?;
        }
    }
    Ok(())
}

pub fn place_outputs(out: &Path, run: &Path) -> io::Result<()> {
    ensure_dir(run)?;
    let app0 = run.join("app0");
    ensure_dir(&app0)?;
    let modules = app0.join("sce_module");
    ensure_dir(&modules)?;
    let mut placed = HashSet::new();
    move_outputs(out, run, &mut placed)?;
    prune_files(&modules, &placed)
}

fn move_outputs(out: &Path, dest: &Path, placed: &mut HashSet<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(out)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            ensure_dir(&target)?;
            move_outputs(&entry.path(), &target, placed)?;
        } else if kind.is_file() {
            fs::rename(entry.path(), &target)?;
            placed.insert(target);
        }
    }
    Ok(())
}

pub fn install_libs(install: &Path, run: &Path) -> io::Result<usize> {
    ensure_dir(run)?;
    let libs = run.join("libs");
    ensure_dir(&libs)?;
    let mut installed = HashSet::new();
    let mut copy = |src: &Path| -> io::Result<()> {
        let dest = libs.join(file_name(src)?);
        replace_file(src, &dest)?;
        installed.insert(dest);
        Ok(())
    };

    for (dir, required) in [
        ("anyps5/libs", "anyps5/libAstroHybridHost.prx"),
        ("shims", "lib/libkyty_gpu.so"),
    ] {
        for entry in fs::read_dir(install.join(dir))? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "prx") && path.is_file() {
                copy(&path)?;
            }
        }
        copy(&install.join(required))?;
    }
    for entry in fs::read_dir(install.join("lib"))? {
        let entry = entry?;
        if entry
            .file_name()
            .as_encoded_bytes()
            .starts_with(b"libnvidia-ngx-dlss.so.")
            && entry.path().is_file()
        {
            copy(&entry.path())?;
        }
    }
    prune_files(&libs, &installed)?;
    Ok(installed.len())
}

fn prune_files(dir: &Path, keep: &HashSet<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file() && !keep.contains(&path) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn replace_file(src: &Path, dst: &Path) -> io::Result<()> {
    replace(dst, |temp| fs::copy(src, temp).map(|_| ()))
}

fn replace_link(src: &Path, dst: &Path) -> io::Result<()> {
    replace(dst, |temp| symlink(src, temp))
}

fn replace(dst: &Path, create: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    let mut name = std::ffi::OsString::from(".");
    name.push(file_name(dst)?);
    name.push(".astroport-tmp");
    let temp = dst.with_file_name(name);
    match fs::remove_file(&temp) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let result = create(&temp).and_then(|()| fs::rename(&temp, dst));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn file_name(path: &Path) -> io::Result<&std::ffi::OsStr> {
    path.file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))
}

fn ensure_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("{} is not a real directory", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(path),
        Err(error) => Err(error),
    }
}

pub fn link_saves(data: &Path, run: &Path) -> Result<(), JobError> {
    ensure_dir(run)?;
    let data = path::absolute(data)?;
    let saves = data.join("saves");
    ensure_dir(&saves)?;
    for name in ["_sd", "_sd_mem", "_trophy"] {
        let target = saves.join(name);
        let link = run.join(name);
        ensure_dir(&target)?;
        if name == "_sd" {
            ensure_dir(&target.join("sce_sdmemory"))?;
        }
        let removed = match fs::symlink_metadata(&link) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir(&link),
            Ok(metadata) if metadata.is_symlink() => Ok(()),
            Ok(_) => Err(io::ErrorKind::DirectoryNotEmpty.into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
        match removed {
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {
                return Err(JobError {
                    category: "conflict",
                    message: format!(
                        "{} holds files. Move them into {} and build again.",
                        link.display(),
                        target.display()
                    ),
                });
            }
            result => result?,
        }
        replace_link(&target, &link)?;
    }
    ensure_dir(&run.join("data"))?;
    Ok(())
}
