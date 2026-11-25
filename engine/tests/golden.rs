//! Golden tests (EN-12): `fixtures/<case>/context.json` → `expected.json`.
//!
//! Each case also checks determinism (10 runs, shuffled input order) and the
//! ENGINE §7 invariants. `SECRETARY_BLESS=1` rewrites `expected.json`;
//! review every diff by hand, and bump `ALGORITHM_VERSION` when behaviour
//! changes on purpose.

use std::fs;
use std::path::PathBuf;

use secretary_engine::calculate::{check_invariants, run};
use secretary_engine::model::{Context, Output};

fn cases() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut out: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("fixtures dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("context.json").is_file())
        .collect();
    out.sort();
    out
}

#[test]
fn golden_cases() {
    let bless = std::env::var("SECRETARY_BLESS").is_ok_and(|v| v == "1");
    let cases = cases();
    assert!(!cases.is_empty(), "no fixtures");
    let mut failures = Vec::new();
    for case in cases {
        let name = case.file_name().unwrap().to_string_lossy().into_owned();
        let ctx: Context =
            serde_json::from_str(&fs::read_to_string(case.join("context.json")).unwrap())
                .unwrap_or_else(|e| panic!("{name}: bad context.json: {e}"));
        let out = run(&ctx);
        check_invariants(&ctx, &out).unwrap_or_else(|e| panic!("{name}: {e}"));

        // Determinism: same output for any input order, every time.
        let mut shuffled = ctx.clone();
        for i in 0..10 {
            let n = i.min(shuffled.events.len().saturating_sub(1));
            shuffled.events.rotate_left(n);
            if i % 2 == 1 {
                shuffled.events.reverse();
            }
            assert_eq!(run(&shuffled), out, "{name}: output depends on input order");
        }

        let path = case.join("expected.json");
        let actual = serde_json::to_string_pretty(&out).unwrap() + "\n";
        if bless {
            fs::write(&path, &actual).unwrap();
            continue;
        }
        let expected: Output = match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s)
                .unwrap_or_else(|e| panic!("{name}: bad expected.json: {e}")),
            Err(_) => {
                failures.push(format!(
                    "{name}: missing expected.json (run with SECRETARY_BLESS=1)"
                ));
                continue;
            }
        };
        if expected != out {
            failures.push(format!(
                "{name}: output differs from expected.json:\n{actual}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
