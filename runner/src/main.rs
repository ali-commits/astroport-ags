use astroport_run::build::{build, status, BuildArgs};
use astroport_run::layout::default_font_roots;
use astroport_run::{progress::Progress, JobError};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: astroport-run build --dump <DIR> --data <DIR> [--install <DIR>] | status --dump <DIR> --data <DIR> --version <V> [--install <DIR>]";

struct Args {
    dump: PathBuf,
    data: PathBuf,
    install: Option<PathBuf>,
    version: Option<String>,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Option<Args> {
    let status = match args.next()?.as_str() {
        "build" => false,
        "status" => true,
        _ => return None,
    };

    let (mut dump, mut data, mut install, mut version) = (None, None, None, None);
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--dump" => &mut dump,
            "--data" => &mut data,
            "--install" => &mut install,
            "--version" if status => &mut version,
            _ => return None,
        };
        let value = args.next()?;
        if slot.is_some() || value.is_empty() || value.starts_with("--") {
            return None;
        }
        *slot = Some(value);
    }

    if status && version.is_none() {
        return None;
    }
    Some(Args {
        dump: PathBuf::from(dump?),
        data: PathBuf::from(data?),
        install: install.map(PathBuf::from),
        version,
    })
}

fn default_install() -> Result<PathBuf, JobError> {
    let executable = env::current_exe()?;
    executable
        .parent()
        .and_then(|dir| dir.parent())
        .map(PathBuf::from)
        .ok_or_else(|| JobError {
            category: "internal",
            message: "Cannot determine the install folder".into(),
        })
}

fn main() -> ExitCode {
    let Some(args) = parse_args(env::args().skip(1)) else {
        Progress::from_env().error(&JobError {
            category: "internal",
            message: USAGE.into(),
        });
        return ExitCode::from(2);
    };

    if let Some(version) = args.version {
        let install = args.install.map_or_else(default_install, Ok);
        let state = install.map_or("missing", |install| {
            status(&args.dump, &args.data, &version, &install)
        });
        println!(r#"{{"state":"{state}"}}"#);
        return ExitCode::SUCCESS;
    }

    let mut progress = Progress::from_env();
    let result = args
        .install
        .map_or_else(default_install, Ok)
        .and_then(|install| {
            build(
                &BuildArgs {
                    dump: args.dump,
                    data: args.data,
                    install,
                },
                &default_font_roots(),
                &mut progress,
            )
        });
    match result {
        Ok(_) => {
            progress.completed();
            ExitCode::SUCCESS
        }
        Err(error) => {
            progress.error(&error);
            ExitCode::FAILURE
        }
    }
}
