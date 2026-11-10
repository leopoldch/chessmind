//! Calibrates the win/draw/loss model behind `SearchScore::wdl` and
//! `SearchScore::win_percent`.
//!
//! Plays the engine against itself at fixed depth from the balanced opening
//! set, records the search score of every move together with the result of the
//! game, and fits the two parameters of the model by maximum likelihood:
//!
//!   P(win)  = 1 / (1 + exp((a - cp) / b))
//!   P(loss) = 1 / (1 + exp((a + cp) / b))
//!   P(draw) = 1 - P(win) - P(loss)
//!
//! The same score is worth much less in an endgame than in a middlegame, so
//! `a` and `b` depend on the game phase (24 = all pieces, 0 = pawns only): they
//! are fitted at phases 0, 8, 16 and 24 and interpolated linearly in between.
//! The output gives the `WDL_A` / `WDL_B` (phase-independent) and
//! `WDL_A_BY_PHASE` / `WDL_B_BY_PHASE` constants of `engine.rs`, with a
//! reliability table (predicted against observed, per score bucket).
//!
//! Usage: cargo run --release --bin calibrate -- [--games N] [--depth D]
//!            [--threads T] [--seed S] [--csv FILE] [--load FILE]
//!
//! `--csv` saves the samples; `--load` refits from a saved file without playing.

use chessmind::engine::{Engine, SearchScore, TimeConfig};
use chessmind::eval::game_phase;
use chessmind::fen::parse_fen;
use chessmind::game::Outcome;
use chessmind::pieces::Color;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const OPENINGS: &str = include_str!("../../openings/balanced.epd");
const MAX_PLIES: usize = 400;
/// Mate scores are recorded as this many centipawns.
const MATE_CP: i32 = 3000;
/// Transposition table entries per game (16 bytes each).
const TABLE_ENTRIES: usize = 1 << 18;

/// One searched position: score and game phase, with the result of the game,
/// all from the point of view of the side to move.
#[derive(Clone, Copy)]
struct Sample {
    cp: i32,
    phase: i32,
    /// 2 = win, 1 = draw, 0 = loss.
    result: u8,
}

struct Config {
    games: usize,
    depth: u32,
    threads: usize,
    seed: u64,
    csv: Option<String>,
    load: Option<String>,
}

fn parse_args() -> Config {
    let mut cfg = Config {
        games: 1000,
        depth: 10,
        threads: 4,
        seed: 1,
        csv: None,
        load: None,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--games" => cfg.games = value.parse().expect("--games N"),
            "--depth" => cfg.depth = value.parse().expect("--depth D"),
            "--threads" => cfg.threads = value.parse::<usize>().expect("--threads T").max(1),
            "--seed" => cfg.seed = value.parse().expect("--seed S"),
            "--csv" => cfg.csv = Some(value),
            "--load" => cfg.load = Some(value),
            other => {
                eprintln!("unknown argument '{other}'");
                std::process::exit(2);
            }
        }
        i += 2;
    }
    cfg
}

