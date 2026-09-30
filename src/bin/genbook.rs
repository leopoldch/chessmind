//! Generates a balanced opening book (EPD, one position per line) for
//! `selfplay --openings`.
//!
//! Seeds are the start position plus the built-in lines of
//! `openings/builtin.txt`. Each seed is extended by random walks of
//! `--min-plies..=--max-plies` extra plies; every ply is drawn uniformly among
//! the moves whose shallow search score (`--walk-depth`) is within
//! `--walk-margin` cp of the best move. The end positions are deduplicated by
//! Zobrist hash, positions in check are dropped, and the rest are kept only
//! if a deeper search (`--depth`) scores them within +/- `--max-eval` cp for
//! the side to move.
//!
//! The search is a small self-contained alpha-beta (PVS + null move + LMR +
//! quiescence) over chessmind's move generator and evaluation: the engine's
//! public API does not expose scores, and no opening book is involved.
//! Every walk / evaluation uses its own RNG stream and a cleared hash table,
//! so the output only depends on `--seed` (not on the thread count).
//!
//! Usage:
//!   genbook [--out openings/balanced.epd] [--walks-per-seed 100]
//!           [--min-plies 2] [--max-plies 6] [--walk-depth 4] [--walk-margin 60]
//!           [--depth 7] [--max-eval 80] [--count N] [--seed 1] [--threads N]

use chessmind::board::{Board, color_idx};
use chessmind::eval::evaluate;
use chessmind::fen;
use chessmind::game::Game;
use chessmind::movegen::{generate_captures_fast, generate_moves_fast};
use chessmind::pieces::Color;
use chessmind::see::static_exchange_eval;
use chessmind::types::{Move, MoveList};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use std::collections::HashSet;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const BUILTIN_OPENINGS: &str = include_str!("../../openings/builtin.txt");

const INF: i32 = 32_000;
const MATE: i32 = 30_000;
const MAX_PLY: usize = 64;
const TT_BITS: u32 = 17;

struct Args {
    out: String,
    walks_per_seed: usize,
    min_plies: usize,
    max_plies: usize,
    walk_depth: i32,
    walk_margin: i32,
    depth: i32,
    max_eval: i32,
    count: Option<usize>,
    seed: u64,
    threads: usize,
}

fn usage() -> ! {
    eprintln!(
        "usage: genbook [--out FILE] [--walks-per-seed N] [--min-plies N] [--max-plies N]
               [--walk-depth D] [--walk-margin CP] [--depth D] [--max-eval CP]
               [--count N] [--seed N] [--threads N]"
    );
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut a = Args {
        out: "openings/balanced.epd".into(),
        walks_per_seed: 100,
        min_plies: 2,
        max_plies: 6,
        walk_depth: 4,
        walk_margin: 60,
        depth: 7,
        max_eval: 80,
        count: None,
        seed: 1,
        threads: num_cpus::get(),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let v = args.get(i + 1).cloned().unwrap_or_else(|| usage());
        fn num<T: std::str::FromStr>(s: &str) -> T {
            s.parse().unwrap_or_else(|_| usage())
        }
        match args[i].as_str() {
            "--out" => a.out = v,
            "--walks-per-seed" => a.walks_per_seed = num(&v),
            "--min-plies" => a.min_plies = num(&v),
            "--max-plies" => a.max_plies = num(&v),
            "--walk-depth" => a.walk_depth = num::<i32>(&v).max(1),
            "--walk-margin" => a.walk_margin = num(&v),
            "--depth" => a.depth = num::<i32>(&v).max(1),
            "--max-eval" => a.max_eval = num(&v),
            "--count" => a.count = Some(num(&v)),
            "--seed" => a.seed = num(&v),
            "--threads" => a.threads = num::<usize>(&v).max(1),
            _ => usage(),
        }
        i += 2;
    }
    if a.max_plies < a.min_plies {
        usage();
    }
    a
}

// ---------------------------------------------------------------- search ---

#[derive(Clone, Copy, Default)]
struct TtEntry {
    key: u64,
    mv: u16,
    depth: i8,
    bound: u8,
    score: i32,
}

const EXACT: u8 = 0;
const LOWER: u8 = 1;
const UPPER: u8 = 2;

struct Searcher {
    tt: Vec<TtEntry>,
    killers: [[Move; 2]; MAX_PLY],
    history: [[i32; 64]; 64],
    nodes: u64,
}

fn opposite(c: Color) -> Color {
    if c == Color::White {
        Color::Black
    } else {
        Color::White
    }
}

