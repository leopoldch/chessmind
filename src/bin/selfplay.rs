//! Headless match runner between two UCI engines (e.g. an old and a new build).
//!
//! Every opening is played as a *pair* of games with colours swapped, both
//! games on the same worker, so the pentanomial statistics (pair outcomes
//! LL, LD, DD/WL, WD, WW) are exact. Openings come from an EPD/FEN file
//! (`--openings`, e.g. `openings/balanced.epd`) or from the built-in lines in
//! `openings/builtin.txt`. The runner keeps the authoritative game state
//! (legality, mate, stalemate, threefold repetition, 50-move rule,
//! insufficient material, ply cap) and its own clocks.
//!
//! Statistics: pentanomial score, logistic Elo +/- 95%, normalized Elo and the
//! exact fishtest pentanomial GSPRT log-likelihood ratio (see `gsprt_llr`),
//! plus the old trinomial W/D/L summary for reference.
//!
//! Usage:
//!   selfplay --engine1 PATH --engine2 PATH [--games N | --pairs N]
//!            [--tc BASE_MS+INC_MS | --movetime MS] [--tc1 SPEC] [--tc2 SPEC]
//!            [--openings FILE] [--openings-order random|sequential] [--seed N]
//!            [--concurrency K] [--threads T] [--hash MB] [--no-book] [--pgn FILE]
//!            [--sprt] [--elo0 0] [--elo1 5] [--alpha 0.05] [--beta 0.05] [--sprt-min-pairs 20]
//!            [--sprt-model normalized|logistic]
//!            [--quiet] [--status-interval SECS]
//!            [--max-plies 400] [--timemargin MS] [--name1 NAME] [--name2 NAME]
//!
//! A time-control SPEC is `BASE_MS+INC_MS`, `movetime=MS` or `depth=N`.

use chessmind::fen;
use chessmind::game::Game;
use chessmind::pieces::{Color, PieceType};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Balanced opening lines (UCI moves from the start position, 4-8 plies).
const BUILTIN_OPENINGS: &str = include_str!("../../openings/builtin.txt");

// ---------------------------------------------------------------- config ---

#[derive(Clone, Copy, Debug, PartialEq)]
enum TimeControl {
    Clock { base_ms: u64, inc_ms: u64 },
    MoveTime(u64),
    Depth(u32),
}

impl TimeControl {
    fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim();
        if let Some(v) = spec
            .strip_prefix("movetime=")
            .or_else(|| spec.strip_prefix("mt="))
        {
            return v.parse().ok().map(TimeControl::MoveTime);
        }
        if let Some(v) = spec.strip_prefix("depth=") {
            return v.parse().ok().map(TimeControl::Depth);
        }
        let (base, inc) = spec.split_once('+').unwrap_or((spec, "0"));
        Some(TimeControl::Clock {
            base_ms: base.parse().ok()?,
            inc_ms: inc.parse().ok()?,
        })
    }

    fn describe(&self) -> String {
        match *self {
            TimeControl::Clock { base_ms, inc_ms } => format!("{base_ms}+{inc_ms} ms"),
            TimeControl::MoveTime(ms) => format!("movetime {ms} ms"),
            TimeControl::Depth(d) => format!("depth {d}"),
        }
    }

    fn pgn(&self) -> String {
        match *self {
            TimeControl::Clock { base_ms, inc_ms } => {
                format!("{}+{}", base_ms as f64 / 1000.0, inc_ms as f64 / 1000.0)
            }
            TimeControl::MoveTime(ms) => format!("{}/move", ms as f64 / 1000.0),
            TimeControl::Depth(d) => format!("depth {d}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum OpeningOrder {
    Random,
    Sequential,
}

#[derive(Clone)]
struct Config {
    engine1: String,
    engine2: String,
    name1: String,
    name2: String,
    pairs: usize,
    tc1: TimeControl,
    tc2: TimeControl,
    concurrency: usize,
    threads: usize,
    hash_mb: usize,
    no_book: bool,
    pgn: Option<String>,
    sprt: bool,
    elo0: f64,
    elo1: f64,
    alpha: f64,
    beta: f64,
    sprt_min_pairs: u32,
    sprt_model: SprtModel,
    max_plies: usize,
    time_margin_ms: u64,
    openings: Option<String>,
    order: OpeningOrder,
    seed: u64,
    quiet: bool,
    status_interval: f64,
}

fn usage() -> ! {
    eprintln!(
        "usage: selfplay --engine1 PATH --engine2 PATH [--games N | --pairs N]
         [--tc BASE_MS+INC_MS | --movetime MS] [--tc1 SPEC] [--tc2 SPEC]
         [--openings FILE] [--openings-order random|sequential] [--seed N]
         [--concurrency K] [--threads T] [--hash MB] [--no-book] [--pgn FILE]
         [--sprt] [--elo0 E] [--elo1 E] [--alpha A] [--beta B] [--sprt-min-pairs N]
         [--sprt-model normalized|logistic]
         [--quiet] [--status-interval SECS]
         [--max-plies N] [--timemargin MS] [--name1 NAME] [--name2 NAME]
  SPEC = BASE_MS+INC_MS | movetime=MS | depth=N  (--tc1/--tc2 give time odds)"
    );
    std::process::exit(2);
}

fn default_concurrency() -> usize {
    (num_cpus::get_physical() / 2).max(1)
}

fn parse_args() -> Config {
    let mut cfg = Config {
        engine1: String::new(),
        engine2: String::new(),
        name1: "engine1".into(),
        name2: "engine2".into(),
        pairs: 50,
        tc1: TimeControl::Clock {
            base_ms: 10_000,
            inc_ms: 100,
        },
        tc2: TimeControl::Clock {
            base_ms: 10_000,
            inc_ms: 100,
        },
        concurrency: default_concurrency(),
        threads: 1,
        hash_mb: 16,
        no_book: false,
        pgn: None,
        sprt: false,
        elo0: 0.0,
        elo1: 5.0,
        alpha: 0.05,
        beta: 0.05,
        sprt_min_pairs: 20,
        sprt_model: SprtModel::Normalized,
        max_plies: 400,
        time_margin_ms: 100,
        openings: None,
        order: OpeningOrder::Random,
        seed: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1),
        quiet: false,
        status_interval: 10.0,
    };
    let mut tc1: Option<TimeControl> = None;
    let mut tc2: Option<TimeControl> = None;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let value = |i: usize| -> String { args.get(i + 1).cloned().unwrap_or_else(|| usage()) };
    fn num<T: std::str::FromStr>(s: &str) -> T {
        s.parse().unwrap_or_else(|_| {
            eprintln!("invalid number '{s}'");
            usage()
        })
    }
    fn tc(s: &str) -> TimeControl {
        TimeControl::parse(s).unwrap_or_else(|| {
            eprintln!("invalid time control '{s}'");
            usage()
        })
    }
    while i < args.len() {
        let flag = args[i].as_str();
        let mut takes_value = true;
        match flag {
            "--engine1" => cfg.engine1 = value(i),
            "--engine2" => cfg.engine2 = value(i),
            "--name1" => cfg.name1 = value(i),
            "--name2" => cfg.name2 = value(i),
            "--games" => cfg.pairs = num::<usize>(&value(i)).div_ceil(2).max(1),
            "--pairs" => cfg.pairs = num::<usize>(&value(i)).max(1),
            "--tc" => {
                cfg.tc1 = tc(&value(i));
                cfg.tc2 = cfg.tc1;
            }
            "--movetime" => {
                cfg.tc1 = TimeControl::MoveTime(num(&value(i)));
                cfg.tc2 = cfg.tc1;
            }
            "--tc1" => tc1 = Some(tc(&value(i))),
            "--tc2" => tc2 = Some(tc(&value(i))),
            "--concurrency" => cfg.concurrency = num::<usize>(&value(i)).max(1),
            "--threads" => cfg.threads = num::<usize>(&value(i)).max(1),
            "--hash" => cfg.hash_mb = num::<usize>(&value(i)).max(1),
            "--pgn" => cfg.pgn = Some(value(i)),
            "--elo0" => cfg.elo0 = num(&value(i)),
            "--elo1" => cfg.elo1 = num(&value(i)),
            "--alpha" => cfg.alpha = num(&value(i)),
            "--beta" => cfg.beta = num(&value(i)),
            "--sprt-min-pairs" => cfg.sprt_min_pairs = num(&value(i)),
            "--sprt-model" => {
                cfg.sprt_model = match value(i).as_str() {
                    "logistic" => SprtModel::Logistic,
                    "normalized" => SprtModel::Normalized,
                    other => {
                        eprintln!("invalid --sprt-model '{other}'");
                        usage()
                    }
                }
            }
            "--max-plies" => cfg.max_plies = num(&value(i)),
            "--timemargin" => cfg.time_margin_ms = num(&value(i)),
            "--openings" => cfg.openings = Some(value(i)),
            "--openings-order" => {
                cfg.order = match value(i).as_str() {
                    "random" => OpeningOrder::Random,
                    "sequential" => OpeningOrder::Sequential,
                    other => {
                        eprintln!("invalid --openings-order '{other}'");
                        usage()
                    }
                }
            }
            "--seed" => cfg.seed = num(&value(i)),
            "--status-interval" => cfg.status_interval = num(&value(i)),
            "--no-book" => {
                cfg.no_book = true;
                takes_value = false;
            }
            "--sprt" => {
                cfg.sprt = true;
                takes_value = false;
            }
            "--quiet" | "-q" => {
                cfg.quiet = true;
                takes_value = false;
            }
            "-h" | "--help" => usage(),
            _ => {
                eprintln!("unknown argument '{flag}'");
                usage()
            }
        }
        i += if takes_value { 2 } else { 1 };
    }
    // Per-engine overrides win regardless of flag order.
    if let Some(t) = tc1 {
        cfg.tc1 = t;
    }
    if let Some(t) = tc2 {
        cfg.tc2 = t;
    }
    if cfg.engine1.is_empty() || cfg.engine2.is_empty() {
        usage();
    }
    if cfg.sprt && cfg.elo1 <= cfg.elo0 {
        eprintln!("--elo1 must be greater than --elo0");
        usage();
    }
    cfg
}

// -------------------------------------------------------------- openings ---

#[derive(Clone)]
struct Opening {
    /// Human-readable description (PGN `Opening` tag, logs).
    label: String,
    /// Normalized start FEN; `None` means the standard start position.
    fen: Option<String>,
    /// UCI moves played from the start position before the engines take over.
    moves: Vec<String>,
}

impl Opening {
    /// Start position with its halfmove clock and fullmove number.
    fn start(&self) -> (Game, u32, u32) {
        match &self.fen {
            Some(f) => {
                let pos = fen::parse_fen(f).expect("opening FEN validated at load time");
                (pos.game, pos.halfmove, pos.fullmove)
            }
            None => (Game::new(), 0, 1),
        }
    }
}

fn builtin_openings() -> Vec<Opening> {
    BUILTIN_OPENINGS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .enumerate()
        .map(|(i, l)| Opening {
            label: format!("#{} {l}", i + 1),
            fen: None,
            moves: l.split_whitespace().map(str::to_string).collect(),
        })
        .collect()
}

/// Read one FEN or EPD per line (blank lines and `#` comments are skipped).
fn load_openings(path: &str) -> Result<Vec<Opening>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read '{path}': {e}"))?;
    let mut out = Vec::new();
    let mut bad = 0;
    for (lineno, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match fen::parse_fen(line) {
            Ok(mut pos) => {
                if pos.game.legal_moves().is_empty() {
                    bad += 1;
                    eprintln!("{path}:{}: no legal moves, skipped", lineno + 1);
                    continue;
                }
                let normalized = fen::to_fen(&pos.game, pos.halfmove, pos.fullmove);
                out.push(Opening {
                    label: fen::to_epd(&pos.game),
                    fen: Some(normalized),
                    moves: Vec::new(),
                });
            }
            Err(e) => {
                bad += 1;
                eprintln!("{path}:{}: {e}, skipped", lineno + 1);
            }
        }
    }
    if bad > 0 {
        eprintln!("{path}: {bad} invalid line(s) skipped");
    }
    if out.is_empty() {
        return Err(format!("'{path}' contains no usable positions"));
    }
    Ok(out)
}