/// Plays one game from `fen` and returns its samples, or None when the game
/// did not reach a result within `MAX_PLIES`.
fn play_game(fen: &str, depth: u32) -> Option<Vec<Sample>> {
    let mut game = parse_fen(fen).ok()?.game;
    let mut engine = Engine::with_threads_and_table(depth, 1, TABLE_ENTRIES);
    let config = TimeConfig::fixed_depth(depth);
    let mut searched: Vec<(Color, i32, i32)> = Vec::new();

    let outcome = loop {
        if let Some(outcome) = game.outcome() {
            break outcome;
        }
        if searched.len() >= MAX_PLIES {
            return None;
        }
        let side = game.current_turn;
        let phase = game_phase(&game.board);
        let stop = Arc::new(AtomicBool::new(false));
        let ((from, to), _) = engine.best_move_timed_opts(&mut game, &config, false, stop)?;
        let cp = match engine.last_search_score()? {
            SearchScore::Centipawns(cp) => cp.clamp(-MATE_CP, MATE_CP),
            SearchScore::Mate(n) if n > 0 => MATE_CP,
            SearchScore::Mate(_) => -MATE_CP,
        };
        searched.push((side, cp, phase));
        if !game.make_move(&from, &to) {
            return None;
        }
    };

    let winner = match outcome {
        Outcome::Win(color) => Some(color),
        Outcome::Draw(_) => None,
    };
    Some(
        searched
            .into_iter()
            .map(|(side, cp, phase)| Sample {
                cp,
                phase,
                result: match winner {
                    Some(color) if color == side => 2,
                    Some(_) => 0,
                    None => 1,
                },
            })
            .collect(),
    )
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Model parameters: `a` and `b` at phases 0, 8, 16 and 24. The
/// phase-independent model simply has the same value at every node.
#[derive(Clone, Copy)]
struct Params {
    a: [f64; 4],
    b: [f64; 4],
}

impl Params {
    fn flat(a: f64, b: f64) -> Self {
        Self { a: [a; 4], b: [b; 4] }
    }

    /// (a, b) at `phase`, interpolated linearly between the two nearest nodes.
    fn at(&self, phase: i32) -> (f64, f64) {
        let x = (phase as f64).clamp(0.0, 24.0) / 8.0;
        let i = (x as usize).min(2);
        let t = x - i as f64;
        (
            self.a[i] + (self.a[i + 1] - self.a[i]) * t,
            self.b[i] + (self.b[i + 1] - self.b[i]) * t,
        )
    }

    /// (win, draw, loss) probabilities for a score in centipawns.
    fn wdl(&self, cp: i32, phase: i32) -> (f64, f64, f64) {
        let (a, b) = self.at(phase);
        let cp = cp as f64;
        let win = sigmoid((cp - a) / b);
        let loss = sigmoid((-cp - a) / b);
        (win, (1.0 - win - loss).max(0.0), loss)
    }
}

/// Mean negative log-likelihood of the samples under the model.
fn loss(samples: &[Sample], params: &Params) -> f64 {
    let mut total = 0.0;
    for s in samples {
        let (win, draw, loss) = params.wdl(s.cp, s.phase);
        let p = match s.result {
            2 => win,
            1 => draw,
            _ => loss,
        };
        total -= p.max(1e-9).ln();
    }
    total / samples.len().max(1) as f64
}

/// Maximum-likelihood fit by coordinate search with a shrinking step. With
/// `by_phase` the eight node values move independently, otherwise `a` and `b`
/// are the same at every phase.
fn fit(samples: &[Sample], by_phase: bool, start: Params) -> Params {
    let mut params = start;
    let mut best = loss(samples, &params);
    let mut step = 64.0;
    while step > 0.1 {
        let mut improved = false;
        let coordinates = if by_phase { 8 } else { 2 };
        for coordinate in 0..coordinates {
            for delta in [step, -step] {
                let mut candidate = params;
                if by_phase {
                    if coordinate < 4 {
                        candidate.a[coordinate] += delta;
                    } else {
                        candidate.b[coordinate - 4] += delta;
                    }
                } else if coordinate == 0 {
                    candidate = Params::flat(params.a[0] + delta, params.b[0]);
                } else {
                    candidate = Params::flat(params.a[0], params.b[0] + delta);
                }
                if candidate.a.iter().any(|&a| a < 0.0) || candidate.b.iter().any(|&b| b < 5.0) {
                    continue;
                }
                let value = loss(samples, &candidate);
                if value < best {
                    params = candidate;
                    best = value;
                    improved = true;
                }
            }
        }
        if !improved {
            step /= 2.0;
        }
    }
    params
}

fn print_reliability(samples: &[Sample], params: &Params) {
    const EDGES: [i32; 12] = [
        -100_000, -600, -350, -200, -100, -40, 40, 100, 200, 350, 600, 100_000,
    ];
    println!(
        "{:>14} {:>8} | {:>6} {:>6} | {:>6} {:>6} | {:>6} {:>6} | {:>7} {:>7}",
        "cp range", "n", "win", "obs", "draw", "obs", "loss", "obs", "score", "obs"
    );
    for pair in EDGES.windows(2) {
        let bucket: Vec<&Sample> = samples
            .iter()
            .filter(|s| s.cp >= pair[0] && s.cp < pair[1])
            .collect();
        if bucket.is_empty() {
            continue;
        }
        let n = bucket.len() as f64;
        let (mut pw, mut pd, mut pl) = (0.0, 0.0, 0.0);
        let (mut ow, mut od, mut ol) = (0.0, 0.0, 0.0);
        for s in &bucket {
            let (win, draw, loss) = params.wdl(s.cp, s.phase);
            pw += win;
            pd += draw;
            pl += loss;
            match s.result {
                2 => ow += 1.0,
                1 => od += 1.0,
                _ => ol += 1.0,
            }
        }
        let pct = |x: f64| 100.0 * x / n;
        let label = match (pair[0] < -10_000, pair[1] > 10_000) {
            (true, _) => format!("< {}", pair[1]),
            (_, true) => format!(">= {}", pair[0]),
            _ => format!("{}..{}", pair[0], pair[1]),
        };
        println!(
            "{:>14} {:>8} | {:>6.1} {:>6.1} | {:>6.1} {:>6.1} | {:>6.1} {:>6.1} | {:>7.1} {:>7.1}",
            label,
            bucket.len(),
            pct(pw),
            pct(ow),
            pct(pd),
            pct(od),
            pct(pl),
            pct(ol),
            pct(pw + pd / 2.0),
            pct(ow + od / 2.0),
        );
    }
}

/// Mean absolute gap, in percentage points, between the predicted and the
/// observed expected score over (phase band, score bucket) cells.
fn calibration_error(samples: &[Sample], params: &Params) -> f64 {
    use std::collections::BTreeMap;
    let mut cells: BTreeMap<(i32, i32), (f64, f64, f64)> = BTreeMap::new();
    for s in samples {
        let (win, draw, _) = params.wdl(s.cp, s.phase);
        let cell = cells
            .entry((s.phase / 8, s.cp.clamp(-800, 800).div_euclid(100)))
            .or_default();
        cell.0 += 1.0;
        cell.1 += win + draw / 2.0;
        cell.2 += s.result as f64 / 2.0;
    }
    let total: f64 = cells.values().map(|c| c.0).sum();
    cells
        .values()
        .map(|&(n, predicted, observed)| (predicted - observed).abs() / n * (n / total))
        .sum::<f64>()
        * 100.0
}

fn load_samples(path: &str) -> Vec<Sample> {
    let text = std::fs::read_to_string(path).expect("cannot read csv");
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split(',');
            Some(Sample {
                cp: fields.next()?.parse().ok()?,
                phase: fields.next()?.parse().ok()?,
                result: fields.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// Plays the calibration games and returns their samples.
fn generate_samples(cfg: &Config) -> Vec<Sample> {
    let openings: Vec<&str> = OPENINGS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    // Deterministic shuffle of the opening indices (xorshift64*).
    let mut order: Vec<usize> = (0..openings.len()).collect();
    let mut state = cfg.seed.max(1);
    for i in (1..order.len()).rev() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let r = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        order.swap(i, (r % (i as u64 + 1)) as usize);
    }
    let games = cfg.games.min(order.len());

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let samples: Mutex<Vec<Sample>> = Mutex::new(Vec::new());

    std::thread::scope(|scope| {
        for _ in 0..cfg.threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= games {
                        break;
                    }
                    if let Some(game) = play_game(openings[order[index]], cfg.depth) {
                        samples.lock().unwrap().extend(game);
                    }
                    let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if finished.is_multiple_of(50) {
                        eprintln!("{finished}/{games} games");
                    }
                }
            });
        }
    });

    samples.into_inner().unwrap()
}