fn has_non_pawn_material(b: &Board, c: Color) -> bool {
    let bb = &b.bitboards[color_idx(c)];
    (bb[1] | bb[2] | bb[3] | bb[4]) != 0
}

impl Searcher {
    fn new() -> Self {
        Self {
            tt: vec![TtEntry::default(); 1 << TT_BITS],
            killers: [[Move::NONE; 2]; MAX_PLY],
            history: [[0; 64]; 64],
            nodes: 0,
        }
    }

    /// Forget everything so each task is independent (determinism).
    fn clear(&mut self) {
        self.tt.fill(TtEntry::default());
        self.killers = [[Move::NONE; 2]; MAX_PLY];
        self.history = [[0; 64]; 64];
    }

    fn slot(&self, key: u64) -> usize {
        (key >> (64 - TT_BITS)) as usize
    }

    fn order(&self, b: &Board, list: &MoveList, tt_move: Move, ply: usize) -> Vec<(Move, i32)> {
        list.iter()
            .map(|&m| {
                let score = if m == tt_move {
                    1_000_000
                } else if m.is_capture() || m.is_promotion() {
                    let see = if m.is_capture() {
                        static_exchange_eval(b, m)
                    } else {
                        0
                    };
                    if see >= 0 {
                        500_000 + see
                    } else {
                        -500_000 + see
                    }
                } else if ply < MAX_PLY && self.killers[ply].contains(&m) {
                    400_000
                } else {
                    self.history[m.from_sq() as usize & 63][m.to_sq() as usize & 63]
                };
                (m, score)
            })
            .collect()
    }

    fn qsearch(&mut self, b: &mut Board, c: Color, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.nodes += 1;
        if ply >= MAX_PLY - 1 {
            return evaluate(b, c);
        }
        let in_check = b.in_check_fast(c);
        let mut list = MoveList::new();
        let mut best;
        if in_check {
            generate_moves_fast(b, c, &mut list);
            if list.is_empty() {
                return -MATE + ply as i32;
            }
            best = -INF;
        } else {
            let stand = evaluate(b, c);
            if stand >= beta {
                return stand;
            }
            alpha = alpha.max(stand);
            best = stand;
            generate_captures_fast(b, c, &mut list);
        }
        let mut moves = self.order(b, &list, Move::NONE, MAX_PLY);
        moves.sort_unstable_by_key(|&(_, s)| -s);
        for (m, s) in moves {
            if !in_check && s < 0 {
                break; // losing captures (SEE < 0)
            }
            let undo = b.make_move_fast(m, c);
            let score = -self.qsearch(b, opposite(c), -beta, -alpha, ply + 1);
            b.unmake_move_fast(undo, c);
            if score > best {
                best = score;
                if score > alpha {
                    alpha = score;
                    if score >= beta {
                        break;
                    }
                }
            }
        }
        best
    }