/// Check every opening can be set up (illegal moves in a line are fatal).
fn validate_openings(openings: &[Opening]) -> Result<(), String> {
    for op in openings {
        let (mut game, _, _) = op.start();
        for mv in &op.moves {
            if !is_legal(&mut game, mv) || !game.make_move(&mv[..2], &mv[2..]) {
                return Err(format!("opening {} has illegal move {mv}", op.label));
            }
        }
        if game.legal_moves().is_empty() {
            return Err(format!("opening {} is already over", op.label));
        }
    }
    Ok(())
}

/// Opening index for each pair: a (re)shuffled or sequential walk of the book.
fn schedule(n_openings: usize, pairs: usize, order: OpeningOrder, seed: u64) -> Vec<usize> {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut out = Vec::with_capacity(pairs);
    while out.len() < pairs {
        let mut cycle: Vec<usize> = (0..n_openings).collect();
        if order == OpeningOrder::Random {
            cycle.shuffle(&mut rng);
        }
        out.extend(cycle.into_iter().take(pairs - out.len()));
    }
    out
}

// ---------------------------------------------------------------- engine ---

struct EngineProc {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
}

#[derive(Debug)]
enum ReadError {
    Timeout,
    Crashed,
}

impl EngineProc {
    fn start(path: &str, cfg: &Config) -> Result<Self, String> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start '{path}': {e}"))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut proc = Self { child, stdin, rx };
        proc.send("uci");
        proc.wait_for("uciok", Duration::from_secs(10))
            .map_err(|e| format!("'{path}' did not answer uci: {e:?}"))?;
        proc.send(&format!("setoption name Threads value {}", cfg.threads));
        proc.send(&format!("setoption name Hash value {}", cfg.hash_mb));
        if cfg.no_book {
            proc.send("setoption name OwnBook value false");
        }
        proc.sync()
            .map_err(|e| format!("'{path}' not ready: {e:?}"))?;
        Ok(proc)
    }

    fn send(&mut self, line: &str) {
        let _ = writeln!(self.stdin, "{line}");
        let _ = self.stdin.flush();
    }

    fn wait_for(&mut self, prefix: &str, timeout: Duration) -> Result<String, ReadError> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(line) if line.starts_with(prefix) => return Ok(line),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return Err(ReadError::Timeout),
                Err(RecvTimeoutError::Disconnected) => return Err(ReadError::Crashed),
            }
        }
    }

    fn sync(&mut self) -> Result<(), ReadError> {
        self.send("isready");
        self.wait_for("readyok", Duration::from_secs(30))
            .map(|_| ())
    }
}

impl Drop for EngineProc {
    fn drop(&mut self) {
        self.send("quit");
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ------------------------------------------------------------------ game ---

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    WhiteWins,
    BlackWins,
    Draw,
}

impl Outcome {
    fn pgn(self) -> &'static str {
        match self {
            Outcome::WhiteWins => "1-0",
            Outcome::BlackWins => "0-1",
            Outcome::Draw => "1/2-1/2",
        }
    }
    fn win_for(color: Color) -> Self {
        if color == Color::White {
            Outcome::WhiteWins
        } else {
            Outcome::BlackWins
        }
    }
}

struct GameRecord {
    index: usize,
    pair: usize,
    opening: usize,
    engine1_white: bool,
    outcome: Outcome,
    reason: String,
    /// SAN of every move from the start FEN (opening moves included).
    san: Vec<String>,
    start_fen: Option<String>,
    start_turn: Color,
    start_fullmove: u32,
    /// Engines that must be restarted (crashed or unresponsive): (white, black).
    restart: (bool, bool),
}

impl GameRecord {
    /// Points scored by engine1 in this game (0, 0.5 or 1).
    fn engine1_points(&self) -> f64 {
        match (self.outcome, self.engine1_white) {
            (Outcome::Draw, _) => 0.5,
            (Outcome::WhiteWins, true) | (Outcome::BlackWins, false) => 1.0,
            _ => 0.0,
        }
    }
}

fn opponent(c: Color) -> Color {
    if c == Color::White {
        Color::Black
    } else {
        Color::White
    }
}

fn color_name(c: Color) -> &'static str {
    if c == Color::White { "White" } else { "Black" }
}

fn is_legal(game: &mut Game, mv: &str) -> bool {
    mv.len() >= 4
        && mv.is_ascii()
        && game
            .legal_moves()
            .into_iter()
            .any(|(f, t)| mv[..2] == f && mv[2..] == t)
}

