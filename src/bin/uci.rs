//! Minimal UCI front-end for the chessmind engine.
//!
//! Supports: uci, isready, setoption (Hash, Threads, OwnBook), ucinewgame,
//! position startpos|fen ... [moves ...], go (wtime/btime/winc/binc/movestogo/
//! movetime/depth/infinite), stop, ponderhit (ignored), quit.
//!
//! The search runs on a background thread so `stop` and `isready` are handled
//! while it is thinking.
//!
//! Usage: cargo run --release --bin uci

use chessmind::board::Board;
use chessmind::engine::{Engine, TimeConfig};
use chessmind::game::Game;
use chessmind::pieces::{Color, Piece, PieceType};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const DEFAULT_HASH_MB: usize = 64;
const MAX_HASH_MB: usize = 4096;
const MAX_THREADS: usize = 256;
/// Bytes per transposition-table entry (key + data, two u64 atomics).
const TT_ENTRY_BYTES: usize = 16;

fn send(line: &str) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

struct Options {
    hash_mb: usize,
    threads: usize,
    own_book: bool,
}

fn new_engine(opts: &Options) -> Engine {
    let entries = (opts.hash_mb.max(1) * 1024 * 1024) / TT_ENTRY_BYTES;
    Engine::with_threads_and_table(64, opts.threads.max(1), entries)
}

struct Search {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

struct Uci {
    opts: Options,
    engine: Arc<Mutex<Engine>>,
    moves_played: Vec<String>,
    start_fen: Option<String>,
    search: Option<Search>,
}

impl Uci {
    fn new() -> Self {
        let opts = Options {
            hash_mb: DEFAULT_HASH_MB,
            threads: 1,
            own_book: true,
        };
        let engine = Arc::new(Mutex::new(new_engine(&opts)));
        Self {
            opts,
            engine,
            moves_played: Vec::new(),
            start_fen: None,
            search: None,
        }
    }

    /// Stop any running search and wait for it to print its bestmove.
    fn stop_search(&mut self) {
        if let Some(search) = self.search.take() {
            search.stop.store(true, Ordering::Release);
            let _ = search.handle.join();
        }
    }

    /// Wait for a running search to finish on its own (used before commands
    /// that must not race with it).
    fn wait_search(&mut self) {
        if let Some(search) = self.search.take() {
            let _ = search.handle.join();
        }
    }

    fn rebuild_engine(&mut self) {
        self.wait_search();
        self.engine = Arc::new(Mutex::new(new_engine(&self.opts)));
    }

    fn handle_setoption(&mut self, tokens: &[&str]) {
        // setoption name <id...> [value <x...>]
        let name_pos = tokens.iter().position(|t| *t == "name");
        let value_pos = tokens.iter().position(|t| *t == "value");
        let Some(np) = name_pos else { return };
        let name_end = value_pos.unwrap_or(tokens.len());
        let name = tokens[np + 1..name_end].join(" ").to_ascii_lowercase();
        let value = value_pos
            .map(|vp| tokens[vp + 1..].join(" "))
            .unwrap_or_default();

        match name.as_str() {
            "hash" => {
                if let Ok(mb) = value.trim().parse::<usize>() {
                    self.opts.hash_mb = mb.clamp(1, MAX_HASH_MB);
                    self.rebuild_engine();
                }
            }
            "threads" => {
                if let Ok(n) = value.trim().parse::<usize>() {
                    self.opts.threads = n.clamp(1, MAX_THREADS);
                    self.wait_search();
                    self.engine.lock().unwrap().set_threads(self.opts.threads);
                }
            }
            "ownbook" => {
                self.opts.own_book = value.trim().eq_ignore_ascii_case("true");
            }
            _ => send(&format!("info string unknown option '{name}'")),
        }
    }