    #[allow(clippy::too_many_arguments)]
    fn search(
        &mut self,
        b: &mut Board,
        c: Color,
        mut depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        can_null: bool,
    ) -> i32 {
        let in_check = b.in_check_fast(c);
        if in_check && ply < MAX_PLY / 2 {
            depth += 1;
        }
        if depth <= 0 || ply >= MAX_PLY - 1 {
            return self.qsearch(b, c, alpha, beta, ply);
        }
        self.nodes += 1;
        let key = b.hash(c);
        let slot = self.slot(key);
        let entry = self.tt[slot];
        let mut tt_move = Move::NONE;
        if entry.key == key {
            tt_move = Move(entry.mv);
            if entry.depth as i32 >= depth && entry.score.abs() < MATE - 200 {
                let s = entry.score;
                match entry.bound {
                    EXACT => return s,
                    LOWER if s >= beta => return s,
                    UPPER if s <= alpha => return s,
                    _ => {}
                }
            }
        }

        // Null move pruning.
        if can_null
            && !in_check
            && depth >= 3
            && beta.abs() < MATE - 200
            && has_non_pawn_material(b, c)
            && evaluate(b, c) >= beta
        {
            let ep = b.en_passant.take();
            let r = 2 + depth / 4;
            let score = -self.search(
                b,
                opposite(c),
                depth - 1 - r,
                -beta,
                -beta + 1,
                ply + 1,
                false,
            );
            b.en_passant = ep;
            if score >= beta {
                return beta;
            }
        }

        let mut list = MoveList::new();
        generate_moves_fast(b, c, &mut list);
        if list.is_empty() {
            return if in_check { -MATE + ply as i32 } else { 0 };
        }
        let mut moves = self.order(b, &list, tt_move, ply);
        moves.sort_unstable_by_key(|&(_, s)| -s);

        let alpha_orig = alpha;
        let mut best = -INF;
        let mut best_move = Move::NONE;
        for (i, &(m, _)) in moves.iter().enumerate() {
            let quiet = !m.is_capture() && !m.is_promotion();
            let undo = b.make_move_fast(m, c);
            let score = if i == 0 {
                -self.search(b, opposite(c), depth - 1, -beta, -alpha, ply + 1, true)
            } else {
                let r = if depth >= 3 && i >= 3 && quiet && !in_check {
                    1 + (i >= 8 && depth >= 5) as i32
                } else {
                    0
                };
                let mut s = -self.search(
                    b,
                    opposite(c),
                    depth - 1 - r,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    true,
                );
                if s > alpha && (r > 0 || s < beta) {
                    s = -self.search(b, opposite(c), depth - 1, -beta, -alpha, ply + 1, true);
                }
                s
            };
            b.unmake_move_fast(undo, c);
            if score > best {
                best = score;
                best_move = m;
                if score > alpha {
                    alpha = score;
                    if score >= beta {
                        if quiet && ply < MAX_PLY {
                            if self.killers[ply][0] != m {
                                self.killers[ply][1] = self.killers[ply][0];
                                self.killers[ply][0] = m;
                            }
                            let h = &mut self.history[m.from_sq() as usize & 63]
                                [m.to_sq() as usize & 63];
                            *h = (*h + depth * depth).min(300_000);
                        }
                        break;
                    }
                }
            }
        }
        let bound = if best >= beta {
            LOWER
        } else if best > alpha_orig {
            EXACT
        } else {
            UPPER
        };
        self.tt[slot] = TtEntry {
            key,
            mv: best_move.0,
            depth: depth.min(127) as i8,
            bound,
            score: best,
        };
        best
    }

    /// Score of the position for the side to move (iterative deepening).
    fn evaluate_position(&mut self, b: &mut Board, c: Color, depth: i32) -> i32 {
        let mut score = 0;
        for d in 1..=depth {
            score = self.search(b, c, d, -INF, INF, 0, false);
        }
        score
    }

    /// Root moves whose score is within `margin` of the best, at `depth`.
    fn good_moves(&mut self, b: &mut Board, c: Color, depth: i32, margin: i32) -> Vec<Move> {
        let mut list = MoveList::new();
        generate_moves_fast(b, c, &mut list);
        let mut scored: Vec<(Move, i32)> = list.iter().map(|&m| (m, 0)).collect();
        let mut result = Vec::new();
        for d in 1..=depth {
            let mut best = -INF;
            let mut next = Vec::with_capacity(scored.len());
            for &(m, _) in &scored {
                // Moves that fail low against (best - margin) cannot qualify.
                let floor = if best == -INF {
                    -INF
                } else {
                    best - margin - 1
                };
                let undo = b.make_move_fast(m, c);
                let s = -self.search(b, opposite(c), d - 1, -INF, -floor, 1, true);
                b.unmake_move_fast(undo, c);
                best = best.max(s);
                next.push((m, if s > floor { s } else { -INF }));
            }
            next.sort_by_key(|&(_, s)| -s);
            if d == depth {
                result = next
                    .iter()
                    .filter(|&&(_, s)| s != -INF && s >= best - margin)
                    .map(|&(m, _)| m)
                    .collect();
            }
            scored = next;
        }
        result
    }
}

// ------------------------------------------------------------------ book ---

fn seeds() -> Vec<Vec<String>> {
    let mut out = vec![Vec::new()]; // start position
    for line in BUILTIN_OPENINGS.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        out.push(line.split_whitespace().map(str::to_string).collect());
    }
    out
}

fn play(game: &mut Game, mv: &str) -> bool {
    mv.len() >= 4 && game.make_move(&mv[..2], &mv[2..])
}

/// One random walk: seed line + random near-best plies. Returns the final
/// game, or None if the walk ended in a position unsuitable as an opening.
fn walk(s: &mut Searcher, seed_line: &[String], rng: &mut StdRng, a: &Args) -> Option<Game> {
    s.clear();
    let mut game = Game::new();
    for mv in seed_line {
        if !play(&mut game, mv) {
            panic!("illegal built-in opening move {mv}");
        }
    }
    let plies = rng.gen_range(a.min_plies..=a.max_plies);
    for _ in 0..plies {
        let c = game.current_turn;
        let moves = s.good_moves(&mut game.board, c, a.walk_depth, a.walk_margin);
        let m = *moves.choose(rng)?;
        if !play(&mut game, &m.to_algebraic()) {
            return None;
        }
    }
    let c = game.current_turn;
    if game.board.in_check_fast(c) || game.legal_moves().is_empty() {
        return None;
    }
    Some(game)
}