/// SAN for a legal move, without the check suffix.
fn san_base(game: &mut Game, from: &str, to: &str) -> String {
    let board = &game.board;
    let Some(piece) = board.get(from) else {
        return format!("{from}{to}");
    };
    let dest = &to[..2];
    let promo = to.get(2..3).map(|p| format!("={}", p.to_ascii_uppercase()));
    let fx = from.as_bytes()[0];
    let tx = dest.as_bytes()[0];

    if piece.piece_type == PieceType::King && fx.abs_diff(tx) == 2 {
        return if tx > fx {
            "O-O".into()
        } else {
            "O-O-O".into()
        };
    }
    let capture = board.get(dest).is_some() || (piece.piece_type == PieceType::Pawn && fx != tx);

    let mut s = String::new();
    if piece.piece_type == PieceType::Pawn {
        if capture {
            s.push(fx as char);
        }
    } else {
        s.push(match piece.piece_type {
            PieceType::Knight => 'N',
            PieceType::Bishop => 'B',
            PieceType::Rook => 'R',
            PieceType::Queen => 'Q',
            _ => 'K',
        });
        let piece_type = piece.piece_type;
        let rivals: Vec<String> = game
            .legal_moves()
            .into_iter()
            .filter(|(f, t)| {
                f != from
                    && &t[..2] == dest
                    && game.board.get(f).map(|p| p.piece_type) == Some(piece_type)
            })
            .map(|(f, _)| f)
            .collect();
        if !rivals.is_empty() {
            let same_file = rivals.iter().any(|f| f.as_bytes()[0] == fx);
            let same_rank = rivals.iter().any(|f| f.as_bytes()[1] == from.as_bytes()[1]);
            if !same_file {
                s.push(fx as char);
            } else if !same_rank {
                s.push(from.as_bytes()[1] as char);
            } else {
                s.push_str(from);
            }
        }
    }
    if capture {
        s.push('x');
    }
    s.push_str(dest);
    if let Some(p) = promo {
        s.push_str(&p);
    }
    s
}

fn insufficient_material(game: &Game) -> bool {
    let b = &game.board;
    for c in [Color::White, Color::Black] {
        for pt in [PieceType::Pawn, PieceType::Rook, PieceType::Queen] {
            if b.piece_count_color(pt, c) > 0 {
                return false;
            }
        }
    }
    let mut knights = 0;
    let mut bishop_square_colors = Vec::new();
    for y in 0..8 {
        for x in 0..8 {
            match b.get_index(x, y).map(|p| p.piece_type) {
                Some(PieceType::Knight) => knights += 1,
                Some(PieceType::Bishop) => bishop_square_colors.push((x + y) % 2),
                _ => {}
            }
        }
    }
    let minors = knights + bishop_square_colors.len();
    if minors <= 1 {
        return true;
    }
    // Only bishops, all on the same square colour: no mate possible.
    knights == 0 && bishop_square_colors.windows(2).all(|w| w[0] == w[1])
}

/// Apply a validated move, updating SAN and the halfmove clock.
fn apply_move(game: &mut Game, from: &str, to: &str, san: &mut Vec<String>, halfmove: &mut u32) {
    let is_pawn = game.board.get(from).map(|p| p.piece_type) == Some(PieceType::Pawn);
    let capture = game.board.get(&to[..2]).is_some();
    let mut s = san_base(game, from, to);
    assert!(
        game.make_move(from, to),
        "move {from}{to} rejected after validation"
    );
    *halfmove = if is_pawn || capture { 0 } else { *halfmove + 1 };
    let side = game.current_turn;
    if game.board.in_check(side) {
        s.push(if game.legal_moves().is_empty() {
            '#'
        } else {
            '+'
        });
    }
    san.push(s);
}

/// Returns Some((outcome, reason)) if the game is over.
fn adjudicate(
    game: &mut Game,
    halfmove: u32,
    plies: usize,
    max_plies: usize,
) -> Option<(Outcome, String)> {
    let side = game.current_turn;
    if game.legal_moves().is_empty() {
        return Some(if game.board.in_check(side) {
            (
                Outcome::win_for(opponent(side)),
                format!("{} mates", color_name(opponent(side))),
            )
        } else {
            (Outcome::Draw, "stalemate".into())
        });
    }
    let hash = game.board.hash(side);
    if game.repetition_count(hash) >= 3 {
        return Some((Outcome::Draw, "threefold repetition".into()));
    }
    if halfmove >= 100 {
        return Some((Outcome::Draw, "fifty-move rule".into()));
    }
    if insufficient_material(game) {
        return Some((Outcome::Draw, "insufficient material".into()));
    }
    if plies >= max_plies {
        return Some((Outcome::Draw, format!("max plies ({max_plies})")));
    }
    None
}

/// Play one game. `tcs` are the time controls of (white, black).
#[allow(clippy::too_many_arguments)]
fn play_game(
    index: usize,
    pair: usize,
    opening_idx: usize,
    opening: &Opening,
    engine1_white: bool,
    white: &mut EngineProc,
    black: &mut EngineProc,
    tcs: [TimeControl; 2],
    cfg: &Config,
) -> GameRecord {
    let (mut game, mut halfmove, fullmove) = opening.start();
    let mut record = GameRecord {
        index,
        pair,
        opening: opening_idx,
        engine1_white,
        outcome: Outcome::Draw,
        reason: String::new(),
        san: Vec::new(),
        start_fen: opening.fen.clone(),
        start_turn: game.current_turn,
        start_fullmove: fullmove,
        restart: (false, false),
    };

    // New game handshake.
    for (i, e) in [&mut *white, &mut *black].into_iter().enumerate() {
        e.send("ucinewgame");
        if e.sync().is_err() {
            let side = if i == 0 { Color::White } else { Color::Black };
            record.outcome = Outcome::win_for(opponent(side));
            record.reason = format!("{} engine unresponsive at start", color_name(side));
            if i == 0 {
                record.restart.0 = true;
            } else {
                record.restart.1 = true;
            }
            return record;
        }
    }

    let mut moves: Vec<String> = Vec::new();
    for mv in &opening.moves {
        let (from, to) = mv.split_at(2);
        apply_move(&mut game, from, to, &mut record.san, &mut halfmove);
        moves.push(mv.clone());
    }
    let position_head = match &opening.fen {
        Some(f) => format!("position fen {f}"),
        None => "position startpos".to_string(),
    };

    let clock = |tc: TimeControl| match tc {
        TimeControl::Clock { base_ms, inc_ms } => (base_ms as i64, inc_ms as i64),
        _ => (0, 0),
    };
    let (mut wtime, winc) = clock(tcs[0]);
    let (mut btime, binc) = clock(tcs[1]);
    let margin = cfg.time_margin_ms as i64;

    loop {
        if let Some((outcome, reason)) = adjudicate(&mut game, halfmove, moves.len(), cfg.max_plies)
        {
            record.outcome = outcome;
            record.reason = reason;
            return record;
        }

        let side = game.current_turn;
        let is_white = side == Color::White;
        let tc = tcs[if is_white { 0 } else { 1 }];
        let engine: &mut EngineProc = if is_white { &mut *white } else { &mut *black };
        let (go, budget_ms) = match tc {
            TimeControl::Clock { .. } => {
                // If the opponent is not on a clock, mirror our own clock for it.
                let (own, own_inc) = if is_white {
                    (wtime, winc)
                } else {
                    (btime, binc)
                };
                let other_on_clock =
                    matches!(tcs[if is_white { 1 } else { 0 }], TimeControl::Clock { .. });
                let (wt, wi, bt, bi) = if other_on_clock {
                    (wtime, winc, btime, binc)
                } else {
                    (own, own_inc, own, own_inc)
                };
                (
                    format!(
                        "go wtime {} btime {} winc {wi} binc {bi}",
                        wt.max(1),
                        bt.max(1)
                    ),
                    own,
                )
            }
            TimeControl::MoveTime(ms) => (format!("go movetime {ms}"), ms as i64),
            TimeControl::Depth(d) => (format!("go depth {d}"), 120_000),
        };

        if moves.is_empty() {
            engine.send(&position_head);
        } else {
            engine.send(&format!("{position_head} moves {}", moves.join(" ")));
        }
        let started = Instant::now();
        engine.send(&go);
        let wait = Duration::from_millis((budget_ms.max(0) + margin + 1000) as u64);
        let reply = engine.wait_for("bestmove", wait);
        let elapsed = started.elapsed().as_millis() as i64;

        let lose = |record: &mut GameRecord, reason: String| {
            record.outcome = Outcome::win_for(opponent(side));
            record.reason = reason;
        };
        let line = match reply {
            Ok(line) => line,
            Err(err) => {
                // Unresponsive or crashed: loss, and restart that engine.
                if matches!(err, ReadError::Timeout) {
                    engine.send("stop");
                    if engine.wait_for("bestmove", Duration::from_secs(2)).is_ok() {
                        lose(&mut record, format!("{} loses on time", color_name(side)));
                        return record;
                    }
                }
                if is_white {
                    record.restart.0 = true;
                } else {
                    record.restart.1 = true;
                }
                lose(
                    &mut record,
                    format!("{} engine unresponsive ({:?})", color_name(side), err),
                );
                return record;
            }
        };

        // Clock bookkeeping.
        let over = match tc {
            TimeControl::Clock { .. } => {
                let (clock, inc) = if is_white {
                    (&mut wtime, winc)
                } else {
                    (&mut btime, binc)
                };
                // Charge the full elapsed time and never refill an overdraft:
                // `margin` is one fixed allowance for process/IPC latency over
                // the whole game, not a per-move tolerance.
                *clock -= elapsed;
                let flagged = *clock < -margin;
                *clock += inc;
                flagged
            }
            TimeControl::MoveTime(ms) => elapsed > ms as i64 + margin,
            TimeControl::Depth(_) => false,
        };
        if over {
            lose(
                &mut record,
                format!("{} loses on time ({elapsed} ms)", color_name(side)),
            );
            return record;
        }

        let mv = line
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .to_ascii_lowercase();
        if !is_legal(&mut game, &mv) {
            lose(
                &mut record,
                format!("{} illegal move '{mv}'", color_name(side)),
            );
            return record;
        }
        let (from, to) = mv.split_at(2);
        apply_move(&mut game, from, to, &mut record.san, &mut halfmove);
        moves.push(mv);
    }
}

