//! Synthetic 1+1 and shifted repetitions only; no GUI, capture or answer injection.
//! Run each profile in a fresh process for external peak RSS/private-byte sampling.
//! This measures latency/output, not accuracy or a whole-machine RAM guarantee.
use board_core::StrokePoint;
use board_hwr::{NeuralOptions, NeuralRecognizer};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

const USAGE: &str = "neural_benchmark --model-dir <dir> --profile baseline|low-memory [--rounds 3] [--ink-repeat 1..8] [--intra-threads auto|1|2|3|4] [--prepacking true|false] [--memory-pattern true|false] [--cpu-arena true|false] [--spinning true|false] [--optimization-level 0|1|2|3] [--max-tokens 1..256] [--time-budget-secs 1..60]";

struct Args {
    model_dir: PathBuf,
    profile: String,
    rounds: usize,
    ink_repeat: usize,
    options: NeuralOptions,
}

fn parse_args(args: Vec<String>) -> Result<Args, String> {
    if args.len() % 2 != 0 {
        return Err(USAGE.into());
    }
    let mut flags = std::collections::BTreeMap::new();
    for pair in args.chunks_exact(2) {
        if flags.insert(pair[0].as_str(), pair[1].as_str()).is_some() {
            return Err(format!("duplicate option: {}", pair[0]));
        }
    }
    let model_dir = PathBuf::from(flags.remove("--model-dir").ok_or(USAGE)?);
    if model_dir.as_os_str().is_empty() {
        return Err("--model-dir must not be empty".into());
    }
    let profile = flags.remove("--profile").ok_or(USAGE)?.to_owned();
    let mut options = match profile.as_str() {
        "baseline" => NeuralOptions::baseline(),
        "low-memory" => NeuralOptions::low_memory(),
        _ => return Err(USAGE.into()),
    };
    let number = |value: &str| {
        value
            .parse::<usize>()
            .map_err(|_| format!("invalid integer: {value}"))
    };
    let rounds = number(flags.remove("--rounds").unwrap_or("3"))?;
    if !(1..=1000).contains(&rounds) {
        return Err("--rounds must be 1..=1000".into());
    }
    let ink_repeat = number(flags.remove("--ink-repeat").unwrap_or("1"))?;
    if !(1..=8).contains(&ink_repeat) {
        return Err("--ink-repeat must be 1..=8".into());
    }
    if let Some(value) = flags.remove("--intra-threads") {
        options.intra_threads = if value == "auto" {
            None
        } else {
            Some(number(value)?)
        };
    }
    for (flag, target) in [
        ("--prepacking", &mut options.prepacking),
        ("--memory-pattern", &mut options.memory_pattern),
        ("--cpu-arena", &mut options.cpu_arena),
        ("--spinning", &mut options.spinning),
    ] {
        if let Some(value) = flags.remove(flag) {
            *target = value
                .parse::<bool>()
                .map_err(|_| format!("{flag} requires true|false"))?;
        }
    }
    if let Some(value) = flags.remove("--optimization-level") {
        options.optimization_level = value.parse().map_err(|_| "invalid optimization level")?;
    }
    if let Some(value) = flags.remove("--max-tokens") {
        options.max_tokens = number(value)?;
    }
    if let Some(value) = flags.remove("--time-budget-secs") {
        options.time_budget =
            Duration::from_secs(value.parse().map_err(|_| "invalid time budget")?);
    }
    if !flags.is_empty() {
        return Err(format!(
            "unknown options: {:?}",
            flags.keys().collect::<Vec<_>>()
        ));
    }
    options.validate()?;
    Ok(Args {
        model_dir,
        profile,
        rounds,
        ink_repeat,
        options,
    })
}

fn emit(value: Value) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, &value).map_err(|e| e.to_string())?;
    writeln!(out)
        .and_then(|_| out.flush())
        .map_err(|e| e.to_string())
}

fn one_plus_one() -> Vec<Vec<StrokePoint>> {
    let point = |x, y| StrokePoint {
        x,
        y,
        time: 0.0,
        pressure: 0.5,
    };
    vec![
        vec![point(15., 25.), point(25., 15.), point(25., 85.)],
        vec![point(58., 50.), point(98., 50.)],
        vec![point(78., 30.), point(78., 70.)],
        vec![point(122., 25.), point(132., 15.), point(132., 85.)],
    ]
}

fn repeated_ink(repeat: usize) -> Vec<Vec<StrokePoint>> {
    let base = one_plus_one();
    let mut strokes = Vec::with_capacity(repeat * 6 - 2);
    for index in 0..repeat {
        let shift = index as f32 * 180.0;
        if index > 0 {
            // Connect adjacent 1+1 groups with a drawn plus, not a decoder hint.
            for stroke in &base[1..3] {
                let mut connector = stroke.clone();
                for point in &mut connector {
                    point.x += shift - 100.0;
                }
                strokes.push(connector);
            }
        }
        for stroke in &base {
            let mut shifted = stroke.clone();
            for point in &mut shifted {
                point.x += shift;
            }
            strokes.push(shifted);
        }
    }
    strokes
}

