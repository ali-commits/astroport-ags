#[allow(dead_code)]
mod common;

use astroport_run::layout::{
    find_fonts, install_libs, link_app0, link_fonts, link_saves, place_outputs, NOTO_FONTS,
};
use common::{fake_dump, fake_install, temp_dir};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;

#[test]
fn app0_links_every_dump_entry_but_sce_module() {
    let root = temp_dir("app0");
    let dump = fake_dump(&root.join("[DL] Astro Bot"), "01.019.000", b"elf", &[]);
    let run = root.join("run");
    let app0 = run.join("app0");
    fs::create_dir_all(&app0).unwrap();
    symlink(dump.join("args.txt"), app0.join("args.txt")).unwrap();
    symlink(dump.join("data"), app0.join("old-data")).unwrap();
    link_app0(&dump, &run).unwrap();

    for name in ["eboot.bin", "data", "sce_sys", "param.sfx"] {
        assert_eq!(fs::read_link(app0.join(name)).unwrap(), dump.join(name));
    }
    assert!(app0.join("old-data").symlink_metadata().is_err());
    assert!(app0.join("sce_module").symlink_metadata().unwrap().is_dir());
    let args = app0.join("args.txt");
    assert!(args.symlink_metadata().unwrap().is_file());
    assert_eq!(
        fs::read(&args).unwrap(),
        fs::read(dump.join("args.txt")).unwrap()
    );
    fs::write(&args, b"private arguments").unwrap();
    assert_eq!(
        fs::read(dump.join("args.txt")).unwrap(),
        b"-package -sequence product"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sce_module_link_is_replaced_not_followed() {
    let root = temp_dir("app0-module-link");
    let modules = [
        ("libc.prx", b"libc".to_vec()),
        ("libkernel.prx", b"kernel".to_vec()),
    ];
    let dump = fake_dump(&root.join("dump"), "01.019.000", b"elf", &modules);
    let run = root.join("run");
    let dest = run.join("app0/sce_module");
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    symlink(dump.join("sce_module"), &dest).unwrap();
    link_app0(&dump, &run).unwrap();

    assert!(dest.symlink_metadata().unwrap().is_dir());
    assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
    assert_eq!(
        fs::read_dir(dump.join("sce_module")).unwrap().count(),
        modules.len()
    );
    for (name, bytes) in modules {
        assert_eq!(fs::read(dump.join("sce_module").join(name)).unwrap(), bytes);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn shims_win_over_anyps5_modules() {
    let root = temp_dir("shims");
    let install = fake_install(&root.join("install"));
    let run = root.join("run");
    assert_eq!(install_libs(&install, &run).unwrap(), 8);
    let libs = run.join("libs");
    assert_eq!(
        fs::read_to_string(libs.join("libSceAgc.prx")).unwrap(),
        "shim libSceAgc.prx"
    );
    assert_eq!(
        fs::read_to_string(libs.join("libkernel.prx")).unwrap(),
        "anyps5 libkernel.prx"
    );
    assert!(libs.join("libAstroHybridHost.prx").is_file());
    assert!(libs.join("libkyty_gpu.so").is_file());
    assert_eq!(
        fs::read(libs.join("libnvidia-ngx-dlss.so.310.9.16")).unwrap(),
        b"dlss"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn no_dlss_runtime_is_fine() {
    let root = temp_dir("no-dlss");
    let install = fake_install(&root.join("install"));
    fs::remove_file(install.join("lib/libnvidia-ngx-dlss.so.310.9.16")).unwrap();
    let run = root.join("run");
    let stale = run.join("libs/libnvidia-ngx-dlss.so.1");
    fs::create_dir_all(stale.parent().unwrap()).unwrap();
    fs::write(&stale, b"old dlss").unwrap();
    assert_eq!(install_libs(&install, &run).unwrap(), 7);
    assert!(!stale.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn replaced_file_keeps_the_old_inode_alive() {
    let root = temp_dir("old-inode");
    let install = fake_install(&root.join("install"));
    let run = root.join("run");
    install_libs(&install, &run).unwrap();
    let dest = run.join("libs/libc.prx");
    let mut old = File::open(&dest).unwrap();
    let source = install.join("anyps5/libs/libc.prx");
    fs::write(&source, b"new libc").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o751)).unwrap();

    install_libs(&install, &run).unwrap();
    let mut bytes = Vec::new();
    old.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"anyps5 libc.prx");
    assert_eq!(fs::read(&dest).unwrap(), b"new libc");
    assert_eq!(dest.metadata().unwrap().permissions().mode() & 0o777, 0o751);
    assert!(fs::read_dir(run.join("libs")).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".astroport-tmp")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_libs_are_removed() {
    let root = temp_dir("stale-libs");
    let install = fake_install(&root.join("install"));
    let run = root.join("run");
    let stale = run.join("libs/old.prx");
    fs::create_dir_all(stale.parent().unwrap()).unwrap();
    fs::write(&stale, b"old lib").unwrap();
    install_libs(&install, &run).unwrap();
    assert!(!stale.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fonts_found_under_nested_roots() {
    let root = temp_dir("nested-fonts");
    let roots = [root.join("first"), root.join("second")];
    let expected: Vec<PathBuf> = NOTO_FONTS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let nested = roots[i % 2].join("nested/fonts");
            fs::create_dir_all(&nested).unwrap();
            let path = nested.join(name);
            fs::write(&path, b"").unwrap();
            path
        })
        .collect();
    fs::write(roots[1].join(NOTO_FONTS[0]), b"duplicate").unwrap();
    let fonts = find_fonts(&roots).unwrap();
    assert_eq!(fonts, expected);
    let run = root.join("run");
    link_fonts(&fonts, &run).unwrap();
    link_fonts(&fonts, &run).unwrap();
    let links = run.join("anyps5-fonts");
    assert_eq!(fs::read_dir(&links).unwrap().count(), 24);
    for (name, path) in NOTO_FONTS.iter().zip(expected) {
        assert_eq!(fs::read_link(links.join(name)).unwrap(), path);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_fonts_name_them() {
    let root = temp_dir("missing-fonts");
    for name in NOTO_FONTS {
        if name != "NotoSansCJK-Bold.ttc" {
            fs::write(root.join(name), b"").unwrap();
        }
    }
    let error = find_fonts(std::slice::from_ref(&root)).unwrap_err();
    assert_eq!(error.category, "unsupported");
    assert_eq!(
        error.message,
        "Install the Noto fonts (missing: NotoSansCJK-Bold.ttc)."
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn saves_are_seeded_and_linked() {
    let root = temp_dir("saves");
    let data = root.join("data");
    let run = root.join("run");
    fs::create_dir_all(run.join("_sd_mem")).unwrap();
    link_saves(&data, &run).unwrap();
    assert!(data.join("saves/_sd/sce_sdmemory").is_dir());
    assert!(run.join("data").symlink_metadata().unwrap().is_dir());
    for name in ["_sd", "_sd_mem", "_trophy"] {
        assert_eq!(
            fs::read_link(run.join(name)).unwrap(),
            data.join("saves").join(name)
        );
    }
    link_saves(&data, &run).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_save_folder_with_files_stops() {
    let root = temp_dir("save-conflict");
    let data = root.join("data");
    let run = root.join("run");
    let slot = run.join("_sd_mem/u268435456/slot0.bin");
    fs::create_dir_all(slot.parent().unwrap()).unwrap();
    fs::write(&slot, b"saved game").unwrap();

    let error = link_saves(&data, &run).unwrap_err();
    assert_eq!(error.category, "conflict");
    assert_eq!(
        error.message,
        format!(
            "{} holds files. Move them into {} and build again.",
            run.join("_sd_mem").display(),
            data.join("saves/_sd_mem").display()
        )
    );
    assert_eq!(fs::read(&slot).unwrap(), b"saved game");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn place_outputs_moves_and_prunes_guest_modules() {
    let root = temp_dir("outputs");
    let out = root.join("out");
    let run = root.join("run");
    let modules = "app0/sce_module";
    let stale = run.join(modules).join("old.prx.guest.prx");
    fs::create_dir_all(out.join(modules)).unwrap();
    fs::create_dir_all(run.join(modules)).unwrap();
    fs::write(&stale, b"stale module").unwrap();
    let paths = [
        "AstroBot",
        "AstroBot.registry.json",
        "app0/sce_module/libc.prx.guest.prx",
    ];
    for path in paths {
        fs::write(out.join(path), path).unwrap();
    }

    place_outputs(&out, &run).unwrap();
    for path in paths {
        assert_eq!(fs::read_to_string(run.join(path)).unwrap(), path);
        assert!(!out.join(path).exists());
    }
    assert!(!stale.exists());
    assert_eq!(fs::read_dir(out.join(modules)).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}