// ----------------------------------------------------------------- stats ---

const Z95: f64 = 1.959964;

fn elo_from_score(s: f64) -> f64 {
    let s = s.clamp(1e-6, 1.0 - 1e-6);
    // `+ 0.0` turns -0.0 into 0.0 for display.
    -400.0 * (1.0 / s - 1.0).log10() + 0.0
}

fn score_from_elo(elo: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf(-elo / 400.0))
}

fn erf(x: f64) -> f64 {
    // Abramowitz & Stegun 7.1.26
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t
            * (-x * x).exp();
    if x >= 0.0 { y } else { -y }
}

fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

/// Wald bounds (lower, upper) for the log-likelihood ratio.
fn sprt_bounds(alpha: f64, beta: f64) -> (f64, f64) {
    ((beta / (1.0 - alpha)).ln(), ((1.0 - beta) / alpha).ln())
}

/// Mean and variance of a discrete distribution given as (count, value) pairs.
fn mean_var(dist: &[(f64, f64)]) -> (f64, f64, f64) {
    let n: f64 = dist.iter().map(|(c, _)| c).sum();
    if n <= 0.0 {
        return (0.0, 0.5, 0.0);
    }
    let mean = dist.iter().map(|(c, v)| c * v).sum::<f64>() / n;
    let var = dist
        .iter()
        .map(|(c, v)| c * (v - mean).powi(2))
        .sum::<f64>()
        / n;
    (n, mean, var)
}

/// Elo model of the SPRT bounds `--elo0/--elo1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SprtModel {
    /// Logistic Elo: the hypotheses fix the expected score.
    Logistic,
    /// Normalized Elo (modern fishtest default): the hypotheses fix the
    /// t-value (score - 0.5) / sigma, so bounds are independent of the draw
    /// rate and of the opening book.
    Normalized,
}

impl SprtModel {
    fn name(self) -> &'static str {
        match self {
            SprtModel::Logistic => "logistic",
            SprtModel::Normalized => "normalized",
        }
    }
}

/// fishtest regularization: empty cells get a tiny count so the MLE exists.
const REG_EPS: f64 = 1e-3;