fn run(args: Args) -> Result<(), String> {
    let options = &args.options;
    let strokes = repeated_ink(args.ink_repeat);
    emit(json!({
        "event": "start", "pid": std::process::id(),
        "model_dir": args.model_dir, "profile": args.profile, "rounds": args.rounds,
        "input": if args.ink_repeat == 1 { "synthetic_1_plus_1" } else { "synthetic_shifted_1_plus_1_groups_with_plus_connectors" },
        "ink_repeat": args.ink_repeat, "ink_shift_x": 180,
        "stroke_count": strokes.len(), "point_count": strokes.iter().map(Vec::len).sum::<usize>(),
        "input_shape": [1, 1, 448, 448],
        "decode": "greedy_full_prefix_no_kv_cache", "weight_precision": "not_inspected",
        "options": {
            "intra_threads_requested": options.intra_threads,
            "intra_threads_effective": options.effective_intra_threads()?,
            "inter_threads": 1, "parallel_execution": false,
            "cpu_arena": options.cpu_arena, "memory_pattern": options.memory_pattern,
            "prepacking": options.prepacking, "spinning": options.spinning,
            "optimization_level": options.optimization_level,
            "max_tokens": options.max_tokens, "time_budget_secs": options.time_budget.as_secs_f64()
        },
        "memory_measurement": "external sampler required; no RAM ceiling verified"
    }))?;
    let started = Instant::now();
    let loaded = NeuralRecognizer::load_with_options(&args.model_dir, args.options);
    let load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut recognizer = match loaded {
        Ok(recognizer) => recognizer,
        Err(error) => {
            emit(json!({"event": "load", "ok": false, "load_ms": load_ms, "error": error}))?;
            return Err("model load failed".into());
        }
    };
    emit(
        json!({"event": "load", "ok": true, "load_ms": load_ms, "model_dir": recognizer.model_dir()}),
    )?;
    let mut total_seconds = 0.0;
    let mut total_tokens = 0;
    let mut finished_rounds = 0;
    for round in 1..=args.rounds {
        let started = Instant::now();
        let result = recognizer.recognize(&strokes);
        let seconds = started.elapsed().as_secs_f64();
        total_seconds += seconds;
        match result {
            Ok(result) => {
                total_tokens += result.generated_tokens;
                finished_rounds += usize::from(result.finished);
                emit(json!({
                    "event": "recognize", "round": round, "ok": true,
                    "first_run": round == 1, "recognize_ms": seconds * 1000.0,
                    "generated_tokens": result.generated_tokens,
                    "tokens_per_second": result.generated_tokens as f64 / seconds,
                    "finished_eos": result.finished, "latex": result.latex,
                    "mean_log_probability": result.mean_log_probability
                }))?;
            }
            Err(error) => {
                emit(json!({"event": "recognize", "round": round, "ok": false,
                    "recognize_ms": seconds * 1000.0, "error": error}))?;
                return Err("recognition failed".into());
            }
        }
    }
    emit(json!({
        "event": "summary", "rounds": args.rounds, "finished_eos_rounds": finished_rounds,
        "load_ms": load_ms, "total_recognize_ms": total_seconds * 1000.0,
        "mean_recognize_ms": total_seconds * 1000.0 / args.rounds as f64,
        "total_generated_tokens": total_tokens, "tokens_per_second": total_tokens as f64 / total_seconds
    }))
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("{USAGE}");
        return;
    }
    if let Err(error) = parse_args(args).and_then(run) {
        let _ = emit(json!({"event": "error", "error": error}));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: &[&str]) -> Vec<String> {
        [
            "--model-dir",
            "explicit-model-dir",
            "--profile",
            "low-memory",
        ]
        .into_iter()
        .chain(extra.iter().copied())
        .map(str::to_owned)
        .collect()
    }

    #[test]
    fn explicit_directory_and_profile_are_required() {
        assert!(parse_args(vec![]).is_err());
        assert!(parse_args(vec!["--profile".into(), "baseline".into()]).is_err());
        assert_eq!(
            parse_args(args(&[])).unwrap().options,
            NeuralOptions::default()
        );
    }

    #[test]
    fn repeated_ink_is_shifted_geometry_not_a_prediction() {
        assert_eq!(parse_args(args(&[])).unwrap().ink_repeat, 1);
        assert_eq!(repeated_ink(1), one_plus_one());
        let repeated = repeated_ink(8);
        assert_eq!(repeated.len(), 46);
        let base = one_plus_one();
        for (actual, original) in repeated[42..].iter().zip(&base) {
            for (actual, original) in actual.iter().zip(original) {
                assert_eq!(actual.x, original.x + 7.0 * 180.0);
                assert_eq!(actual.y, original.y);
                assert_eq!(actual.pressure, original.pressure);
                assert_eq!(actual.time, original.time);
            }
        }
        assert_eq!(
            parse_args(args(&["--ink-repeat", "8"])).unwrap().ink_repeat,
            8
        );
    }

    #[test]
    fn overrides_are_reportable_and_validated() {
        let parsed = parse_args(args(&[
            "--intra-threads",
            "1",
            "--prepacking",
            "false",
            "--rounds",
            "2",
        ]))
        .unwrap();
        assert_eq!(parsed.options.intra_threads, Some(1));
        assert!(!parsed.options.prepacking);
        assert_eq!(parsed.rounds, 2);
        for extra in [
            ["--intra-threads", "0"],
            ["--intra-threads", "5"],
            ["--max-tokens", "257"],
            ["--time-budget-secs", "61"],
            ["--optimization-level", "4"],
            ["--prepacking", "yes"],
            ["--rounds", "0"],
            ["--ink-repeat", "0"],
            ["--ink-repeat", "9"],
            ["--unknown", "true"],
            ["--profile", "baseline"],
        ] {
            assert!(parse_args(args(&extra)).is_err(), "{extra:?}");
        }
    }
}