    fn handle_position(&mut self, tokens: &[&str]) {
        let moves_pos = tokens.iter().position(|t| *t == "moves");
        let head_end = moves_pos.unwrap_or(tokens.len());
        match tokens.get(1) {
            Some(&"startpos") => self.start_fen = None,
            Some(&"fen") => {
                let fen = tokens[2..head_end].join(" ");
                if game_from_fen(&fen).is_none() {
                    send(&format!("info string invalid fen '{fen}'"));
                    return;
                }
                self.start_fen = Some(fen);
            }
            _ => return,
        }
        self.moves_played = moves_pos
            .map(|mp| tokens[mp + 1..].iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();
    }

    fn build_game(&self) -> Game {
        let mut game = match &self.start_fen {
            Some(fen) => game_from_fen(fen).unwrap_or_else(Game::new),
            None => Game::new(),
        };
        for mv in &self.moves_played {
            let mv = mv.to_ascii_lowercase();
            if mv.len() < 4 || !mv.is_ascii() {
                send(&format!("info string bad move '{mv}'"));
                break;
            }
            let (from, to) = mv.split_at(2);
            if !game.make_move(from, to) {
                send(&format!("info string illegal move '{mv}'"));
                break;
            }
        }
        game
    }

    fn handle_go(&mut self, tokens: &[&str]) {
        self.stop_search();

        let mut config = TimeConfig::new();
        let mut i = 1;
        let num = |i: usize| tokens.get(i + 1).and_then(|v| v.parse::<i64>().ok());
        while i < tokens.len() {
            match tokens[i] {
                "wtime" => config.wtime = num(i).map(|v| v.max(1) as u64),
                "btime" => config.btime = num(i).map(|v| v.max(1) as u64),
                "winc" => config.winc = num(i).map(|v| v.max(0) as u64),
                "binc" => config.binc = num(i).map(|v| v.max(0) as u64),
                "movestogo" => config.movestogo = num(i).map(|v| v.max(1) as u32),
                "movetime" => config.movetime = num(i).map(|v| v.max(1) as u64),
                "depth" => config.depth = num(i).map(|v| v.max(1) as u32),
                "infinite" | "ponder" => {
                    config.infinite = true;
                    i += 1;
                    continue;
                }
                _ => {
                    i += 1;
                    continue;
                }
            }
            i += 2;
        }
        let infinite = config.infinite;

        let mut game = self.build_game();
        // The book is keyed on move history from the start position only.
        let allow_book = self.opts.own_book && self.start_fen.is_none();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let engine = self.engine.clone();

        let handle = thread::spawn(move || {
            let legal = game.legal_moves();
            let start = Instant::now();
            let mut engine = engine.lock().unwrap();
            let result = if legal.is_empty() {
                None
            } else {
                engine.best_move_timed_opts(&mut game, &config, allow_book, stop_for_thread.clone())
            };
            let elapsed_ms = start.elapsed().as_millis() as u64;

            if let Some((_, depth)) = &result {
                let nodes = engine.last_search_nodes();
                let nps = nodes * 1000 / elapsed_ms.max(1);
                send(&format!(
                    "info depth {depth} nodes {nodes} time {elapsed_ms} nps {nps}"
                ));
            }
            drop(engine);

            // UCI: in infinite mode bestmove is only sent after `stop`.
            if infinite {
                while !stop_for_thread.load(Ordering::Acquire) {
                    thread::sleep(std::time::Duration::from_millis(2));
                }
            }

            let best = match result {
                Some(((from, to), _)) if legal.iter().any(|(f, t)| *f == from && *t == to) => {
                    format!("{from}{to}")
                }
                // Fall back to any legal move rather than resigning silently.
                _ => legal
                    .first()
                    .map(|(f, t)| format!("{f}{t}"))
                    .unwrap_or_else(|| "0000".to_string()),
            };
            send(&format!("bestmove {best}"));
        });

        self.search = Some(Search { stop, handle });
    }

    fn run(&mut self) {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let tokens: Vec<&str> = line.split_whitespace().collect();
            let Some(cmd) = tokens.first() else { continue };
            match *cmd {
                "uci" => {
                    send("id name chessmind");
                    send("id author Leopold Chappuis");
                    send(&format!(
                        "option name Hash type spin default {DEFAULT_HASH_MB} min 1 max {MAX_HASH_MB}"
                    ));
                    send(&format!(
                        "option name Threads type spin default 1 min 1 max {MAX_THREADS}"
                    ));
                    send("option name OwnBook type check default true");
                    send("uciok");
                }
                "isready" => send("readyok"),
                "setoption" => self.handle_setoption(&tokens),
                "ucinewgame" => {
                    self.stop_search();
                    self.rebuild_engine();
                    self.start_fen = None;
                    self.moves_played.clear();
                }
                "position" => {
                    self.wait_search();
                    self.handle_position(&tokens);
                }
                "go" => self.handle_go(&tokens),
                "stop" => self.stop_search(),
                "ponderhit" => {}
                "quit" => {
                    self.stop_search();
                    break;
                }
                _ => send(&format!("info string unknown command '{cmd}'")),
            }
        }
        self.stop_search();
    }
}

/// Build a `Game` from a FEN string (halfmove/fullmove counters are ignored).
fn game_from_fen(fen: &str) -> Option<Game> {
    let parts: Vec<&str> = fen.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let mut board = Board::new();
    let ranks: Vec<&str> = parts[0].split('/').collect();
    if ranks.len() != 8 {
        return None;
    }
    for (i, rank) in ranks.iter().enumerate() {
        let y = 7 - i;
        let mut x = 0usize;
        for c in rank.chars() {
            if let Some(d) = c.to_digit(10) {
                x += d as usize;
                continue;
            }
            let color = if c.is_ascii_uppercase() {
                Color::White
            } else {
                Color::Black
            };
            let piece_type = match c.to_ascii_lowercase() {
                'p' => PieceType::Pawn,
                'n' => PieceType::Knight,
                'b' => PieceType::Bishop,
                'r' => PieceType::Rook,
                'q' => PieceType::Queen,
                'k' => PieceType::King,
                _ => return None,
            };
            if x > 7 {
                return None;
            }
            board.set_index(x, y, Some(Piece { piece_type, color }));
            x += 1;
        }
        if x != 8 {
            return None;
        }
    }
    let turn = match parts[1] {
        "w" => Color::White,
        "b" => Color::Black,
        _ => return None,
    };
    let castling = parts.get(2).copied().unwrap_or("-");
    board.castling = [
        [castling.contains('K'), castling.contains('Q')],
        [castling.contains('k'), castling.contains('q')],
    ];
    board.en_passant = parts.get(3).and_then(|ep| {
        if *ep == "-" {
            None
        } else {
            Board::algebraic_to_index(ep)
        }
    });
    board.find_king(Color::White)?;
    board.find_king(Color::Black)?;

    let hash = board.hash(turn);
    let mut hash_counts = HashMap::new();
    hash_counts.insert(hash, 1);
    Some(Game {
        board,
        current_turn: turn,
        history: Vec::new(),
        hash_history: vec![hash],
        hash_counts,
        result: None,
    })
}

fn main() {
    Uci::new().run();
}