/// Solves the secular equation `sum_i p_i a_i / (1 + x a_i) = 0` for x in
/// `(-1/max a, -1/min a)` (fishtest `LLRcalc.secular`). The left-hand side is
/// strictly decreasing there, from +inf to -inf, so bisection is exact to
/// machine precision. `None` if the support does not straddle zero.
fn secular(pdf: &[(f64, f64)]) -> Option<f64> {
    let v = pdf.iter().map(|&(a, _)| a).fold(f64::INFINITY, f64::min);
    let w = pdf
        .iter()
        .map(|&(a, _)| a)
        .fold(f64::NEG_INFINITY, f64::max);
    if v * w >= 0.0 || !v.is_finite() || !w.is_finite() {
        return None;
    }
    let f = |x: f64| pdf.iter().map(|&(a, p)| p * a / (1.0 + x * a)).sum::<f64>();
    let (mut lo, mut hi) = (-1.0 / w + 1e-9, -1.0 / v - 1e-9);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if mid <= lo || mid >= hi {
            break;
        }
        if f(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

/// Maximum-likelihood distribution on the support of `pdf` (value, prob)
/// whose expectation is `s` (fishtest `MLE_expected`). Returns probabilities.
fn mle_expected(pdf: &[(f64, f64)], s: f64) -> Option<Vec<f64>> {
    let shifted: Vec<(f64, f64)> = pdf.iter().map(|&(a, p)| (a - s, p)).collect();
    let x = secular(&shifted)?;
    Some(shifted.iter().map(|&(b, p)| p / (1.0 + x * b)).collect())
}

/// Maximum-likelihood distribution on the support of `pdf` whose t-value
/// `(mu - reference) / sigma` equals `t` (fishtest `MLE_t_value`, fixed-point
/// iteration on the linearized constraint).
fn mle_t_value(pdf: &[(f64, f64)], reference: f64, t: f64) -> Option<Vec<f64>> {
    let n = pdf.len();
    let mut probs = vec![1.0 / n as f64; n];
    for _ in 0..100 {
        // `mean_var` takes (weight, value) pairs.
        let current: Vec<(f64, f64)> = pdf.iter().zip(&probs).map(|(&(a, _), &p)| (p, a)).collect();
        let (_, mu, var) = mean_var(&current);
        let sigma = var.sqrt();
        let constraint: Vec<(f64, f64)> = pdf
            .iter()
            .map(|&(a, p)| {
                let z = (mu - a) / sigma;
                (a - reference - t * sigma * (1.0 + z * z) / 2.0, p)
            })
            .collect();
        let x = secular(&constraint)?;
        let next: Vec<f64> = constraint.iter().map(|&(b, p)| p / (1.0 + x * b)).collect();
        let delta = next
            .iter()
            .zip(&probs)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        probs = next;
        if delta < 1e-12 {
            break;
        }
    }
    Some(probs)
}

/// Exact generalized SPRT log-likelihood ratio, as fishtest computes it
/// (`LLRcalc.LLR_logistic` / `LLR_normalized`):
///
///   LLR = sum_k n_k * ln(p1_k / p0_k)
///
/// where `counts` is a trinomial [L, D, W] or pentanomial [LL, LD, DD+WL,
/// WD, WW] tally (cell k has score k / (len - 1)), empty cells are
/// regularized to 1e-3, and p0 / p1 are the maximum-likelihood distributions
/// constrained to hypothesis H0 / H1:
/// * logistic: expected score `s_i = 1 / (1 + 10^(-elo_i / 400))`;
/// * normalized: t-value `(mu - 0.5) / sigma = nelo_i / (800 / ln 10)` per
///   game, times sqrt(2) per pair.
///
/// Unlike the Gaussian approximation `N (s1-s0)(2 mean-s0-s1) / (2 var)` this
/// does not blow up when the sample variance is tiny (e.g. all draws) or when
/// the score is far from the hypotheses. NaN if the MLE does not exist.
fn gsprt_llr(counts: &[u32], elo0: f64, elo1: f64, model: SprtModel) -> f64 {
    let total: u32 = counts.iter().sum();
    if total == 0 {
        return 0.0;
    }
    let len = counts.len();
    let reg: Vec<f64> = counts
        .iter()
        .map(|&c| if c == 0 { REG_EPS } else { c as f64 })
        .collect();
    let n: f64 = reg.iter().sum();
    let pdf: Vec<(f64, f64)> = reg
        .iter()
        .enumerate()
        .map(|(k, &c)| (k as f64 / (len - 1) as f64, c / n))
        .collect();
    let mle = |e: f64| match model {
        SprtModel::Logistic => mle_expected(&pdf, score_from_elo(e)),
        SprtModel::Normalized => {
            let per_game_t = e / NELO_SCALE;
            let t = if len == 5 {
                per_game_t * std::f64::consts::SQRT_2
            } else {
                per_game_t
            };
            mle_t_value(&pdf, 0.5, t)
        }
    };
    match (mle(elo0), mle(elo1)) {
        (Some(p0), Some(p1)) => reg
            .iter()
            .zip(p0.iter().zip(&p1))
            .map(|(&c, (&q0, &q1))| c * (q1 / q0).ln())
            .sum(),
        _ => f64::NAN,
    }
}

/// Trinomial W/D/L tally for engine1 (games treated as independent).
#[derive(Default, Clone, Copy)]
struct Tally {
    wins: u32,
    draws: u32,
    losses: u32,
}

impl Tally {
    fn add(&mut self, points: f64) {
        if points >= 1.0 {
            self.wins += 1;
        } else if points > 0.0 {
            self.draws += 1;
        } else {
            self.losses += 1;
        }
    }
    fn dist(&self) -> [(f64, f64); 3] {
        [
            (self.losses as f64, 0.0),
            (self.draws as f64, 0.5),
            (self.wins as f64, 1.0),
        ]
    }
    fn n(&self) -> f64 {
        (self.wins + self.draws + self.losses) as f64
    }
    fn score(&self) -> f64 {
        mean_var(&self.dist()).1
    }
    /// (elo, 95% half-width)
    fn elo(&self) -> (f64, f64) {
        let (n, s, var) = mean_var(&self.dist());
        if n == 0.0 {
            return (0.0, 0.0);
        }
        let se = (var / n).sqrt();
        let lo = elo_from_score(s - Z95 * se);
        let hi = elo_from_score(s + Z95 * se);
        (elo_from_score(s), (hi - lo) / 2.0)
    }
    /// Exact GSPRT LLR on the [L, D, W] counts (see `gsprt_llr`).
    fn llr(&self, elo0: f64, elo1: f64, model: SprtModel) -> f64 {
        gsprt_llr(&[self.losses, self.draws, self.wins], elo0, elo1, model)
    }
    /// Likelihood of superiority (draws ignored).
    fn los(&self) -> f64 {
        let w = self.wins as f64;
        let l = self.losses as f64;
        if w + l == 0.0 {
            return 0.5;
        }
        0.5 * (1.0 + erf((w - l) / (2.0 * (w + l)).sqrt()))
    }
}

/// Pentanomial tally of game pairs (same opening, colours swapped).
/// `counts[k]` = pairs in which engine1 scored k half-points out of 4,
/// i.e. LL, LD, DD|WL, WD, WW with pair score k/4.
#[derive(Default, Clone, Copy, Debug)]
struct Penta {
    counts: [u32; 5],
    /// The DD / WL split of `counts[2]` (informational only).
    dd: u32,
    wl: u32,
}

const PAIR_SCORES: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
/// Normalized-Elo scale: 800 / ln(10).
const NELO_SCALE: f64 = 347.435_585_522_601_46;

impl Penta {
    /// Record a pair from engine1's points in each game (0, 0.5 or 1).
    fn add_pair(&mut self, a: f64, b: f64) {
        let k = ((a + b) * 2.0).round().clamp(0.0, 4.0) as usize;
        self.counts[k] += 1;
        if k == 2 {
            if a == 0.5 {
                self.dd += 1;
            } else {
                self.wl += 1;
            }
        }
    }
    fn n(&self) -> u32 {
        self.counts.iter().sum()
    }
    fn dist(&self) -> [(f64, f64); 5] {
        let mut d = [(0.0, 0.0); 5];
        for k in 0..5 {
            d[k] = (self.counts[k] as f64, PAIR_SCORES[k]);
        }
        d
    }
    /// (pairs, mean pair score, per-pair variance)
    fn stats(&self) -> (f64, f64, f64) {
        mean_var(&self.dist())
    }
    fn score(&self) -> f64 {
        self.stats().1
    }
    /// Logistic Elo and 95% half-width from the pentanomial variance.
    fn elo(&self) -> (f64, f64) {
        let (n, s, var) = self.stats();
        if n == 0.0 {
            return (0.0, 0.0);
        }
        let se = (var / n).sqrt();
        let lo = elo_from_score(s - Z95 * se);
        let hi = elo_from_score(s + Z95 * se);
        (elo_from_score(s), (hi - lo) / 2.0)
    }
    /// Normalized Elo (fishtest convention: per-game sigma = sqrt(2 * var_pair)).
    fn nelo(&self) -> (f64, f64) {
        let (n, s, var) = self.stats();
        if n == 0.0 || var <= 0.0 {
            return (0.0, 0.0);
        }
        let sigma_pg = (2.0 * var).sqrt();
        let nelo = NELO_SCALE * (s - 0.5) / sigma_pg + 0.0;
        let err = NELO_SCALE * Z95 * (var / n).sqrt() / sigma_pg;
        (nelo, err)
    }
    fn los(&self) -> f64 {
        let (n, s, var) = self.stats();
        if n == 0.0 || var <= 0.0 {
            return 0.5;
        }
        normal_cdf((s - 0.5) / (var / n).sqrt())
    }
    /// Exact pentanomial GSPRT LLR (fishtest; see `gsprt_llr`), one sample
    /// per game pair.
    fn llr(&self, elo0: f64, elo1: f64, model: SprtModel) -> f64 {
        gsprt_llr(&self.counts, elo0, elo1, model)
    }
    fn compact(&self) -> String {
        let c = self.counts;
        format!("{}-{}-{}-{}-{}", c[0], c[1], c[2], c[3], c[4])
    }
}

// ------------------------------------------------------------------- pgn ---

fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}.{m:02}.{d:02}")
}

/// Movetext tokens with move numbers, starting at `fullmove` with `turn`.
fn movetext(san: &[String], turn: Color, fullmove: u32) -> Vec<String> {
    let offset = if turn == Color::Black { 1 } else { 0 };
    san.iter()
        .enumerate()
        .map(|(i, s)| {
            let k = i + offset;
            let number = fullmove as usize + k / 2;
            if k % 2 == 0 {
                format!("{number}. {s}")
            } else if i == 0 {
                format!("{number}... {s}")
            } else {
                s.clone()
            }
        })
        .collect()
}

fn pgn_text(rec: &GameRecord, cfg: &Config, openings: &[Opening]) -> String {
    let (w, b, wtc, btc) = if rec.engine1_white {
        (&cfg.name1, &cfg.name2, cfg.tc1, cfg.tc2)
    } else {
        (&cfg.name2, &cfg.name1, cfg.tc2, cfg.tc1)
    };
    let mut tags = vec![
        ("Event", "chessmind selfplay".to_string()),
        ("Site", "local".to_string()),
        ("Date", today()),
        ("Round", format!("{}.{}", rec.pair + 1, rec.index % 2 + 1)),
        ("White", w.clone()),
        ("Black", b.clone()),
        ("Result", rec.outcome.pgn().to_string()),
    ];
    if wtc == btc {
        tags.push(("TimeControl", wtc.pgn()));
    } else {
        tags.push(("WhiteTimeControl", wtc.pgn()));
        tags.push(("BlackTimeControl", btc.pgn()));
    }
    if let Some(f) = &rec.start_fen {
        tags.push(("SetUp", "1".to_string()));
        tags.push(("FEN", f.clone()));
    }
    tags.push(("PlyCount", rec.san.len().to_string()));
    tags.push(("Opening", openings[rec.opening].label.clone()));
    tags.push(("Termination", rec.reason.clone()));

    let mut out = String::new();
    for (k, v) in tags {
        out.push_str(&format!("[{k} \"{}\"]\n", v.replace('"', "'")));
    }
    out.push('\n');
    let mut line = String::new();
    for token in movetext(&rec.san, rec.start_turn, rec.start_fullmove) {
        if line.len() + token.len() + 1 > 79 {
            out.push_str(line.trim_end());
            out.push('\n');
            line.clear();
        }
        line.push_str(&token);
        line.push(' ');
    }
    line.push_str(&format!("{{{}}} {}", rec.reason, rec.outcome.pgn()));
    out.push_str(&line);
    out.push_str("\n\n");
    out
}

