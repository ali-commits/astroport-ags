use std::{fs, path::Path};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

fn manifest() -> toml::Table {
    fs::read_to_string(Path::new(ROOT).join("ags-extension.toml"))
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn manifest_is_protocol_2() {
    let m = manifest();
    assert_eq!(m["id"].as_str(), Some("astroport"));
    assert_eq!(m["name"].as_str(), Some("AstroPort"));
    assert!(m["version"].as_str().is_some_and(|v| !v.is_empty()));
    assert_eq!(m["license"].as_str(), Some("GPL-2.0-only"));
    assert_eq!(m["protocol"].as_integer(), Some(2));
    assert_eq!(
        m["roles"].as_array().unwrap(),
        &vec![toml::Value::from("launcher")]
    );
    assert_eq!(m["adapter"].as_str(), Some("bin/astroport-adapter"));
    assert_eq!(
        m["install"]["release"]["asset"].as_str(),
        Some("astroport-{version}-linux-{arch}.tar.zst")
    );
    let ch: Vec<_> = m["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ch, ["stable", "testing"]);
}

#[test]
fn assets_exist_and_are_inert_svg() {
    let m = manifest();
    let assets = m["assets"].as_table().unwrap();
    for key in ["icon", "logo", "badge"] {
        assert!(assets.contains_key(key), "Missing {key} asset");
    }
    for entry in assets.values() {
        let path = entry.as_str().unwrap();
        let svg = fs::read_to_string(Path::new(ROOT).join(path)).unwrap();
        assert!(svg.len() < 4096, "{path} exceeds the SVG size limit");
        assert!(svg.starts_with("<svg "), "{path} has no SVG root");
        assert!(
            svg.contains(r#"viewBox="0 0 64 64""#),
            "{path} has the wrong viewBox"
        );
        for tag in svg.split('<').skip(1) {
            let (tag, _) = tag.split_once('>').unwrap();
            let name = tag
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap();
            assert!(
                ["svg", "g", "rect", "circle", "path", "title"].contains(&name),
                "{path} contains a disallowed element {name}"
            );
            for (end, _) in tag.match_indices('=') {
                let attr = tag[..end].split_whitespace().last().unwrap();
                assert!(
                    !attr.starts_with("on") && !attr.ends_with("href"),
                    "{path} contains a disallowed attribute {attr}"
                );
            }
        }
    }
}
