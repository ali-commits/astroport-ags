use std::{
    cell::OnceCell,
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    pub gpu: String,
    pub vram_mib: Option<u64>,
    pub screen: Option<(u32, u32)>,
    pub nvidia: bool,
}

pub fn probe(sys: &Path, nvidia: impl Fn() -> Option<String>) -> Device {
    let mut result = Device {
        gpu: "unknown".into(),
        ..Device::default()
    };
    let Ok(entries) = fs::read_dir(sys.join("class/drm")) else {
        return result;
    };
    let nvidia_output = OnceCell::new();
    let mut best_gpu = None;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(card) = name.to_str().and_then(|name| name.strip_prefix("card")) else {
            continue;
        };
        let (number, connector) = match card.split_once('-') {
            Some((number, _)) => (number, true),
            None => (card, false),
        };
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }

        let path = entry.path();
        if connector {
            if let Some(mode) = connected_mode(&path) {
                if result.screen.is_none_or(|(_, height)| mode.1 > height) {
                    result.screen = Some(mode);
                }
            }
            continue;
        }

        let path = path.join("device");
        let Ok(vendor) = fs::read_to_string(path.join("vendor")) else {
            continue;
        };
        let is_nvidia = vendor.trim() == "0x10de";
        let (gpu, vram_mib) = match vendor.trim() {
            "0x10de" => nvidia_output
                .get_or_init(&nvidia)
                .as_deref()
                .and_then(|output| output.lines().next())
                .and_then(|line| line.rsplit_once(','))
                .map_or(("NVIDIA", None), |(name, memory)| {
                    (name.trim(), memory.trim().parse().ok())
                }),
            "0x1002" => {
                let memory = fs::read_to_string(path.join("mem_info_vram_total"))
                    .ok()
                    .and_then(|bytes| bytes.trim().parse::<u64>().ok())
                    .map(|bytes| bytes / (1024 * 1024));
                ("AMD", memory)
            }
            _ => ("unknown", None),
        };
        if best_gpu.is_none_or(|best| vram_mib > best) {
            best_gpu = Some(vram_mib);
            result.gpu = gpu.into();
            result.vram_mib = vram_mib;
            result.nvidia = is_nvidia;
        }
    }
    result
}

fn connected_mode(path: &Path) -> Option<(u32, u32)> {
    if fs::read_to_string(path.join("status")).ok()?.trim() != "connected" {
        return None;
    }
    let modes = fs::read_to_string(path.join("modes")).ok()?;
    let (width, height) = modes.lines().next()?.trim().split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

pub fn nvidia_smi() -> Option<String> {
    let mut child = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return String::from_utf8(child.wait_with_output().ok()?.stdout).ok();
            }
            Ok(Some(_)) => return None,
            Ok(None) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if !remaining.is_zero() {
                    thread::sleep(remaining.min(Duration::from_millis(10)));
                    continue;
                }
            }
            Err(_) => {}
        }
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
}

#[derive(Debug, PartialEq)]
pub struct Defaults {
    pub render: &'static str,
    pub lighting: &'static str,
    pub dlss: bool,
}

pub fn defaults(d: &Device) -> Defaults {
    let vram = d.vram_mib.unwrap_or(0);
    let screen_height = d.screen.map_or(1080, |(_, height)| height);
    let render = [(2160, 16, "2160p"), (1872, 12, "1872p"), (1368, 8, "1368p")]
        .into_iter()
        .find(|(height, gib, _)| *height <= screen_height && gib * 1024 <= vram)
        .map_or("1080p", |(_, _, name)| name);
    Defaults {
        render,
        lighting: if vram >= 16 * 1024 {
            "original"
        } else {
            "fast"
        },
        dlss: d.nvidia,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::{tempdir, TempDir};

    fn fake(cards: &[(&str, &str, Option<u64>)], screens: &[(&str, &str, &str)]) -> TempDir {
        let sys = tempdir().unwrap();
        let drm = sys.path().join("class/drm");
        for (card, vendor, vram_bytes) in cards {
            let device = drm.join(card).join("device");
            fs::create_dir_all(&device).unwrap();
            fs::write(device.join("vendor"), vendor).unwrap();
            if let Some(bytes) = vram_bytes {
                fs::write(device.join("mem_info_vram_total"), bytes.to_string()).unwrap();
            }
        }
        for (connector, status, modes) in screens {
            let connector = drm.join(connector);
            fs::create_dir_all(&connector).unwrap();
            fs::write(connector.join("status"), status).unwrap();
            fs::write(connector.join("modes"), modes).unwrap();
        }
        sys
    }

    #[test]
    fn nvidia_4k() {
        let sys = fake(
            &[("card1", "0x10de\n", None)],
            &[("card1-DP-1", "connected\n", "3840x2160\n")],
        );
        let d = probe(sys.path(), || {
            Some("NVIDIA GeForce RTX 4080, 16376\nAnother GPU, 24576\n".into())
        });
        assert_eq!(
            d,
            Device {
                gpu: "NVIDIA GeForce RTX 4080".into(),
                vram_mib: Some(16376),
                screen: Some((3840, 2160)),
                nvidia: true,
            }
        );
        assert_eq!(
            defaults(&d),
            Defaults {
                render: "1872p",
                lighting: "fast",
                dlss: true,
            }
        );
    }

    #[test]
    fn amd_vram_from_sysfs() {
        let sys = fake(
            &[
                ("card0", "0x1002", Some(17179869184)),
                ("card1", "0x10de", None),
            ],
            &[("card0-DP-1", "connected", "3840x2160")],
        );
        let d = probe(sys.path(), || Some("NVIDIA GeForce RTX 4080, 16376".into()));
        assert_eq!(
            d,
            Device {
                gpu: "AMD".into(),
                vram_mib: Some(16384),
                screen: Some((3840, 2160)),
                nvidia: false,
            }
        );
        assert_eq!(
            defaults(&d),
            Defaults {
                render: "2160p",
                lighting: "original",
                dlss: false,
            }
        );
    }

    #[test]
    fn screen_caps_render() {
        let sys = fake(
            &[("card0", "0x1002", Some(24 * 1024 * 1024 * 1024))],
            &[
                ("card0-HDMI-A-1", "connected", "3840x1080"),
                ("card0-DP-1", "connected", "2560x1440\n3840x2160"),
            ],
        );
        let d = probe(sys.path(), || None);
        assert_eq!(d.screen, Some((2560, 1440)));
        assert_eq!(defaults(&d).render, "1368p");
    }

    #[test]
    fn unknown_device_is_safe() {
        let sys = fake(&[("card0", "0x8086", None)], &[]);
        let d = probe(sys.path(), || None);
        assert_eq!(
            d,
            Device {
                gpu: "unknown".into(),
                ..Device::default()
            }
        );
        let safe = Defaults {
            render: "1080p",
            lighting: "fast",
            dlss: false,
        };
        assert_eq!(defaults(&Device::default()), safe);
        assert_eq!(defaults(&d), safe);
    }

    #[test]
    fn disconnected_screen_ignored() {
        let sys = fake(
            &[("card0", "0x1002", Some(17179869184))],
            &[("card0-DP-1", "disconnected", "7680x4320")],
        );
        let d = probe(sys.path(), || None);
        assert_eq!(d.screen, None);
        assert_eq!(defaults(&d).render, "1080p");
    }
}