// ------------------------------------------------------------------ main ---

/// Plays whole pairs (both colours of one opening) until the schedule is
/// exhausted or `stop` is set; in-flight pairs are always finished.
fn worker(
    cfg: Arc<Config>,
    openings: Arc<Vec<Opening>>,
    plan: Arc<Vec<usize>>,
    next: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<GameRecord>,
) -> Result<(), String> {
    let mut e1: Option<EngineProc> = None;
    let mut e2: Option<EngineProc> = None;
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let pair = next.fetch_add(1, Ordering::AcqRel);
        if pair >= plan.len() {
            return Ok(());
        }
        let opening_idx = plan[pair];
        for game in 0..2 {
            if e1.is_none() {
                e1 = Some(EngineProc::start(&cfg.engine1, &cfg)?);
            }
            if e2.is_none() {
                e2 = Some(EngineProc::start(&cfg.engine2, &cfg)?);
            }
            let engine1_white = game == 0;
            let index = pair * 2 + game;
            let opening = &openings[opening_idx];
            let (p1, p2) = (e1.as_mut().unwrap(), e2.as_mut().unwrap());
            let rec = if engine1_white {
                play_game(
                    index,
                    pair,
                    opening_idx,
                    opening,
                    true,
                    p1,
                    p2,
                    [cfg.tc1, cfg.tc2],
                    &cfg,
                )
            } else {
                play_game(
                    index,
                    pair,
                    opening_idx,
                    opening,
                    false,
                    p2,
                    p1,
                    [cfg.tc2, cfg.tc1],
                    &cfg,
                )
            };
            let (restart1, restart2) = if engine1_white {
                rec.restart
            } else {
                (rec.restart.1, rec.restart.0)
            };
            if restart1 {
                e1 = None;
            }
            if restart2 {
                e2 = None;
            }
            if tx.send(rec).is_err() {
                return Ok(());
            }
        }
    }
}

struct Report<'a> {
    cfg: &'a Config,
    penta: Penta,
    tally: Tally,
    bounds: (f64, f64),
}

impl Report<'_> {
    fn status_line(&self, elapsed: f64) -> String {
        let (elo, err) = self.penta.elo();
        let games = self.tally.n();
        format!(
            "pairs {}/{} | penta {} | score {:.1}% | Elo {:+.1} +/- {:.1} | nElo {:+.1} | LLR {:.2} ({:.2}, {:.2}) | {:.1} games/min",
            self.penta.n(),
            self.cfg.pairs,
            self.penta.compact(),
            100.0 * self.penta.score(),
            elo,
            err,
            self.penta.nelo().0,
            self.penta
                .llr(self.cfg.elo0, self.cfg.elo1, self.cfg.sprt_model),
            self.bounds.0,
            self.bounds.1,
            games * 60.0 / elapsed.max(1e-3),
        )
    }
}

