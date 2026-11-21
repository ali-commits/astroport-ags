use serde_json::{json, Value};
use std::path::Path;

// sheet §3.2: job.plan params {job, game, params, cleanup, resume} and result JobPlan {label, steps, argv, env, cwd, writable, admission (four bools), notify, changes_software, resumable, cleanup}.
// sheet §3.2: job is the id of the button with job: true that was pressed ("build"); game is the adapter game id.
// sheet §3.1: hello lists "jobs"; without it the host drops the job button.
// sheet §3.3: progress "step" is a 0-based index into steps (0..=4), past the end clamped to the last; astroport-run sends 0-based.
// sheet §3.3: a worker exit without a final outcome or error line fails as internal "The job stopped before it finished."
// sheet §3.5: argv[0] <data>/generated/run-<v>/AstroBot is launched only when listed with its SHA-256 in <ext>/.generated.json, written after a completed job (else generated-changed).
// SHEET: whether the host calls library.list again after a completed job is not in the M2-5 sheet (§3.8 sends only page.changed); installed flips on the next library.list.
// SHEET: library entry with installed false: no sheet defines how an unsupported entry hides Play (design 5.1 "no Play button"); the M1 host shows its Install chrome.
pub fn job_plan(install: &Path, data: &Path, dump: &Path) -> Value {
    json!({
        "label": "Build game files",
        "steps": [
            "Check the game dump",
            "Unwrap the game files",
            "Convert for Linux",
            "Set up the game folder",
            "Record the result"
        ],
        "argv": [install.join("bin/astroport-run"), "build", "--dump", dump,
                 "--install", install, "--data", data],
        "env": {},
        "cwd": data,
        "writable": [data.join("generated"), data.join("saves")],
        "admission": {
            "blocks_launch": true,
            "pauses_while_playing": false,
            "runs_while_playing": false,
            "counts_against_downloads": false
        },
        "notify": {"completed": true, "failed": true},
        "changes_software": true,
        "resumable": false,
        "cleanup": false
    })
}

// sheet §6.6: With Step 5 live support, controller.haptics appears once, in the game overlay with no tab (m1 §1.2.2 and §1.3).
// sheet §1.2, §2.2: standard is a map key -> Support {level, live, applier, values, min, max, requires, method}; level honoured|override|fixed|unsupported, live a restart word, applier source|ags.
// sheet §2.1: perf.render_scale values are integer percents, so graphics.render stays an AstroPort item and the key is not declared.
// sheet §2.5: perf.low_latency reaches launch.plan standard from M4 (honoured, applier source); its registry default is "off".
// sheet §2.2: perf.frame_generation absent = AGS offers lsfg for local play; not declaring it is "does not refuse it".
pub fn standard_support() -> Value {
    json!({
        "display.resolution": {"level": "honoured", "live": "live", "applier": "source"},
        "display.fps": {"level": "honoured", "live": "next_launch", "applier": "source", "values": [30, 60]},
        "perf.low_latency": {"level": "honoured", "live": "next_launch", "applier": "source", "values": ["off", "on"]}
    })
}
