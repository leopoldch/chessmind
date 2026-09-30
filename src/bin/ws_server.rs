use chessmind::{
    engine::{Engine, TimeConfig},
    game::Game,
    pieces::Color,
    san::parse_san,
};
use futures_util::{SinkExt, StreamExt};
use num_cpus;
use serde::Deserialize;
use serde_json;
use std::env;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn is_coordinate(mv: &str) -> bool {
    mv.len() >= 4
        && mv.as_bytes()[0].is_ascii_lowercase()
        && mv.as_bytes()[1].is_ascii_digit()
        && mv.as_bytes()[2].is_ascii_lowercase()
        && mv.as_bytes()[3].is_ascii_digit()
}

fn normalize_move(game: &mut Game, raw: &str) -> Option<(String, String)> {
    let mv = raw.replace('+', "").replace('#', "");
    if is_coordinate(&mv) {
        Some((mv[0..2].to_string(), mv[2..].to_string()))
    } else {
        parse_san(game, &mv, game.current_turn)
    }
}

#[derive(Deserialize)]
struct MoveEntry {
    #[serde(rename = "move")]
    mov: String,
    #[serde(rename = "color")]
    _color: String,
}

#[derive(Deserialize, Default, Clone, Debug)]
struct TimeControl {
    #[serde(default)]
    wtime: Option<u64>,
    #[serde(default)]
    btime: Option<u64>,
    #[serde(default)]
    winc: Option<u64>,
    #[serde(default)]
    binc: Option<u64>,
    #[serde(default)]
    movestogo: Option<u32>,
    #[serde(default)]
    depth: Option<u32>,
    #[serde(default)]
    movetime: Option<u64>,
}