fn main() {
    let cfg = Arc::new(parse_args());

    let openings = match &cfg.openings {
        Some(path) => load_openings(path).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        }),
        None => builtin_openings(),
    };
    if let Err(e) = validate_openings(&openings) {
        eprintln!("{e}");
        std::process::exit(1);
    }
    let plan = Arc::new(schedule(openings.len(), cfg.pairs, cfg.order, cfg.seed));
    let openings = Arc::new(openings);

    let tc_desc = if cfg.tc1 == cfg.tc2 {
        format!("tc {}", cfg.tc1.describe())
    } else {
        format!(
            "tc {} {} vs {} {}",
            cfg.name1,
            cfg.tc1.describe(),
            cfg.name2,
            cfg.tc2.describe()
        )
    };
    println!(
        "{} ({}) vs {} ({}): {} pairs ({} games), {tc_desc}, concurrency {}, threads {}, hash {} MB, book {}",
        cfg.name1,
        cfg.engine1,
        cfg.name2,
        cfg.engine2,
        cfg.pairs,
        cfg.pairs * 2,
        cfg.concurrency,
        cfg.threads,
        cfg.hash_mb,
        if cfg.no_book { "off" } else { "engine" }
    );
    println!(
        "openings: {} ({} positions, {} order, seed {})",
        cfg.openings.as_deref().unwrap_or("built-in lines"),
        openings.len(),
        if cfg.order == OpeningOrder::Random {
            "random"
        } else {
            "sequential"
        },
        cfg.seed
    );
    if cfg.sprt {
        println!(
            "SPRT: exact pentanomial GSPRT, {} Elo, elo0={} elo1={} alpha={} beta={} (no verdict before {} pairs)",
            cfg.sprt_model.name(),
            cfg.elo0,
            cfg.elo1,
            cfg.alpha,
            cfg.beta,
            cfg.sprt_min_pairs
        );
    }

    let mut pgn_file = cfg.pgn.as_ref().map(|path| {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap_or_else(|e| {
                eprintln!("cannot open pgn '{path}': {e}");
                std::process::exit(1);
            })
    });

    let next = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::new();
    for _ in 0..cfg.concurrency.min(cfg.pairs) {
        let (cfg, openings, plan, next, stop, tx, errors) = (
            cfg.clone(),
            openings.clone(),
            plan.clone(),
            next.clone(),
            stop.clone(),
            tx.clone(),
            errors.clone(),
        );
        handles.push(thread::spawn(move || {
            if let Err(e) = worker(cfg, openings, plan, next, stop.clone(), tx) {
                eprintln!("worker error: {e}");
                errors.lock().unwrap().push(e);
                stop.store(true, Ordering::Release);
            }
        }));
    }
    drop(tx);

    let started = Instant::now();
    let mut report = Report {
        cfg: &cfg,
        penta: Penta::default(),
        tally: Tally::default(),
        bounds: sprt_bounds(cfg.alpha, cfg.beta),
    };
    let (lower, upper) = report.bounds;
    let mut white_score = 0.0f64;
    let mut reasons: Vec<(String, u32)> = Vec::new();
    let mut illegal = 0u32;
    // Time forfeits suffered by (engine1, engine2).
    let mut time_losses = (0u32, 0u32);
    let mut done = 0usize;
    let mut sprt_verdict: Option<&str> = None;
    let mut half_pairs: HashMap<usize, f64> = HashMap::new();
    let mut last_status = Instant::now();

    for rec in rx {
        done += 1;
        let points = rec.engine1_points();
        report.tally.add(points);
        white_score += match rec.outcome {
            Outcome::WhiteWins => 1.0,
            Outcome::Draw => 0.5,
            Outcome::BlackWins => 0.0,
        };
        if rec.reason.contains("illegal move") {
            illegal += 1;
        }
        if rec.reason.contains("loses on time") {
            if points == 0.0 {
                time_losses.0 += 1;
            } else {
                time_losses.1 += 1;
            }
        }
        // Group reasons without the per-move detail (e.g. times in ms).
        let key = rec
            .reason
            .split(['(', '\''])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        match reasons.iter_mut().find(|(k, _)| *k == key) {
            Some((_, c)) => *c += 1,
            None => reasons.push((key, 1)),
        }
        let pair_done = match half_pairs.remove(&rec.pair) {
            Some(first) => {
                report.penta.add_pair(first, points);
                true
            }
            None => {
                half_pairs.insert(rec.pair, points);
                false
            }
        };

        if !cfg.quiet {
            let (elo, err) = report.penta.elo();
            println!(
                "[{done:>4}/{}] pair {:>4}.{} opening {:>4} {} (W) vs {}: {:<7} {:<28} plies {:>3} | penta {} elo {:+.1} +/- {:.1} llr {:.2}",
                cfg.pairs * 2,
                rec.pair + 1,
                rec.index % 2 + 1,
                rec.opening + 1,
                if rec.engine1_white {
                    &cfg.name1
                } else {
                    &cfg.name2
                },
                if rec.engine1_white {
                    &cfg.name2
                } else {
                    &cfg.name1
                },
                rec.outcome.pgn(),
                rec.reason,
                rec.san.len(),
                report.penta.compact(),
                elo,
                err,
                report.penta.llr(cfg.elo0, cfg.elo1, cfg.sprt_model),
            );
        } else if pair_done && last_status.elapsed().as_secs_f64() >= cfg.status_interval {
            last_status = Instant::now();
            println!("{}", report.status_line(started.elapsed().as_secs_f64()));
        }
        if let Some(f) = pgn_file.as_mut() {
            let _ = f.write_all(pgn_text(&rec, &cfg, &openings).as_bytes());
            let _ = f.flush();
        }
        // With a handful of pairs the 1e-3 regularization prior still weighs on
        // the MLE, so as a safety net no verdict before --sprt-min-pairs
        // complete pairs.
        if cfg.sprt && pair_done && sprt_verdict.is_none() && report.penta.n() >= cfg.sprt_min_pairs
        {
            let llr = report.penta.llr(cfg.elo0, cfg.elo1, cfg.sprt_model);
            if llr >= upper {
                sprt_verdict = Some("H1 accepted");
            } else if llr <= lower {
                sprt_verdict = Some("H0 accepted");
            }
            if let Some(v) = sprt_verdict {
                println!(
                    "SPRT: {v} (LLR {llr:.2}) after {} pairs, finishing pairs in progress...",
                    report.penta.n()
                );
                stop.store(true, Ordering::Release);
            }
        }
    }
    for h in handles {
        let _ = h.join();
    }

    let elapsed = started.elapsed().as_secs_f64();
    if cfg.quiet {
        println!("{}", report.status_line(elapsed));
    }
    let penta = report.penta;
    let tally = report.tally;
    let n = tally.n();
    println!();
    println!(
        "=== Result after {} games / {} complete pairs ({elapsed:.1} s) ===",
        n as u32,
        penta.n()
    );
    let c = penta.counts;
    println!(
        "Pentanomial [LL LD DD+WL WD WW] for {}: [{} {} {} {} {}]  (DD {} / WL {})",
        cfg.name1, c[0], c[1], c[2], c[3], c[4], penta.dd, penta.wl
    );
    let (elo, err) = penta.elo();
    let (nelo, nerr) = penta.nelo();
    println!(
        "Pair score: {:.2}%   Elo: {elo:+.1} +/- {err:.1} (95%)   nElo: {nelo:+.1} +/- {nerr:.1}   LOS: {:.1}%",
        100.0 * penta.score(),
        100.0 * penta.los()
    );
    println!(
        "SPRT (exact pentanomial GSPRT, {} Elo) elo0={} elo1={} alpha={} beta={}: LLR {:.2} [{:.2}, {:.2}]{}",
        cfg.sprt_model.name(),
        cfg.elo0,
        cfg.elo1,
        cfg.alpha,
        cfg.beta,
        penta.llr(cfg.elo0, cfg.elo1, cfg.sprt_model),
        lower,
        upper,
        match sprt_verdict {
            Some(v) => format!(" -> {v}"),
            None if cfg.sprt => " -> inconclusive".to_string(),
            None => String::new(),
        }
    );
    let (telo, terr) = tally.elo();
    println!(
        "Trinomial (games as independent): +{} ={} -{}  score {:.1}%  Elo {telo:+.1} +/- {terr:.1}  LOS {:.1}%  LLR {:.2}",
        tally.wins,
        tally.draws,
        tally.losses,
        100.0 * tally.score(),
        100.0 * tally.los(),
        tally.llr(cfg.elo0, cfg.elo1, cfg.sprt_model),
    );
    println!(
        "Draw ratio: {:.1}%   White score: {:.1}%   Illegal moves: {illegal}   Time forfeits: {} {} / {} {}",
        100.0 * tally.draws as f64 / n.max(1.0),
        100.0 * white_score / n.max(1.0),
        cfg.name1,
        time_losses.0,
        cfg.name2,
        time_losses.1,
    );
    println!("Terminations:");
    for (reason, count) in &reasons {
        println!("  {reason:<30} {count}");
    }
    let errors = errors.lock().unwrap();
    if !errors.is_empty() {
        for e in errors.iter() {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn penta(counts: [u32; 5]) -> Penta {
        Penta {
            counts,
            ..Penta::default()
        }
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn pair_classification() {
        let mut p = Penta::default();
        p.add_pair(0.0, 0.0); // LL
        p.add_pair(0.0, 0.5); // LD
        p.add_pair(0.5, 0.0); // DL
        p.add_pair(0.5, 0.5); // DD
        p.add_pair(1.0, 0.0); // WL
        p.add_pair(0.0, 1.0); // LW
        p.add_pair(1.0, 0.5); // WD
        p.add_pair(1.0, 1.0); // WW
        assert_eq!(p.counts, [1, 2, 3, 1, 1]);
        assert_eq!((p.dd, p.wl), (1, 2));
        assert_eq!(p.n(), 8);
    }

    #[test]
    fn symmetric_distribution_is_zero_elo() {
        let p = penta([1, 2, 3, 2, 1]);
        let (n, mean, var) = p.stats();
        assert_eq!(n, 9.0);
        assert!(close(mean, 0.5, 1e-12));
        assert!(close(var, 0.75 / 9.0, 1e-12));
        let (elo, err) = p.elo();
        assert!(close(elo, 0.0, 1e-9));
        // score 0.5 +/- 1.96*sqrt(var/9) = 0.5 +/- 0.18861 -> +/- 137.9 Elo.
        assert!(close(err, 137.9, 0.2), "err {err}");
        // Symmetric hypotheses around 0: LLR is exactly 0.
        for model in [SprtModel::Logistic, SprtModel::Normalized] {
            assert!(close(p.llr(-5.0, 5.0, model), 0.0, 1e-9));
        }
        assert!(close(p.los(), 0.5, 1e-9));
    }

    use SprtModel::{Logistic, Normalized};

    /// The old Gaussian approximation (fishtest `LLR_alt2`, times N), kept
    /// here only to compare against the exact GSPRT.
    fn approx_llr(p: &Penta, elo0: f64, elo1: f64) -> f64 {
        let mut d = p.dist();
        for c in d.iter_mut() {
            if c.0 == 0.0 {
                c.0 = REG_EPS;
            }
        }
        let (n, mean, var) = mean_var(&d);
        let (s0, s1) = (score_from_elo(elo0), score_from_elo(elo1));
        n * (s1 - s0) * (2.0 * mean - s0 - s1) / (2.0 * var)
    }

    #[test]
    fn llr_matches_fishtest_reference_values() {
        // Produced by fishtest server/fishtest/stats/LLRcalc.py (LLR_logistic,
        // LLR_normalized); the first two are its own self-test samples.
        let cases: [([u32; 5], f64, f64, SprtModel, f64); 13] = [
            (
                [10789, 19328, 33806, 19402, 10543],
                -3.0,
                1.0,
                Logistic,
                2.1310678117855075,
            ),
            (
                [10789, 19328, 33806, 19402, 10543],
                -3.0,
                1.0,
                Normalized,
                0.3008697855029647,
            ),
            (
                [39, 2226, 31451, 2412, 40],
                0.2,
                0.9,
                Logistic,
                2.162542580049078,
            ),
            (
                [39, 2226, 31451, 2412, 40],
                0.764,
                3.439,
                Normalized,
                2.1629408695841943,
            ),
            ([0, 10, 20, 50, 20], 0.0, 5.0, Logistic, 1.4286861064744187),
            ([0, 10, 20, 50, 20], 0.0, 5.0, Normalized, 1.268896403977093),
            ([3, 20, 45, 25, 7], 0.0, 10.0, Logistic, 0.6959726064980855),
            (
                [3, 20, 45, 25, 7],
                0.0,
                10.0,
                Normalized,
                0.4949702786678729,
            ),
            ([0, 0, 50, 0, 0], 0.0, 5.0, Logistic, -0.7176058361124835),
            ([0, 0, 50, 0, 0], 0.0, 5.0, Normalized, -0.01756124857176134),
            ([2, 5, 80, 8, 5], 0.0, 5.0, Logistic, 0.5384862572588683),
            ([2, 5, 80, 8, 5], 0.0, 5.0, Normalized, 0.26570559377961006),
            ([0, 0, 40, 10, 0], 0.0, 5.0, Normalized, 0.31255435949946725),
        ];
        for (counts, e0, e1, model, want) in cases {
            let got = penta(counts).llr(e0, e1, model);
            assert!(
                close(got, want, 1e-6 * want.abs().max(1.0)),
                "{counts:?} {e0}/{e1} {model:?}: got {got}, fishtest {want}"
            );
        }
        // Trinomial [L, D, W].
        let t = Tally {
            losses: 65804,
            draws: 56553,
            wins: 65388,
        };
        assert!(close(t.llr(-3.0, 1.0, Logistic), 2.049042617996086, 1e-6));
        assert!(close(
            t.llr(-3.0, 1.0, Normalized),
            0.49171177626817564,
            1e-6
        ));
    }

    #[test]
    fn llr_sign_follows_the_result() {
        let good = penta([0, 10, 20, 50, 20]);
        let bad = penta([20, 50, 20, 10, 0]);
        for model in [Logistic, Normalized] {
            assert!(good.llr(0.0, 5.0, model) > 0.0);
            assert!(bad.llr(0.0, 5.0, model) < 0.0);
            // Mirror image of the counts <=> mirror image of the hypotheses.
            assert!(close(
                good.llr(0.0, 5.0, model),
                -bad.llr(-5.0, 0.0, model),
                1e-9
            ));
        }
    }

    #[test]
    fn exact_matches_approximation_for_large_samples() {
        // Realistic fishtest-like run: tens of thousands of pairs, score near
        // the hypotheses, moderate variance.
        let p = penta([1187, 7410, 13475, 7378, 1164]);
        let exact = p.llr(-3.0, 1.0, Logistic);
        let approx = approx_llr(&p, -3.0, 1.0);
        assert!(
            close(exact, approx, 0.02 * approx.abs()),
            "exact {exact} approx {approx}"
        );
        let q = penta([10789, 19328, 33806, 19402, 10543]);
        let (exact, approx) = (q.llr(-3.0, 1.0, Logistic), approx_llr(&q, -3.0, 1.0));
        assert!(
            close(exact, approx, 0.02 * approx.abs()),
            "exact {exact} approx {approx}"
        );
    }

    #[test]
    fn exact_is_conservative_for_tiny_variance() {
        // All draws: the regularized variance is ~1e-5 and the Gaussian
        // approximation screams "H0" after 50 pairs; the exact LLR does not.
        let dd = penta([0, 0, 50, 0, 0]);
        let approx = approx_llr(&dd, 0.0, 5.0);
        assert!(approx < -100.0, "approx {approx}");
        for model in [Logistic, Normalized] {
            let exact = dd.llr(0.0, 5.0, model);
            assert!(
                exact.is_finite() && (-2.94..0.0).contains(&exact),
                "{model:?} {exact}"
            );
        }
        // 20% WD pairs among the draws: the approximation accepts H1 at
        // once, the exact test is still inconclusive.
        let wd = penta([0, 0, 80, 20, 0]);
        assert!(approx_llr(&wd, 0.0, 5.0) > 2.94);
        for model in [Logistic, Normalized] {
            let exact = wd.llr(0.0, 5.0, model);
            assert!(exact > 0.0 && exact < 2.94, "{model:?} {exact}");
        }
        // Score far from the hypotheses: the approximation overstates the
        // evidence (it assumes a normal likelihood with the sample variance).
        let p = penta([0, 10, 20, 50, 20]);
        assert!(p.llr(0.0, 5.0, Logistic) < approx_llr(&p, 0.0, 5.0));
    }

    #[test]
    fn mle_satisfies_its_constraint() {
        let pdf: Vec<(f64, f64)> = [3.0, 20.0, 45.0, 25.0, 7.0]
            .iter()
            .enumerate()
            .map(|(k, &c)| (k as f64 / 4.0, c / 100.0))
            .collect();
        let with_probs = |probs: &[f64]| -> Vec<(f64, f64)> {
            pdf.iter().zip(probs).map(|(&(a, _), &p)| (p, a)).collect()
        };
        let p = mle_expected(&pdf, 0.52).unwrap();
        let (n, mean, _) = mean_var(&with_probs(&p));
        assert!(close(n, 1.0, 1e-9) && close(mean, 0.52, 1e-9));
        let p = mle_t_value(&pdf, 0.5, 0.1).unwrap();
        let (n, mean, var) = mean_var(&with_probs(&p));
        assert!(close(n, 1.0, 1e-9) && close((mean - 0.5) / var.sqrt(), 0.1, 1e-9));
    }

    #[test]
    fn pentanomial_variance_is_smaller_than_trinomial_for_correlated_pairs() {
        // Pairs are mostly WL / LW (opening decides the winner): games are
        // strongly anti-correlated, pentanomial error bar must be tighter.
        let p = penta([2, 5, 80, 8, 5]);
        let mut t = Tally::default();
        for (k, &c) in p.counts.iter().enumerate() {
            let games: [f64; 2] = match k {
                0 => [0.0, 0.0],
                1 => [0.0, 0.5],
                2 => [1.0, 0.0],
                3 => [1.0, 0.5],
                _ => [1.0, 1.0],
            };
            for _ in 0..c {
                t.add(games[0]);
                t.add(games[1]);
            }
        }
        assert!(close(p.score(), t.score(), 1e-12));
        assert!(p.elo().1 < t.elo().1);
    }

    #[test]
    fn empty_and_degenerate() {
        let p = Penta::default();
        assert_eq!(p.elo(), (0.0, 0.0));
        assert_eq!(p.llr(0.0, 5.0, Logistic), 0.0);
        // All draws: zero variance; regularized LLR is finite and negative
        // (a 50% score is evidence for elo0 = 0 against elo1 = 5).
        let d = penta([0, 0, 50, 0, 0]);
        assert_eq!(d.elo(), (0.0, 0.0));
        for model in [Logistic, Normalized] {
            let llr = d.llr(0.0, 5.0, model);
            assert!(llr.is_finite() && llr < 0.0);
        }
        // A single pair of any kind: finite.
        for k in 0..5 {
            let mut c = [0; 5];
            c[k] = 1;
            for model in [Logistic, Normalized] {
                assert!(penta(c).llr(0.0, 5.0, model).is_finite(), "{c:?} {model:?}");
            }
        }
    }

    #[test]
    fn bounds_and_elo_conversions() {
        let (lo, hi) = sprt_bounds(0.05, 0.05);
        assert!(close(lo, -2.944, 1e-3) && close(hi, 2.944, 1e-3));
        for e in [-100.0, -5.0, 0.0, 5.0, 30.0] {
            assert!(close(elo_from_score(score_from_elo(e)), e, 1e-6));
        }
        assert!(close(normal_cdf(1.959964), 0.975, 1e-4));
    }

    #[test]
    fn time_control_specs() {
        assert_eq!(
            TimeControl::parse("1000+20"),
            Some(TimeControl::Clock {
                base_ms: 1000,
                inc_ms: 20
            })
        );
        assert_eq!(
            TimeControl::parse("5000"),
            Some(TimeControl::Clock {
                base_ms: 5000,
                inc_ms: 0
            })
        );
        assert_eq!(
            TimeControl::parse("movetime=50"),
            Some(TimeControl::MoveTime(50))
        );
        assert_eq!(TimeControl::parse("depth=4"), Some(TimeControl::Depth(4)));
        assert_eq!(TimeControl::parse("abc"), None);
    }

    #[test]
    fn schedule_keeps_every_opening_once_per_cycle() {
        let s = schedule(5, 12, OpeningOrder::Random, 7);
        assert_eq!(s.len(), 12);
        let mut first: Vec<usize> = s[..5].to_vec();
        first.sort();
        assert_eq!(first, vec![0, 1, 2, 3, 4]);
        assert_eq!(s, schedule(5, 12, OpeningOrder::Random, 7), "seeded");
        assert_eq!(
            schedule(3, 4, OpeningOrder::Sequential, 1),
            vec![0, 1, 2, 0]
        );
    }

    #[test]
    fn builtin_openings_are_legal() {
        let ops = builtin_openings();
        assert_eq!(ops.len(), 52);
        validate_openings(&ops).unwrap();
    }

    #[test]
    fn movetext_numbering_from_fen() {
        let san: Vec<String> = ["Nf6", "c4", "e6"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            movetext(&san, Color::Black, 1),
            vec!["1... Nf6", "2. c4", "e6"]
        );
        assert_eq!(
            movetext(&san, Color::White, 7),
            vec!["7. Nf6", "c4", "8. e6"]
        );
    }
}