fn main() {
    let cfg = parse_args();
    let samples = match &cfg.load {
        Some(path) => load_samples(path),
        None => generate_samples(&cfg),
    };
    let count = |result: u8| samples.iter().filter(|s| s.result == result).count();
    println!(
        "{} positions (side to move: +{} ={} -{})",
        samples.len(),
        count(2),
        count(1),
        count(0)
    );
    if samples.is_empty() {
        return;
    }

    if let Some(path) = &cfg.csv {
        let mut file = std::fs::File::create(path).expect("cannot create csv");
        writeln!(file, "cp,phase,result").unwrap();
        for s in &samples {
            writeln!(file, "{},{},{}", s.cp, s.phase, s.result).unwrap();
        }
    }

    let flat = fit(&samples, false, Params::flat(150.0, 120.0));
    let phased = fit(&samples, true, flat);

    println!("\nphase-independent model:");
    println!("  WDL_A = {:.1}, WDL_B = {:.1}", flat.a[0], flat.b[0]);
    println!(
        "  log-loss {:.4}, calibration error {:.2} points",
        loss(&samples, &flat),
        calibration_error(&samples, &flat)
    );

    println!("\nphase-aware model (phases 0, 8, 16, 24):");
    println!("  WDL_A_BY_PHASE = {:.1?}", phased.a);
    println!("  WDL_B_BY_PHASE = {:.1?}", phased.b);
    println!(
        "  log-loss {:.4}, calibration error {:.2} points",
        loss(&samples, &phased),
        calibration_error(&samples, &phased)
    );

    println!("\nreliability of the phase-aware model, predicted vs observed (%):");
    print_reliability(&samples, &phased);
    for (lo, hi, name) in [(0, 8, "endgame"), (8, 16, "late middlegame"), (16, 25, "middlegame")] {
        let part: Vec<Sample> = samples
            .iter()
            .copied()
            .filter(|s| s.phase >= lo && s.phase < hi)
            .collect();
        if part.len() >= 500 {
            println!("\n{name} (phase {lo}..{hi}):");
            print_reliability(&part, &phased);
        }
    }
}