impl TimeControl {
    fn to_time_config(&self) -> TimeConfig {
        if self.wtime.is_some() || self.btime.is_some() {
            TimeConfig {
                wtime: self.wtime,
                btime: self.btime,
                winc: self.winc,
                binc: self.binc,
                movestogo: self.movestogo,
                depth: self.depth,
                movetime: self.movetime,
                infinite: false,
            }
        } else if let Some(depth) = self.depth {
            TimeConfig::fixed_depth(depth)
        } else if let Some(movetime) = self.movetime {
            TimeConfig::fixed_time(movetime)
        } else {
            TimeConfig {
                wtime: Some(300_000), // 5 minutes
                btime: Some(300_000),
                winc: None,
                binc: None,
                movestogo: None,
                depth: None,
                movetime: None,
                infinite: false,
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClientMsg {
    #[serde(rename = "color")]
    Color { color: String },

    #[serde(rename = "move")]
    Move {
        #[serde(rename = "move")]
        mov: String,
        #[serde(flatten)]
        time: Option<TimeControl>,
    },

    #[serde(rename = "moves")]
    Moves {
        moves: Vec<MoveEntry>,
        #[serde(flatten)]
        time: Option<TimeControl>,
    },

    #[serde(rename = "go")]
    Go {
        #[serde(flatten)]
        time: Option<TimeControl>,
    },

    #[serde(rename = "time")]
    Time {
        #[serde(flatten)]
        time: TimeControl,
    },

    #[serde(rename = "stop")]
    Stop,

    #[serde(rename = "newgame")]
    NewGame,
}

/// One engine shared by every connection: the transposition table is allocated
/// and the Syzygy tables are loaded once, and survive extension reconnects.
/// Only one game is expected to be active at a time.
type SharedEngine = Arc<Mutex<Engine>>;

/// Share of the normal clock allocation used when the opponent played the move
/// we were pondering on (the TT is already warm for that position).
const PONDER_HIT_BUDGET_PERCENT: u64 = 40;
const PONDER_HIT_MIN_MS: u64 = 50;

fn lock_engine(engine: &SharedEngine) -> MutexGuard<'_, Engine> {
    engine.lock().unwrap_or_else(PoisonError::into_inner)
}

fn clone_game(game: &Game) -> Game {
    Game {
        board: game.board.clone(),
        current_turn: game.current_turn,
        history: game.history.clone(),
        hash_history: game.hash_history.clone(),
        hash_counts: game.hash_counts.clone(),
        result: game.result,
    }
}

/// Approximates the engine's normal soft allocation for a clock-based search and
/// returns a fixed movetime that is a fraction of it. `None` keeps the normal
/// budget (no clock, fixed depth/movetime, or time trouble).
fn ponder_hit_config(tc: &TimeControl, color: Color) -> Option<TimeConfig> {
    if tc.wtime.is_none() && tc.btime.is_none() {
        return None;
    }
    let (time_left, inc) = match color {
        Color::White => (tc.wtime.unwrap_or(60_000), tc.winc.unwrap_or(0)),
        Color::Black => (tc.btime.unwrap_or(60_000), tc.binc.unwrap_or(0)),
    };
    let in_crisis = if inc > 0 {
        time_left < 1_500
    } else {
        time_left < 5_000
    };
    if in_crisis {
        return None;
    }
    let overhead = if inc > 0 {
        time_left / 3000
    } else {
        time_left / 2000
    }
    .clamp(10, 50);
    let usable = time_left.saturating_sub(overhead);
    let moves_left = match tc.movestogo {
        Some(mtg) => mtg.max(1) as u64,
        None => match time_left {
            t if t < 30_000 => 15,
            t if t < 60_000 => 20,
            t if t < 180_000 => 25,
            t if t < 300_000 => 30,
            t if t < 600_000 => 35,
            _ => 40,
        },
    };
    let normal = usable / moves_left + inc * 3 / 4;
    let movetime = (normal * PONDER_HIT_BUDGET_PERCENT / 100).max(PONDER_HIT_MIN_MS);
    Some(TimeConfig {
        movetime: Some(movetime),
        depth: tc.depth,
        ..TimeConfig::default()
    })
}

struct SearchOutcome {
    best: Option<((String, String), u32)>,
    ponder: Option<(String, String)>,
    elapsed: Duration,
}

struct InflightSearch {
    generation: u64,
    handle: JoinHandle<SearchOutcome>,
    /// Stop flag of this search, raised when it becomes stale.
    stop: Arc<AtomicBool>,
}

/// Runs a search on the blocking pool. The job is skipped if the position was
/// invalidated while it waited for the engine, and it only starts pondering if
/// its result is still current when the search ends.
fn spawn_search(
    engine: SharedEngine,
    mut game: Game,
    config: TimeConfig,
    forced: Option<(String, String)>,
    generation: Arc<AtomicU64>,
    my_generation: u64,
    stop: Arc<AtomicBool>,
) -> JoinHandle<SearchOutcome> {
    tokio::task::spawn_blocking(move || {
        let start_time = Instant::now();
        let is_current = || generation.load(Ordering::SeqCst) == my_generation;
        let mut engine = lock_engine(&engine);
        if !is_current() {
            return SearchOutcome {
                best: None,
                ponder: None,
                elapsed: start_time.elapsed(),
            };
        }
        let best = match forced {
            Some(mv) => {
                engine.stop_ponder();
                Some((mv, 0))
            }
            None => engine.best_move_timed_opts(&mut game, &config, true, stop),
        };
        let mut ponder = None;
        if let Some(((s, e), _)) = &best {
            if is_current() && game.make_move(s, e) {
                ponder = engine.start_ponder(&game);
            }
        }
        SearchOutcome {
            best,
            ponder,
            elapsed: start_time.elapsed(),
        }
    })
}

/// Per-connection state. Engine work never runs on the async task, so control
/// messages (`stop`, `newgame`, new positions) are read while a search runs.
struct Conn {
    engine: SharedEngine,
    game: Game,
    my_color: Option<Color>,
    last_len: usize,
    /// False until a `moves` message synchronised the position after `color`
    /// or `newgame`; forces the first `moves` to be processed even if empty.
    synced: bool,
    time_control: TimeControl,
    /// Bumped whenever the position is invalidated; a search whose generation
    /// differs is stale and its move is never sent.
    generation: Arc<AtomicU64>,
    inflight: Option<InflightSearch>,
    /// A position-changing message arrived during a stale search: search again
    /// once the engine is free.
    deferred_search: bool,
    /// Root hash and predicted reply of the ponder search we started.
    ponder: Option<(u64, String, String)>,
    ponder_hit: bool,
}

impl Conn {
    fn new(engine: SharedEngine) -> Self {
        Self {
            engine,
            game: Game::new(),
            my_color: None,
            last_len: 0,
            synced: false,
            time_control: TimeControl::default(),
            generation: Arc::new(AtomicU64::new(0)),
            inflight: None,
            deferred_search: false,
            ponder: None,
            ponder_hit: false,
        }
    }

    /// Marks any running or queued search as stale.
    fn invalidate(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(search) = &self.inflight {
            search.stop.store(true, Ordering::Release);
        }
        self.ponder = None;
        self.ponder_hit = false;
    }

    /// Stops the background ponder if no search of ours holds the engine (a
    /// running search stops the ponder itself and won't start a stale one).
    async fn stop_ponder(&mut self) {
        self.ponder = None;
        if self.inflight.is_some() {
            return;
        }
        let engine = self.engine.clone();
        let _ = tokio::task::spawn_blocking(move || lock_engine(&engine).stop_ponder()).await;
    }

    fn start_search(&mut self, config: TimeConfig, forced: Option<(String, String)>) {
        let my_generation = self.generation.load(Ordering::SeqCst);
        let stop = Arc::new(AtomicBool::new(false));
        let handle = spawn_search(
            self.engine.clone(),
            clone_game(&self.game),
            config,
            forced,
            self.generation.clone(),
            my_generation,
            stop.clone(),
        );
        self.inflight = Some(InflightSearch {
            generation: my_generation,
            handle,
            stop,
        });
    }

    fn apply_opponent_move(&mut self, raw: &str) {
        if let Some((s, e)) = normalize_move(&mut self.game, raw) {
            let root = self.game.board.hash(self.game.current_turn);
            let hit = matches!(
                &self.ponder,
                Some((hash, ps, pe)) if *hash == root && *ps == s && *pe == e
            );
            if hit {
                println!("Ponder hit on {}{}", s, e);
            }
            self.invalidate();
            self.ponder_hit = hit;
            self.game.make_move(&s, &e);
            self.last_len += 1;
        }
    }

    async fn on_text(&mut self, txt: &str, out: &mut Vec<String>) {
        if let Ok(data) = serde_json::from_str::<ClientMsg>(txt) {
            match data {
                ClientMsg::Color { color } => {
                    self.invalidate();
                    self.stop_ponder().await;
                    self.my_color = match color.as_str() {
                        "white" => Some(Color::White),
                        _ => Some(Color::Black),
                    };
                    println!(
                        "AI colour set to: {}",
                        if self.my_color == Some(Color::White) {
                            "White"
                        } else {
                            "Black"
                        }
                    );
                    // The following `moves` message carries the position and
                    // drives the search.
                    self.game = Game::new();
                    self.last_len = 0;
                    self.synced = false;
                    self.deferred_search = false;
                    return;
                }

                ClientMsg::Move { mov, time } => {
                    if let Some(tc) = time {
                        self.time_control = tc;
                    }
                    self.apply_opponent_move(&mov);
                }

                ClientMsg::Moves { moves, time } => {
                    if let Some(tc) = time {
                        self.time_control = tc;
                    }

                    if self.synced && moves.len() == self.last_len {
                        return;
                    }

                    self.invalidate();
                    self.stop_ponder().await;
                    self.game = Game::new();
                    for entry in &moves {
                        if let Some((s, e)) = normalize_move(&mut self.game, &entry.mov) {
                            self.game.make_move(&s, &e);
                        }
                    }
                    self.last_len = moves.len();
                    self.synced = true;
                }

                ClientMsg::Go { time } => {
                    if let Some(tc) = time {
                        self.time_control = tc;
                    }
                    if self.inflight.is_some() {
                        println!("Search already running, ignoring go");
                        return;
                    }
                    self.ponder = None;
                    self.ponder_hit = false;
                    self.start_search(self.time_control.to_time_config(), None);
                    return;
                }

                ClientMsg::Time { time } => {
                    self.time_control = time;
                    println!("Time control updated: {:?}", self.time_control);
                    return;
                }

                ClientMsg::Stop => {
                    self.invalidate();
                    self.deferred_search = false;
                    self.stop_ponder().await;
                    println!("Search stopped");
                    return;
                }

                ClientMsg::NewGame => {
                    self.invalidate();
                    self.deferred_search = false;
                    self.stop_ponder().await;
                    self.game = Game::new();
                    self.last_len = 0;
                    self.synced = false;
                    self.time_control = TimeControl::default();
                    println!("New game started");
                    return;
                }
            }
        } else {
            self.apply_opponent_move(txt);
        }

        self.maybe_search(out).await;
    }

    /// Reports a finished game or starts our search if it is our turn.
    async fn maybe_search(&mut self, out: &mut Vec<String>) {
        let Some(color) = self.my_color else {
            self.stop_ponder().await;
            return;
        };

        if self.inflight.is_some() {
            // A stale search still owns the engine; search once it returns.
            self.deferred_search = true;
            return;
        }
        self.deferred_search = false;

        if let Some(res) = self.game.result {
            let result = if res == Color::White {
                "white"
            } else {
                "black"
            };
            out.push(format!("{{\"result\":\"{}\"}}", result));
            self.invalidate();
            self.stop_ponder().await;
            self.game = Game::new();
            self.last_len = 0;
            self.my_color = None;
            return;
        }

        if self.game.current_turn != color {
            // Not our turn (e.g. after a resync): nothing to ponder on.
            self.stop_ponder().await;
            return;
        }

        if color == Color::White && self.last_len == 0 {
            self.start_search(
                TimeConfig::default(),
                Some(("d2".to_string(), "d4".to_string())),
            );
            return;
        }

        let config = if self.ponder_hit {
            ponder_hit_config(&self.time_control, color)
                .inspect(|c| println!("Ponder hit: reduced budget {:?} ms", c.movetime))
                .unwrap_or_else(|| self.time_control.to_time_config())
        } else {
            self.time_control.to_time_config()
        };
        self.ponder_hit = false;
        self.start_search(config, None);
    }

    async fn on_search_done(
        &mut self,
        generation: u64,
        outcome: Result<SearchOutcome, tokio::task::JoinError>,
        out: &mut Vec<String>,
    ) {
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(err) => {
                eprintln!("Search task failed: {}", err);
                return;
            }
        };

        if generation != self.generation.load(Ordering::SeqCst) {
            println!("Discarding stale search result after {:?}", outcome.elapsed);
            if self.deferred_search {
                self.maybe_search(out).await;
            } else if outcome.ponder.is_some() {
                self.stop_ponder().await;
            }
            return;
        }

        println!("AI calculation took {:?}", outcome.elapsed);
        let Some(((s, e), depth)) = outcome.best else {
            return;
        };
        if depth > 0 {
            println!("AI calculated depth: {}", depth);
        }
        self.game.make_move(&s, &e);
        self.last_len += 1;
        out.push(
            serde_json::json!({
                "next_move": format!("{}{}", s, e),
                "time_ms": outcome.elapsed.as_millis()
            })
            .to_string(),
        );
        if let Some((ps, pe)) = outcome.ponder {
            println!("Started ponder on {}{}", ps, pe);
            let root = self.game.board.hash(self.game.current_turn);
            self.ponder = Some((root, ps, pe));
        }
    }
}

#[tokio::main]
async fn main() {
    let port = env::args().nth(1).unwrap_or_else(|| "8771".into());
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await.expect("bind");

    let mut engine = Engine::from_env(6, num_cpus::get());
    match engine.load_syzygy_from_env() {
        Ok(Some(path)) => println!("Loaded Syzygy tablebases from {}", path),
        Ok(None) => {}
        Err(err) => eprintln!("Failed to load Syzygy tablebases: {}", err),
    }
    let engine: SharedEngine = Arc::new(Mutex::new(engine));

    println!("WebSocket server on ws://{}", addr);
    println!("Supports time control: wtime, btime, winc, binc, movestogo, depth, movetime");
    while let Ok((stream, addr)) = listener.accept().await {
        println!("Client connected: {}", addr);
        tokio::spawn(handle_conn(stream, addr, engine.clone()));
    }
}

async fn handle_conn(
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
    engine: SharedEngine,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(err) => {
            eprintln!("WebSocket handshake with {} failed: {}", addr, err);
            return;
        }
    };
    let (mut write, mut read) = ws_stream.split();
    let mut conn = Conn::new(engine);
    let mut out = Vec::new();

    loop {
        tokio::select! {
            msg = read.next() => {
                let Some(Ok(msg)) = msg else { break };
                if !msg.is_text() {
                    continue;
                }
                let Ok(txt) = msg.to_text() else { continue };
                println!("Received from {}: {}", addr, txt);
                conn.on_text(txt, &mut out).await;
            }
            res = async { (&mut conn.inflight.as_mut().unwrap().handle).await },
                if conn.inflight.is_some() =>
            {
                let generation = conn.inflight.take().map_or(0, |s| s.generation);
                conn.on_search_done(generation, res, &mut out).await;
            }
        }
        for text in out.drain(..) {
            if write.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    }

    // Never leave a stale search starting a ponder, nor a ponder running
    // for a client that is gone.
    conn.invalidate();
    if let Some(search) = conn.inflight.take() {
        let _ = search.handle.await;
    }
    conn.stop_ponder().await;
    println!("Client disconnected: {}", addr);
}
