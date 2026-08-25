//! `edge-cli` — codegen from the single-source-of-truth `edge.toml`
//! (SPEC §9).
//!
//! One manifest drives both platforms:
//!
//! * `edge-cli generate` writes `wrangler.toml` (Cloudflare) and
//!   `fastly.toml` (Fastly `[setup]`) from `edge.toml`. Output is
//!   deterministic (origins sorted by alias) and diffable; an existing
//!   `[local_server]` section (Viceroy testing config) is preserved.
//! * `edge-cli check` validates that a deployed `fastly.toml` matches the
//!   origin map and store bindings in `edge.toml` (D6: config drift is the
//!   failure D4 exists to catch).

mod check;
mod codegen;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
edge-cli — Edge SDK codegen (SPEC §9)

USAGE:
    edge-cli generate [--edge-toml <path>] [--out-dir <dir>]
                      [--compatibility-date <yyyy-mm-dd>]
        Writes wrangler.toml + fastly.toml from edge.toml (defaults:
        ./edge.toml, current dir, 2025-08-01).

    edge-cli check [--edge-toml <path>] [--fastly-toml <path>]
        Validates fastly.toml [setup] matches edge.toml origins + stores
        (defaults: ./edge.toml, ./fastly.toml).

    edge-cli --help
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    match args[0].as_str() {
        "generate" => match run_generate(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("edge-cli: {e}");
                ExitCode::FAILURE
            }
        },
        "check" => match run_check(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("edge-cli: {e}");
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("edge-cli: unknown subcommand `{other}`\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run_generate(args: &[String]) -> Result<(), String> {
    let mut edge_toml = PathBuf::from("edge.toml");
    let mut out_dir = PathBuf::from(".");
    let mut compatibility_date = codegen::DEFAULT_COMPATIBILITY_DATE.to_string();
    parse_args(args, &mut |flag, value| match flag {
        "--edge-toml" => {
            edge_toml = PathBuf::from(value);
            Ok(())
        }
        "--out-dir" => {
            out_dir = PathBuf::from(value);
            Ok(())
        }
        "--compatibility-date" => {
            compatibility_date = value.to_string();
            Ok(())
        }
        _ => Err(format!("unknown flag `{flag}`")),
    })?;

    let toml_str = std::fs::read_to_string(&edge_toml)
        .map_err(|e| format!("reading {}: {e}", edge_toml.display()))?;
    let config = edge_core::config::EdgeConfig::from_toml_str(&toml_str)
        .map_err(|e| format!("{e} (from {})", edge_toml.display()))?;

    // The output directory is created on demand (e.g. CI writing to a
    // fresh /tmp dir), so `generate` works in a clean checkout.
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("creating {}: {e}", out_dir.display()))?;

    let existing_fastly = std::fs::read_to_string(out_dir.join("fastly.toml"))
        .ok()
        .filter(|s| !s.trim().is_empty());
    let generated = codegen::fastly_toml(&config, existing_fastly.as_deref());
    let wrangler = codegen::wrangler_toml(&config, &compatibility_date);

    let fastly_path = out_dir.join("fastly.toml");
    std::fs::write(&fastly_path, generated)
        .map_err(|e| format!("writing {}: {e}", fastly_path.display()))?;
    let wrangler_path = out_dir.join("wrangler.toml");
    std::fs::write(&wrangler_path, wrangler)
        .map_err(|e| format!("writing {}: {e}", wrangler_path.display()))?;

    println!(
        "generated {} and {} from {}",
        fastly_path.display(),
        wrangler_path.display(),
        edge_toml.display()
    );
    Ok(())
}

fn run_check(args: &[String]) -> Result<(), String> {
    let mut edge_toml = PathBuf::from("edge.toml");
    let mut fastly_toml = PathBuf::from("fastly.toml");
    parse_args(args, &mut |flag, value| match flag {
        "--edge-toml" => {
            edge_toml = PathBuf::from(value);
            Ok(())
        }
        "--fastly-toml" => {
            fastly_toml = PathBuf::from(value);
            Ok(())
        }
        _ => Err(format!("unknown flag `{flag}`")),
    })?;

    let edge_str = std::fs::read_to_string(&edge_toml)
        .map_err(|e| format!("reading {}: {e}", edge_toml.display()))?;
    let config = edge_core::config::EdgeConfig::from_toml_str(&edge_str)
        .map_err(|e| format!("{e} (from {})", edge_toml.display()))?;

    let fastly_str = std::fs::read_to_string(&fastly_toml)
        .map_err(|e| format!("reading {}: {e}", fastly_toml.display()))?;
    let problems = check::validate(&config, &fastly_str);

    if problems.is_empty() {
        println!(
            "OK: {} origins, {} store bindings — {} matches {}",
            config.origins().count(),
            config.stores().binding_count(),
            fastly_toml.display(),
            edge_toml.display()
        );
        Ok(())
    } else {
        Err(format!(
            "{} problems in {} vs {}:\n  {}",
            problems.len(),
            fastly_toml.display(),
            edge_toml.display(),
            problems.join("\n  ")
        ))
    }
}

/// Minimal flag parser: `--flag value` pairs.
fn parse_args(
    args: &[String],
    apply: &mut dyn FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    let mut i = 0;
    while i < args.len() {
        let flag = &args[i];
        if !flag.starts_with("--") {
            return Err(format!("unexpected positional argument `{flag}`"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("flag `{flag}` requires a value"))?;
        apply(flag, value)?;
        i += 2;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_pairs() {
        let mut seen = Vec::new();
        parse_args(
            &[
                "--edge-toml".into(),
                "a.toml".into(),
                "--out-dir".into(),
                "/tmp/x".into(),
            ],
            &mut |f, v| {
                seen.push((f.to_string(), v.to_string()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            seen,
            vec![
                ("--edge-toml".to_string(), "a.toml".to_string()),
                ("--out-dir".to_string(), "/tmp/x".to_string()),
            ]
        );
    }

    #[test]
    fn parse_args_rejects_positional() {
        assert!(parse_args(&["positional".into()], &mut |_, _| Ok(())).is_err());
    }

    #[test]
    fn generate_creates_missing_out_dir() {
        // CI writes to a fresh dir (e.g. /tmp/hw) that does not exist yet;
        // generate must create it instead of failing with os error 2.
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("nested/new");
        run_generate(&[
            "--edge-toml".into(),
            "../examples/hello-world/edge.toml".into(),
            "--out-dir".into(),
            out.to_str().unwrap().into(),
        ])
        .unwrap();
        assert!(out.join("fastly.toml").is_file());
        assert!(out.join("wrangler.toml").is_file());
    }
}
