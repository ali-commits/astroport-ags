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

fn run(args: Args) -> Result<(), JobError> {
    let _install = match args.install {
        Some(install) => install,
        None => default_install()?,
    };
    let (_dump, _data, _version) = (args.dump, args.data, args.version);
    Err(JobError {
        category: "internal",
        message: "not built yet".into(),
    })
}

fn main() -> ExitCode {
    let mut progress = Progress::from_env();
    let Some(args) = parse_args(env::args().skip(1)) else {
        progress.error(&JobError {
            category: "internal",
            message: USAGE.into(),
        });
        return ExitCode::from(2);
    };

    match run(args) {
        Ok(()) => {
            progress.completed();
            ExitCode::SUCCESS
        }
        Err(error) => {
            progress.error(&error);
            ExitCode::FAILURE
        }
    }
}