fn run_parallel<T: Send, F>(n: usize, threads: usize, label: &str, f: F) -> Vec<T>
where
    F: Fn(&mut Searcher, usize) -> T + Sync,
{
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<T>>> = Mutex::new((0..n).map(|_| None).collect());
    let started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..threads.min(n.max(1)) {
            scope.spawn(|| {
                let mut s = Searcher::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let r = f(&mut s, i);
                    results.lock().unwrap()[i] = Some(r);
                    let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if d.is_multiple_of(250) || d == n {
                        eprint!(
                            "\r{label}: {d}/{n} ({:.0} s)   ",
                            started.elapsed().as_secs_f64()
                        );
                        let _ = std::io::stderr().flush();
                    }
                }
            });
        }
    });
    eprintln!();
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|r| r.unwrap())
        .collect()
}

fn main() {
    let a = parse_args();
    let seeds = seeds();
    let n_walks = seeds.len() * a.walks_per_seed;
    eprintln!(
        "genbook: {} seeds x {} walks, +{}..{} plies (depth {}, margin {} cp), filter depth {} |eval| <= {} cp, seed {}, {} threads",
        seeds.len(),
        a.walks_per_seed,
        a.min_plies,
        a.max_plies,
        a.walk_depth,
        a.walk_margin,
        a.depth,
        a.max_eval,
        a.seed,
        a.threads
    );

    // 1. Random walks (each with its own deterministic RNG stream).
    let walks = run_parallel(n_walks, a.threads, "walks", |s, i| {
        let mut rng = StdRng::seed_from_u64(a.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ i as u64);
        walk(s, &seeds[i / a.walks_per_seed], &mut rng, &a)
    });

    // 2. Deduplicate by position hash (first occurrence wins).
    let mut seen = HashSet::new();
    let mut unique: Vec<Game> = Vec::new();
    let mut rejected_walks = 0;
    for g in walks {
        match g {
            Some(g) => {
                if seen.insert(g.board.hash(g.current_turn)) {
                    unique.push(g);
                }
            }
            None => rejected_walks += 1,
        }
    }
    eprintln!(
        "walks: {n_walks}, unusable {rejected_walks}, unique positions {}",
        unique.len()
    );

    // 3. Balance filter with a deeper search.
    let scores = run_parallel(unique.len(), a.threads, "evals", |s, i| {
        s.clear();
        let mut board = unique[i].board.clone();
        s.evaluate_position(&mut board, unique[i].current_turn, a.depth)
    });
    let mut kept: Vec<(String, i32)> = unique
        .iter()
        .zip(&scores)
        .filter(|(_, sc)| sc.abs() <= a.max_eval)
        .map(|(g, &sc)| (fen::to_epd(g), sc))
        .collect();

    // 4. Shuffle (so a truncated file still covers every seed) and write.
    kept.shuffle(&mut StdRng::seed_from_u64(a.seed));
    if let Some(n) = a.count {
        kept.truncate(n);
    }
    let mut hist = [0usize; 9];
    for (_, sc) in &kept {
        let bucket = ((sc + a.max_eval) * 8 / (2 * a.max_eval).max(1)).clamp(0, 8) as usize;
        hist[bucket] += 1;
    }
    let text: String = kept.iter().map(|(l, _)| format!("{l}\n")).collect();
    if let Err(e) = std::fs::write(&a.out, text) {
        eprintln!("cannot write '{}': {e}", a.out);
        std::process::exit(1);
    }
    let mean = kept.iter().map(|(_, s)| *s as f64).sum::<f64>() / kept.len().max(1) as f64;
    eprintln!(
        "wrote {} positions to {} (dropped {} outside +/-{} cp); mean eval {:+.1} cp (side to move)",
        kept.len(),
        a.out,
        unique.len() - scores.iter().filter(|s| s.abs() <= a.max_eval).count(),
        a.max_eval,
        mean
    );
    eprintln!(
        "eval histogram (-{0}..+{0} cp in 9 buckets): {hist:?}",
        a.max_eval
    );
}
