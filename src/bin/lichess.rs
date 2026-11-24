//! Lichess bot for the chessmind engine.
//!
//! Connects to a Lichess BOT account through the Bot API, accepts the
//! challenges that match its settings and plays the games with the engine,
//! using the real game clock. The account's personal API token (scope
//! `bot:play`) is read from the `LICHESS_TOKEN` environment variable.
//!
//! Usage:
//!   cargo run --release --bin lichess -- [options]       play on Lichess
//!   cargo run --release --bin lichess -- upgrade --yes   turn the account into a BOT
//!
//! Run with `--help` for the options.

use chessmind::engine::{Engine, SearchScore, TimeConfig};
use chessmind::fen::game_from_fen;
use chessmind::game::Game;
use chessmind::pieces::{Color, PieceType};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::io::{BufRead, BufReader};
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const LICHESS_URL: &str = "https://lichess.org";
const TOKEN_URL: &str =
    "https://lichess.org/account/oauth/token/create?scopes[]=bot:play&description=chessmind";
const USER_AGENT: &str = concat!("chessmind-lichess-bot/", env!("CARGO_PKG_VERSION"));

/// Bytes per transposition-table entry (key + data, two u64 atomics).
const TT_ENTRY_BYTES: usize = 16;
const MAX_DEPTH: u32 = 64;
const MAX_THREADS: usize = 256;
const MAX_HASH_MB: usize = 4096;

/// Lichess asks clients to wait a minute after an HTTP 429.
const RATE_LIMIT_PAUSE: Duration = Duration::from_secs(60);
/// The event stream sends an empty keep-alive line every 7 s, so a read that
/// waits this long means the connection is dead. A game stream that times out
/// while the opponent thinks is simply reopened.
const STREAM_READ_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// An accepted challenge that has not become a game by then frees its slot.
const ACCEPTED_CHALLENGE_TTL: Duration = Duration::from_secs(30);
/// Time per move in a game without a clock.
const NO_CLOCK_MOVE_MS: u64 = 10_000;
/// The engine always gets at least this much time, even after the overhead.
const MIN_THINK_MS: u64 = 20;
const MOVE_POST_ATTEMPTS: u32 = 3;
const GAME_STREAM_ATTEMPTS: u32 = 10;

/// Lichess speed categories, fastest first.
const SPEEDS: [&str; 6] = [
    "ultraBullet",
    "bullet",
    "blitz",
    "rapid",
    "classical",
    "correspondence",
];
/// The speeds this bot can play: Lichess bans UltraBullet for bots, and
/// correspondence games have no clock to manage.
const PLAYABLE_SPEEDS: [&str; 4] = ["bullet", "blitz", "rapid", "classical"];

const USAGE: &str = "\
Lichess bot for the chessmind engine.

USAGE:
    lichess [OPTIONS]          connect and play
    lichess upgrade [--yes]    turn the account into a BOT account (irreversible)

The personal API token is read from the LICHESS_TOKEN environment variable.
It needs the bot:play scope.

OPTIONS:
    --threads N          search threads per game (default: all logical cores)
    --hash MB            transposition table per game in MB (default: 256)
    --max-games N        games played at the same time (default: 1)
    --move-overhead MS   time kept back on each move for network lag (default: 300)
    --speeds LIST        accepted speeds, comma separated, among bullet, blitz,
                         rapid, classical (default: all four)
    --casual-only        decline rated challenges
    --opponents KIND     who may challenge the bot: all, humans or bots (default: all)
    --no-book            do not use the opening book
    --challenge USER     challenge USER once connected
    --tc MIN+INC         time control of --challenge, e.g. 3+2 or 0.5+0 (default: 3+2)
    --rated              make the --challenge rated
    -h, --help           print this help

SYZYGY_PATH enables endgame tablebases, as for the other front-ends.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opponents {
    All,
    Humans,
    Bots,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChallengeRequest {
    username: String,
    limit_s: u64,
    increment_s: u64,
    rated: bool,
}

#[derive(Clone, Debug)]
struct Config {
    threads: usize,
    hash_mb: usize,
    max_games: usize,
    move_overhead_ms: u64,
    speeds: Vec<String>,
    casual_only: bool,
    opponents: Opponents,
    use_book: bool,
    challenge: Option<ChallengeRequest>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            threads: num_cpus::get().clamp(1, MAX_THREADS),
            hash_mb: 256,
            max_games: 1,
            move_overhead_ms: 300,
            speeds: PLAYABLE_SPEEDS.iter().map(|s| s.to_string()).collect(),
            casual_only: false,
            opponents: Opponents::All,
            use_book: true,
            challenge: None,
        }
    }
}

#[derive(Debug)]
enum Command {
    Run(Config),
    Upgrade { confirmed: bool },
    Help,
}

fn parse_number<T: FromStr>(flag: &str, value: Option<&str>) -> Result<T, String> {
    let value = value.ok_or_else(|| format!("{flag} needs a value"))?;
    value
        .parse()
        .map_err(|_| format!("{flag}: '{value}' is not a valid number"))
}

fn parse_speeds(value: &str) -> Result<Vec<String>, String> {
    let mut speeds = Vec::new();
    for speed in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let Some(known) = SPEEDS.iter().find(|s| s.eq_ignore_ascii_case(speed)) else {
            return Err(format!("--speeds: unknown speed '{speed}'"));
        };
        if !PLAYABLE_SPEEDS.contains(known) {
            return Err(format!(
                "--speeds: the bot cannot play {known} (Lichess bans UltraBullet for bots, \
                 and correspondence has no clock)"
            ));
        }
        speeds.push(known.to_string());
    }
    if speeds.is_empty() {
        return Err("--speeds needs at least one speed".to_string());
    }
    Ok(speeds)
}

/// Parses `MIN+INC` (minutes, then increment seconds) into seconds, checked
/// against the clocks Lichess accepts for a challenge.
fn parse_time_control(text: &str) -> Result<(u64, u64), String> {
    let invalid = || format!("--tc: '{text}' is not of the form MIN+INC, e.g. 3+2");
    let (minutes, increment) = text.split_once('+').ok_or_else(invalid)?;
    let minutes: f64 = minutes.trim().parse().map_err(|_| invalid())?;
    let increment: u64 = increment.trim().parse().map_err(|_| invalid())?;
    if !(0.0..=180.0).contains(&minutes) {
        return Err("--tc: the base time must be between 0 and 180 minutes".to_string());
    }
    let limit = (minutes * 60.0).round() as u64;
    let limit_is_valid = matches!(limit, 0 | 15 | 30 | 45 | 90) || limit.is_multiple_of(60);
    if !limit_is_valid {
        return Err(
            "--tc: the base time must be 0, 0.25, 0.5, 0.75, 1.5 or a whole number of minutes"
                .to_string(),
        );
    }
    if increment > 60 {
        return Err("--tc: the increment must be at most 60 seconds".to_string());
    }
    if limit == 0 && increment == 0 {
        return Err("--tc: 0+0 is not a valid clock".to_string());
    }
    Ok((limit, increment))
}

fn is_valid_username(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 30
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    if args.first().map(String::as_str) == Some("upgrade") {
        return match &args[1..] {
            [] => Ok(Command::Upgrade { confirmed: false }),
            [flag] if flag == "--yes" => Ok(Command::Upgrade { confirmed: true }),
            _ => Err("usage: lichess upgrade [--yes]".to_string()),
        };
    }

    let mut cfg = Config::default();
    let mut challenge_user: Option<String> = None;
    let mut time_control = (180, 2);
    let mut rated = false;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args.get(i + 1).map(String::as_str);
        let mut takes_value = true;
        match flag {
            "-h" | "--help" => return Ok(Command::Help),
            "--threads" => {
                cfg.threads = parse_number::<usize>(flag, value)?.clamp(1, MAX_THREADS);
            }
            "--hash" => cfg.hash_mb = parse_number::<usize>(flag, value)?.clamp(1, MAX_HASH_MB),
            "--max-games" => cfg.max_games = parse_number::<usize>(flag, value)?.max(1),
            "--move-overhead" => cfg.move_overhead_ms = parse_number(flag, value)?,
            "--speeds" => {
                cfg.speeds = parse_speeds(value.ok_or("--speeds needs a value")?)?;
            }
            "--opponents" => {
                cfg.opponents = match value {
                    Some("all") => Opponents::All,
                    Some("humans") => Opponents::Humans,
                    Some("bots") => Opponents::Bots,
                    _ => return Err("--opponents must be all, humans or bots".to_string()),
                };
            }
            "--challenge" => {
                let name = value.ok_or("--challenge needs a username")?;
                if !is_valid_username(name) {
                    return Err(format!("--challenge: '{name}' is not a Lichess username"));
                }
                challenge_user = Some(name.to_string());
            }
            "--tc" => time_control = parse_time_control(value.ok_or("--tc needs a value")?)?,
            "--casual-only" => {
                cfg.casual_only = true;
                takes_value = false;
            }
            "--no-book" => {
                cfg.use_book = false;
                takes_value = false;
            }
            "--rated" => {
                rated = true;
                takes_value = false;
            }
            _ => return Err(format!("unknown option '{flag}'")),
        }
        i += if takes_value { 2 } else { 1 };
    }

    cfg.challenge = challenge_user.map(|username| ChallengeRequest {
        username,
        limit_s: time_control.0,
        increment_s: time_control.1,
        rated,
    });
    Ok(Command::Run(cfg))
}

#[derive(Debug)]
enum ApiError {
    Status(u16, String),
    Transport(String),
}

impl ApiError {
    fn is_rate_limit(&self) -> bool {
        matches!(self, ApiError::Status(429, _))
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Status(code, body) if body.trim().is_empty() => write!(f, "HTTP {code}"),
            ApiError::Status(code, body) => write!(f, "HTTP {code}: {}", body.trim()),
            ApiError::Transport(message) => write!(f, "{message}"),
        }
    }
}

impl From<ureq::Error> for ApiError {
    fn from(err: ureq::Error) -> Self {
        match err {
            ureq::Error::Status(code, response) => {
                ApiError::Status(code, response.into_string().unwrap_or_default())
            }
            ureq::Error::Transport(transport) => ApiError::Transport(transport.to_string()),
        }
    }
}

/// Minimal blocking client for the parts of the Lichess API the bot uses.
#[derive(Clone)]
struct Lichess {
    agent: ureq::Agent,
    auth: String,
}

impl Lichess {
    fn new(token: &str) -> Self {
        let agent = ureq::AgentBuilder::new()
            .user_agent(USER_AGENT)
            .timeout_connect(REQUEST_TIMEOUT)
            .timeout_read(STREAM_READ_TIMEOUT)
            .build();
        Self {
            agent,
            auth: format!("Bearer {token}"),
        }
    }

    fn get(&self, path: &str) -> Result<ureq::Response, ApiError> {
        let response = self
            .agent
            .get(&format!("{LICHESS_URL}{path}"))
            .set("Authorization", &self.auth)
            .call()?;
        Ok(response)
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let body = self
            .get(path)?
            .into_string()
            .map_err(|err| ApiError::Transport(err.to_string()))?;
        serde_json::from_str(&body)
            .map_err(|err| ApiError::Transport(format!("unexpected answer from {path}: {err}")))
    }

    /// Opens an NDJSON stream. Empty keep-alive lines are skipped.
    fn stream(
        &self,
        path: &str,
    ) -> Result<impl Iterator<Item = std::io::Result<String>> + use<>, ApiError> {
        let reader = BufReader::new(self.get(path)?.into_reader());
        Ok(reader
            .lines()
            .filter(|line| !matches!(line, Ok(text) if text.trim().is_empty())))
    }

    fn post(&self, path: &str, form: &[(&str, &str)]) -> Result<(), ApiError> {
        self.agent
            .post(&format!("{LICHESS_URL}{path}"))
            .set("Authorization", &self.auth)
            .timeout(REQUEST_TIMEOUT)
            .send_form(form)?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct Account {
    id: String,
    username: String,
    title: Option<String>,
}

impl Account {
    fn is_bot(&self) -> bool {
        self.title.as_deref() == Some("BOT")
    }
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Event {
    Challenge {
        challenge: Box<Challenge>,
    },
    ChallengeCanceled {
        challenge: ChallengeId,
    },
    ChallengeDeclined {
        challenge: ChallengeId,
    },
    GameStart {
        game: GameId,
    },
    /// Also covers `gameFinish`: the game's own thread reports the result.
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug)]
struct ChallengeId {
    id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GameId {
    game_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Challenge {
    id: String,
    challenger: Option<ChallengeUser>,
    variant: Variant,
    rated: bool,
    speed: String,
    time_control: ChallengeTimeControl,
    /// "in" for a challenge the bot received, "out" for one it sent.
    direction: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ChallengeUser {
    id: String,
    name: Option<String>,
    title: Option<String>,
    rating: Option<u32>,
}

#[derive(Deserialize, Debug)]
struct Variant {
    key: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ChallengeTimeControl {
    #[serde(rename = "type")]
    kind: String,
    limit: Option<u64>,
    increment: Option<u64>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
enum GameEvent {
    GameFull(Box<GameFull>),
    GameState(GameState),
    #[serde(rename_all = "camelCase")]
    OpponentGone {
        gone: bool,
        claim_win_in_seconds: Option<u64>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GameFull {
    variant: Variant,
    clock: Option<Clock>,
    speed: Option<String>,
    rated: Option<bool>,
    white: Player,
    black: Player,
    initial_fen: Option<String>,
    state: GameState,
}

#[derive(Deserialize, Debug)]
struct Clock {}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Player {
    id: Option<String>,
    name: Option<String>,
    title: Option<String>,
    rating: Option<u32>,
    ai_level: Option<u32>,
}

impl Player {
    fn describe(&self) -> String {
        if let Some(level) = self.ai_level {
            return format!("Lichess AI level {level}");
        }
        let name = self.name.as_deref().or(self.id.as_deref()).unwrap_or("?");
        let title = self
            .title
            .as_deref()
            .map(|t| format!("{t} "))
            .unwrap_or_default();
        match self.rating {
            Some(rating) => format!("{title}{name} ({rating})"),
            None => format!("{title}{name}"),
        }
    }
}

#[derive(Deserialize, Debug)]
struct GameState {
    moves: String,
    wtime: u64,
    btime: u64,
    winc: u64,
    binc: u64,
    status: String,
    winner: Option<String>,
}

/// Why the bot turns a challenge down, as a Lichess decline reason, or `None`
/// to accept it.
fn decline_reason(challenge: &Challenge, cfg: &Config, busy: bool) -> Option<&'static str> {
    if !matches!(challenge.variant.key.as_str(), "standard" | "fromPosition") {
        return Some("standard");
    }
    if challenge.time_control.kind != "clock" {
        return Some("timeControl");
    }
    if !cfg.speeds.contains(&challenge.speed) {
        return Some(speed_decline_reason(&challenge.speed, &cfg.speeds));
    }
    if challenge.rated && cfg.casual_only {
        return Some("casual");
    }
    let from_bot = challenge
        .challenger
        .as_ref()
        .is_some_and(|user| user.title.as_deref() == Some("BOT"));
    match cfg.opponents {
        Opponents::Humans if from_bot => return Some("noBot"),
        Opponents::Bots if !from_bot => return Some("onlyBot"),
        _ => {}
    }
    if busy {
        return Some("later");
    }
    None
}

fn speed_decline_reason(speed: &str, accepted: &[String]) -> &'static str {
    let rank_of = |name: &str| SPEEDS.iter().position(|s| *s == name);
    let Some(rank) = rank_of(speed) else {
        return "timeControl";
    };
    let accepted_ranks: Vec<usize> = accepted.iter().filter_map(|s| rank_of(s)).collect();
    let fastest = accepted_ranks.iter().min().copied().unwrap_or(0);
    let slowest = accepted_ranks.iter().max().copied().unwrap_or(SPEEDS.len());
    if rank < fastest {
        "tooFast"
    } else if rank > slowest {
        "tooSlow"
    } else {
        "timeControl"
    }
}

/// Plays a move given in UCI notation. Castling is accepted both as the king's
/// two-square step (`e1g1`) and as the king taking its own rook (`e1h1`).
fn apply_uci_move(game: &mut Game, mv: &str) -> bool {
    if !(4..=5).contains(&mv.len()) || !mv.is_ascii() {
        return false;
    }
    let (from, to) = mv.split_at(2);
    if game.make_move(from, to) {
        return true;
    }
    let castled_to = match (from, to) {
        ("e1", "h1") => "g1",
        ("e1", "a1") => "c1",
        ("e8", "h8") => "g8",
        ("e8", "a8") => "c8",
        _ => return false,
    };
    let king_on_start = game.board.get(from).is_some_and(|piece| {
        piece.piece_type == PieceType::King && piece.color == game.current_turn
    });
    king_on_start && game.make_move(from, castled_to)
}

/// Replays the game from its start position (`None` for the standard one).
fn build_game(initial_fen: Option<&str>, moves: &str) -> Result<Game, String> {
    let mut game = match initial_fen {
        None => Game::new(),
        Some(fen) => game_from_fen(fen).ok_or_else(|| format!("invalid FEN '{fen}'"))?,
    };
    for mv in moves.split_whitespace() {
        if !apply_uci_move(&mut game, mv) {
            return Err(format!("cannot play move '{mv}'"));
        }
    }
    Ok(game)
}

/// Search clock for the side to move, keeping `overhead_ms` back for the time
/// the move spends on the network.
fn clock_config(state: &GameState, color: Color, overhead_ms: u64) -> TimeConfig {
    let keep_overhead = |ms: u64| ms.saturating_sub(overhead_ms).max(MIN_THINK_MS);
    let (wtime, btime) = match color {
        Color::White => (keep_overhead(state.wtime), state.btime),
        Color::Black => (state.wtime, keep_overhead(state.btime)),
    };
    TimeConfig {
        wtime: Some(wtime),
        btime: Some(btime),
        winc: Some(state.winc),
        binc: Some(state.binc),
        ..TimeConfig::new()
    }
}

fn is_running(status: &str) -> bool {
    matches!(status, "created" | "started")
}

fn describe_result(state: &GameState, color: Color) -> String {
    let our_side = match color {
        Color::White => "white",
        Color::Black => "black",
    };
    let verdict = match state.winner.as_deref() {
        Some(winner) if winner == our_side => "won",
        Some(_) => "lost",
        None if matches!(state.status.as_str(), "aborted" | "noStart") => "aborted",
        None => "draw",
    };
    format!("{verdict} ({})", state.status)
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "White",
        Color::Black => "Black",
    }
}

/// One game the bot is playing.
struct Session {
    id: String,
    color: Color,
    /// `None` for the standard start position.
    initial_fen: Option<String>,
    has_clock: bool,
    engine: Engine,
    /// Number of moves on the board when the bot last moved, so that a state
    /// sent twice (draw offer, reconnection) is not answered twice.
    answered_ply: Option<usize>,
}

/// Game slots: games in progress and accepted challenges not started yet.
#[derive(Default)]
struct Slots {
    playing: HashSet<String>,
    accepted: HashMap<String, Instant>,
}

impl Slots {
    fn is_full(&mut self, max_games: usize) -> bool {
        self.accepted
            .retain(|_, accepted_at| accepted_at.elapsed() < ACCEPTED_CHALLENGE_TTL);
        self.playing.len() + self.accepted.len() >= max_games
    }
}

struct Bot {
    api: Lichess,
    cfg: Config,
    /// Lowercase Lichess id of the bot account.
    me: String,
    slots: Mutex<Slots>,
}

impl Bot {
    /// Listens to the account's event stream forever. Returns only when the
    /// token is rejected.
    fn run(self: &Arc<Self>) -> Result<(), ApiError> {
        let mut pending_challenge = self.cfg.challenge.clone();
        let mut retry_delay = Duration::from_secs(1);
        loop {
            match self.api.stream("/api/stream/event") {
                Ok(lines) => {
                    println!("Connected, waiting for challenges.");
                    retry_delay = Duration::from_secs(1);
                    if let Some(request) = pending_challenge.take() {
                        self.send_challenge(&request);
                    }
                    for line in lines {
                        match line {
                            Ok(line) => self.handle_event_line(&line),
                            Err(err) => {
                                eprintln!("Event stream interrupted: {err}");
                                break;
                            }
                        }
                    }
                    eprintln!(
                        "Event stream closed (another bot instance with the same token \
                         closes it too), reconnecting."
                    );
                }
                Err(err @ ApiError::Status(401, _)) => return Err(err),
                Err(err) if err.is_rate_limit() => {
                    eprintln!("Rate limited by Lichess, pausing for a minute.");
                    thread::sleep(RATE_LIMIT_PAUSE);
                    continue;
                }
                Err(err) => eprintln!("Cannot open the event stream: {err}"),
            }
            thread::sleep(retry_delay);
            retry_delay = (retry_delay * 2).min(Duration::from_secs(60));
        }
    }

    fn handle_event_line(self: &Arc<Self>, line: &str) {
        let event = match serde_json::from_str::<Event>(line) {
            Ok(event) => event,
            Err(err) => {
                eprintln!("Ignoring an event that could not be read ({err}): {line}");
                return;
            }
        };
        match event {
            Event::Challenge { challenge } => self.on_challenge(*challenge),
            Event::GameStart { game } => self.on_game_start(game.game_id),
            Event::ChallengeCanceled { challenge } => {
                self.slots.lock().unwrap().accepted.remove(&challenge.id);
                println!("Challenge {} was canceled", challenge.id);
            }
            Event::ChallengeDeclined { challenge } => {
                println!("Challenge {} was declined", challenge.id);
            }
            Event::Other => {}
        }
    }

    fn on_challenge(&self, challenge: Challenge) {
        let challenger = challenge.challenger.as_ref();
        let sent_by_us = challenge.direction.as_deref() == Some("out")
            || challenger.is_some_and(|user| user.id == self.me);
        if sent_by_us {
            return;
        }

        let who = challenger
            .map(|user| {
                let name = user.name.as_deref().unwrap_or(&user.id);
                let title = user
                    .title
                    .as_deref()
                    .map(|t| format!("{t} "))
                    .unwrap_or_default();
                match user.rating {
                    Some(rating) => format!("{title}{name} ({rating})"),
                    None => format!("{title}{name}"),
                }
            })
            .unwrap_or_else(|| "an anonymous player".to_string());
        let clock = match (
            challenge.time_control.limit,
            challenge.time_control.increment,
        ) {
            (Some(limit), Some(increment)) => format!("{}+{increment}", limit as f64 / 60.0),
            _ => challenge.time_control.kind.clone(),
        };
        let kind = if challenge.rated { "rated" } else { "casual" };
        let summary = format!(
            "{kind} {} {clock}, {}",
            challenge.speed, challenge.variant.key
        );

        let reason = {
            let mut slots = self.slots.lock().unwrap();
            let busy = slots.is_full(self.cfg.max_games);
            let reason = decline_reason(&challenge, &self.cfg, busy);
            if reason.is_none() {
                slots.accepted.insert(challenge.id.clone(), Instant::now());
            }
            reason
        };

        match reason {
            None => {
                println!("Accepting {summary} from {who}");
                let path = format!("/api/challenge/{}/accept", challenge.id);
                if let Err(err) = self.api.post(&path, &[]) {
                    eprintln!("Could not accept challenge {}: {err}", challenge.id);
                    self.slots.lock().unwrap().accepted.remove(&challenge.id);
                }
            }
            Some(reason) => {
                println!("Declining {summary} from {who}: {reason}");
                let path = format!("/api/challenge/{}/decline", challenge.id);
                if let Err(err) = self.api.post(&path, &[("reason", reason)]) {
                    eprintln!("Could not decline challenge {}: {err}", challenge.id);
                }
            }
        }
    }

    fn send_challenge(&self, request: &ChallengeRequest) {
        let limit = request.limit_s.to_string();
        let increment = request.increment_s.to_string();
        let form = [
            ("rated", if request.rated { "true" } else { "false" }),
            ("clock.limit", limit.as_str()),
            ("clock.increment", increment.as_str()),
            ("color", "random"),
            ("variant", "standard"),
        ];
        let path = format!("/api/challenge/{}", request.username);
        match self.api.post(&path, &form) {
            Ok(()) => println!(
                "Challenged {} ({}+{}); the challenge expires after 20 s if not accepted.",
                request.username,
                request.limit_s as f64 / 60.0,
                request.increment_s
            ),
            Err(err) => eprintln!("Could not challenge {}: {err}", request.username),
        }
    }

    fn on_game_start(self: &Arc<Self>, game_id: String) {
        {
            let mut slots = self.slots.lock().unwrap();
            slots.accepted.remove(&game_id);
            if !slots.playing.insert(game_id.clone()) {
                return;
            }
        }
        let bot = Arc::clone(self);
        thread::spawn(move || {
            bot.play_game(&game_id);
            bot.slots.lock().unwrap().playing.remove(&game_id);
        });
    }

    fn play_game(&self, game_id: &str) {
        println!("[{game_id}] started: {LICHESS_URL}/{game_id}");
        let mut session: Option<Session> = None;
        let mut failures = 0;
        loop {
            let path = format!("/api/bot/game/stream/{game_id}");
            let lines = match self.api.stream(&path) {
                Ok(lines) => lines,
                Err(ApiError::Status(404, _)) => {
                    eprintln!("[{game_id}] game not found");
                    return;
                }
                Err(err) => {
                    failures += 1;
                    eprintln!("[{game_id}] cannot open the game stream: {err}");
                    if failures >= GAME_STREAM_ATTEMPTS {
                        eprintln!("[{game_id}] giving up on this game");
                        return;
                    }
                    let pause = if err.is_rate_limit() {
                        RATE_LIMIT_PAUSE
                    } else {
                        Duration::from_secs(2)
                    };
                    thread::sleep(pause);
                    continue;
                }
            };
            failures = 0;

            for line in lines {
                let line = match line {
                    Ok(line) => line,
                    Err(err) => {
                        // Usually a read timeout while the opponent thinks.
                        if !matches!(
                            err.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) {
                            eprintln!("[{game_id}] game stream interrupted: {err}");
                        }
                        break;
                    }
                };
                let event = match serde_json::from_str::<GameEvent>(&line) {
                    Ok(event) => event,
                    Err(err) => {
                        eprintln!("[{game_id}] ignoring a line that could not be read ({err})");
                        continue;
                    }
                };
                let state = match event {
                    GameEvent::GameFull(full) => {
                        if session.is_none() {
                            match self.new_session(game_id, &full) {
                                Ok(new_session) => session = Some(new_session),
                                Err(reason) => {
                                    eprintln!("[{game_id}] cannot play this game: {reason}");
                                    self.leave_game(game_id);
                                    return;
                                }
                            }
                        }
                        full.state
                    }
                    GameEvent::GameState(state) => state,
                    GameEvent::OpponentGone {
                        gone,
                        claim_win_in_seconds,
                    } => {
                        self.on_opponent_gone(game_id, gone, claim_win_in_seconds);
                        continue;
                    }
                    GameEvent::Other => continue,
                };
                let Some(session) = session.as_mut() else {
                    continue;
                };
                if !is_running(&state.status) {
                    println!(
                        "[{game_id}] game over: {}",
                        describe_result(&state, session.color)
                    );
                    return;
                }
                self.answer(session, &state);
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    fn new_session(&self, game_id: &str, full: &GameFull) -> Result<Session, String> {
        let is_me = |player: &Player| player.id.as_deref() == Some(self.me.as_str());
        let (color, opponent) = if is_me(&full.white) {
            (Color::White, &full.black)
        } else if is_me(&full.black) {
            (Color::Black, &full.white)
        } else {
            return Err("the bot is not one of the players".to_string());
        };
        if !matches!(full.variant.key.as_str(), "standard" | "fromPosition") {
            return Err(format!("unsupported variant '{}'", full.variant.key));
        }
        let initial_fen = match full.initial_fen.as_deref() {
            None | Some("startpos") => None,
            Some(fen) => {
                game_from_fen(fen).ok_or_else(|| format!("unsupported start position '{fen}'"))?;
                Some(fen.to_string())
            }
        };

        let entries = (self.cfg.hash_mb * 1024 * 1024) / TT_ENTRY_BYTES;
        let mut engine = Engine::with_threads_and_table(MAX_DEPTH, self.cfg.threads, entries);
        if let Err(err) = engine.load_syzygy_from_env() {
            eprintln!("[{game_id}] could not load the Syzygy tablebases: {err}");
        }

        let rated = if full.rated.unwrap_or(false) {
            "rated"
        } else {
            "casual"
        };
        let speed = full.speed.as_deref().unwrap_or("");
        println!(
            "[{game_id}] playing {} against {}, {rated} {speed}",
            color_name(color),
            opponent.describe()
        );
        Ok(Session {
            id: game_id.to_string(),
            color,
            initial_fen,
            has_clock: full.clock.is_some(),
            engine,
            answered_ply: None,
        })
    }

    /// Searches and plays a move if it is the bot's turn in `state`.
    fn answer(&self, session: &mut Session, state: &GameState) {
        let mut game = match build_game(session.initial_fen.as_deref(), &state.moves) {
            Ok(game) => game,
            Err(err) => {
                eprintln!("[{}] cannot follow the game: {err}", session.id);
                return;
            }
        };
        let ply = state.moves.split_whitespace().count();
        if game.current_turn != session.color || session.answered_ply == Some(ply) {
            return;
        }
        let legal = game.legal_moves();
        if legal.is_empty() {
            return;
        }

        let config = if session.has_clock {
            clock_config(state, session.color, self.cfg.move_overhead_ms)
        } else {
            TimeConfig::fixed_time(NO_CLOCK_MOVE_MS)
        };
        let allow_book = self.cfg.use_book && session.initial_fen.is_none();
        let start = Instant::now();
        let result = session.engine.best_move_timed_opts(
            &mut game,
            &config,
            allow_book,
            Arc::new(AtomicBool::new(false)),
        );
        let elapsed_ms = start.elapsed().as_millis();

        let (uci, depth) = match result {
            Some(((from, to), depth)) if legal.iter().any(|(f, t)| *f == from && *t == to) => {
                (format!("{from}{to}"), Some(depth))
            }
            _ => {
                let (from, to) = &legal[0];
                (format!("{from}{to}"), None)
            }
        };

        let mut details = format!("{elapsed_ms} ms");
        if let Some(depth) = depth {
            details.push_str(&format!(", depth {depth}"));
        }
        match session.engine.last_search_score() {
            Some(SearchScore::Centipawns(cp)) => details.push_str(&format!(", {cp:+} cp")),
            Some(SearchScore::Mate(n)) => details.push_str(&format!(", mate {n}")),
            None => {}
        }
        if let Some(percent) = session.engine.last_search_win_percent() {
            details.push_str(&format!(", win {percent:.0}%"));
        }

        // On a network failure the ply stays unanswered, so the state that
        // follows the reconnection triggers a new attempt.
        let path = format!("/api/bot/game/{}/move/{uci}", session.id);
        for attempt in 1..=MOVE_POST_ATTEMPTS {
            match self.api.post(&path, &[]) {
                Ok(()) => {
                    println!("[{}] played {uci} ({details})", session.id);
                    session.answered_ply = Some(ply);
                    return;
                }
                // Lichess refused the move: the game moved on in the meantime.
                Err(err @ ApiError::Status(400, _)) => {
                    eprintln!("[{}] move {uci} refused: {err}", session.id);
                    session.answered_ply = Some(ply);
                    return;
                }
                Err(err) => {
                    eprintln!(
                        "[{}] could not send {uci} (attempt {attempt}): {err}",
                        session.id
                    );
                    let pause = if err.is_rate_limit() {
                        RATE_LIMIT_PAUSE
                    } else {
                        Duration::from_millis(200)
                    };
                    thread::sleep(pause);
                }
            }
        }
    }

    fn on_opponent_gone(&self, game_id: &str, gone: bool, claim_in_seconds: Option<u64>) {
        if !gone {
            println!("[{game_id}] the opponent is back");
            return;
        }
        let Some(seconds) = claim_in_seconds else {
            println!("[{game_id}] the opponent left the game");
            return;
        };
        println!(
            "[{game_id}] the opponent left, claiming the win in {seconds} s if they stay away"
        );
        let api = self.api.clone();
        let path = format!("/api/bot/game/{game_id}/claim-victory");
        let game_id = game_id.to_string();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(seconds + 1));
            if api.post(&path, &[]).is_ok() {
                println!("[{game_id}] claimed the win");
            }
        });
    }

    /// Leaves a game the bot cannot play: abort when still possible, else resign.
    fn leave_game(&self, game_id: &str) {
        if self
            .api
            .post(&format!("/api/bot/game/{game_id}/abort"), &[])
            .is_ok()
        {
            println!("[{game_id}] aborted");
        } else if self
            .api
            .post(&format!("/api/bot/game/{game_id}/resign"), &[])
            .is_ok()
        {
            println!("[{game_id}] resigned");
        }
    }
}

fn upgrade(api: &Lichess, account: &Account, confirmed: bool) -> ExitCode {
    if account.is_bot() {
        println!("{} is already a BOT account.", account.username);
        return ExitCode::SUCCESS;
    }
    if !confirmed {
        println!(
            "This turns the Lichess account '{}' into a BOT account.\n\
             It only works on an account that has never played a game, and it cannot\n\
             be undone: the account will only be able to play through the Bot API.\n\
             Run `lichess upgrade --yes` to go ahead.",
            account.username
        );
        return ExitCode::SUCCESS;
    }
    match api.post("/api/bot/account/upgrade", &[]) {
        Ok(()) => {
            println!("{} is now a BOT account.", account.username);
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Upgrade refused: {err}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let command = match parse_args(&args) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(command) => command,
        Err(err) => {
            eprintln!("{err}\n\nRun with --help for the options.");
            return ExitCode::from(2);
        }
    };

    let token = env::var("LICHESS_TOKEN")
        .map(|token| token.trim().to_string())
        .unwrap_or_default();
    if token.is_empty() {
        eprintln!(
            "LICHESS_TOKEN is not set.\n\
             Create a personal API token with the bot:play scope, logged in as the bot account:\n\
             {TOKEN_URL}\n\
             then set it, e.g. in PowerShell: $env:LICHESS_TOKEN = \"lip_...\""
        );
        return ExitCode::from(2);
    }

    let api = Lichess::new(&token);
    let account: Account = match api.get_json("/api/account") {
        Ok(account) => account,
        Err(ApiError::Status(401, _)) => {
            eprintln!("Lichess rejected the token (HTTP 401). Check LICHESS_TOKEN.");
            return ExitCode::FAILURE;
        }
        Err(err) => {
            eprintln!("Cannot reach Lichess: {err}");
            return ExitCode::FAILURE;
        }
    };

    let cfg = match command {
        Command::Upgrade { confirmed } => return upgrade(&api, &account, confirmed),
        Command::Run(cfg) => cfg,
        Command::Help => unreachable!("handled above"),
    };
    if !account.is_bot() {
        eprintln!(
            "{} is not a BOT account. Turn it into one with `lichess upgrade` \
             (irreversible, only for an account that has never played a game).",
            account.username
        );
        return ExitCode::FAILURE;
    }

    println!(
        "Logged in as BOT {}: {} thread(s), {} MB hash, up to {} game(s), speeds {}, \
         move overhead {} ms",
        account.username,
        cfg.threads,
        cfg.hash_mb,
        cfg.max_games,
        cfg.speeds.join(","),
        cfg.move_overhead_ms
    );
    println!("Profile: {LICHESS_URL}/@/{}", account.username);
    let bot = Arc::new(Bot {
        api,
        cfg,
        me: account.id.to_ascii_lowercase(),
        slots: Mutex::new(Slots::default()),
    });
    match bot.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Lichess rejected the token: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn challenge_json(variant: &str, speed: &str, tc: &str, rated: bool, title: &str) -> String {
        format!(
            r#"{{"type":"challenge","challenge":{{"id":"Abc12345","url":"https://lichess.org/Abc12345",
            "status":"created","challenger":{{"id":"someone","name":"Someone","rating":1850{title}}},
            "destUser":{{"id":"chessmind","name":"chessmind","title":"BOT"}},
            "variant":{{"key":"{variant}","name":"x","short":"x"}},"rated":{rated},"speed":"{speed}",
            "timeControl":{tc},"color":"random","finalColor":"white","perf":{{"icon":"x","name":"x"}},
            "direction":"in"}},"compat":{{"bot":true,"board":true}}}}"#
        )
    }

    fn parse_challenge(json: &str) -> Challenge {
        match serde_json::from_str::<Event>(json).unwrap() {
            Event::Challenge { challenge } => *challenge,
            other => panic!("expected a challenge, got {other:?}"),
        }
    }

    const BLITZ: &str = r#"{"type":"clock","limit":180,"increment":2,"show":"3+2"}"#;

    #[test]
    fn accepts_a_standard_blitz_challenge() {
        let challenge = parse_challenge(&challenge_json("standard", "blitz", BLITZ, true, ""));
        assert_eq!(challenge.id, "Abc12345");
        assert_eq!(challenge.challenger.as_ref().unwrap().id, "someone");
        assert_eq!(decline_reason(&challenge, &Config::default(), false), None);
        assert_eq!(
            decline_reason(&challenge, &Config::default(), true),
            Some("later")
        );
    }

    #[test]
    fn declines_with_the_matching_reason() {
        let cfg = Config::default();
        let chess960 = parse_challenge(&challenge_json("chess960", "blitz", BLITZ, false, ""));
        assert_eq!(decline_reason(&chess960, &cfg, false), Some("standard"));

        let corres = r#"{"type":"correspondence","daysPerTurn":3}"#;
        let corres = parse_challenge(&challenge_json(
            "standard",
            "correspondence",
            corres,
            false,
            "",
        ));
        assert_eq!(decline_reason(&corres, &cfg, false), Some("timeControl"));

        let ultra = r#"{"type":"clock","limit":15,"increment":0,"show":"1/4+0"}"#;
        let ultra = parse_challenge(&challenge_json("standard", "ultraBullet", ultra, false, ""));
        assert_eq!(decline_reason(&ultra, &cfg, false), Some("tooFast"));

        let fast_only = Config {
            speeds: parse_speeds("bullet,blitz").unwrap(),
            ..Config::default()
        };
        let classical = r#"{"type":"clock","limit":1800,"increment":0,"show":"30+0"}"#;
        let classical = parse_challenge(&challenge_json(
            "standard",
            "classical",
            classical,
            false,
            "",
        ));
        assert_eq!(
            decline_reason(&classical, &fast_only, false),
            Some("tooSlow")
        );

        let rated = parse_challenge(&challenge_json("standard", "blitz", BLITZ, true, ""));
        let casual_only = Config {
            casual_only: true,
            ..Config::default()
        };
        assert_eq!(decline_reason(&rated, &casual_only, false), Some("casual"));

        let from_bot = parse_challenge(&challenge_json(
            "standard",
            "blitz",
            BLITZ,
            false,
            r#","title":"BOT""#,
        ));
        let humans = Config {
            opponents: Opponents::Humans,
            ..Config::default()
        };
        assert_eq!(decline_reason(&from_bot, &humans, false), Some("noBot"));
        let bots = Config {
            opponents: Opponents::Bots,
            ..Config::default()
        };
        assert_eq!(decline_reason(&from_bot, &bots, false), None);
        assert_eq!(decline_reason(&rated, &bots, false), Some("onlyBot"));
    }

    #[test]
    fn reads_the_other_events() {
        let start = r#"{"type":"gameStart","game":{"gameId":"Abc12345","fullId":"Abc12345wxyz",
            "color":"white","fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "isMyTurn":true,"opponent":{"id":"someone","username":"Someone","rating":1850}}}"#;
        assert!(matches!(
            serde_json::from_str::<Event>(start).unwrap(),
            Event::GameStart { game } if game.game_id == "Abc12345"
        ));
        let canceled = r#"{"type":"challengeCanceled","challenge":{"id":"Abc12345"}}"#;
        assert!(matches!(
            serde_json::from_str::<Event>(canceled).unwrap(),
            Event::ChallengeCanceled { .. }
        ));
        let unknown = r#"{"type":"somethingNew","data":1}"#;
        assert!(matches!(
            serde_json::from_str::<Event>(unknown).unwrap(),
            Event::Other
        ));
    }

    #[test]
    fn reads_the_game_stream() {
        let full = r#"{"type":"gameFull","id":"Abc12345","rated":false,
            "variant":{"key":"standard","name":"Standard","short":"Std"},
            "clock":{"initial":180000,"increment":2000},"speed":"blitz","perf":{"name":"Blitz"},
            "createdAt":1700000000000,"white":{"id":"chessmind","name":"chessmind","title":"BOT"},
            "black":{"aiLevel":3},"initialFen":"startpos",
            "state":{"type":"gameState","moves":"e2e4 c7c5","wtime":178000,"btime":179500,
            "winc":2000,"binc":2000,"status":"started"}}"#;
        let GameEvent::GameFull(full) = serde_json::from_str::<GameEvent>(full).unwrap() else {
            panic!("expected gameFull");
        };
        assert_eq!(full.white.id.as_deref(), Some("chessmind"));
        assert_eq!(full.black.describe(), "Lichess AI level 3");
        assert!(full.clock.is_some());
        assert_eq!(full.state.moves, "e2e4 c7c5");

        let state = r#"{"type":"gameState","moves":"e2e4 c7c5 g1f3","wtime":170000,
            "btime":175000,"winc":2000,"binc":2000,"status":"resign","winner":"white"}"#;
        let GameEvent::GameState(state) = serde_json::from_str::<GameEvent>(state).unwrap() else {
            panic!("expected gameState");
        };
        assert!(!is_running(&state.status));
        assert_eq!(describe_result(&state, Color::White), "won (resign)");
        assert_eq!(describe_result(&state, Color::Black), "lost (resign)");

        let gone = r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":8}"#;
        assert!(matches!(
            serde_json::from_str::<GameEvent>(gone).unwrap(),
            GameEvent::OpponentGone {
                gone: true,
                claim_win_in_seconds: Some(8)
            }
        ));
        let chat = r#"{"type":"chatLine","room":"player","username":"x","text":"hi"}"#;
        assert!(matches!(
            serde_json::from_str::<GameEvent>(chat).unwrap(),
            GameEvent::Other
        ));
    }

    #[test]
    fn replays_both_castling_notations() {
        let opening = "e2e4 e7e5 g1f3 b8c6 f1c4 g8f6";
        for castle in ["e1g1", "e1h1"] {
            let game = build_game(None, &format!("{opening} {castle}")).unwrap();
            let king = game.board.get("g1").unwrap();
            let rook = game.board.get("f1").unwrap();
            assert_eq!(king.piece_type, PieceType::King);
            assert_eq!(rook.piece_type, PieceType::Rook);
            assert_eq!(game.current_turn, Color::Black);
        }
        assert!(build_game(None, "e2e5").is_err());
    }

    #[test]
    fn replays_from_a_fen_with_a_promotion() {
        let fen = "8/4P3/8/8/8/8/k7/4K3 w - - 0 1";
        let game = build_game(Some(fen), "e7e8n").unwrap();
        assert_eq!(game.board.get("e8").unwrap().piece_type, PieceType::Knight);
        assert_eq!(game.current_turn, Color::Black);
    }

    #[test]
    fn keeps_the_overhead_off_our_clock_only() {
        let state = GameState {
            moves: String::new(),
            wtime: 60_000,
            btime: 50_000,
            winc: 1_000,
            binc: 1_000,
            status: "started".to_string(),
            winner: None,
        };
        let white = clock_config(&state, Color::White, 300);
        assert_eq!((white.wtime, white.btime), (Some(59_700), Some(50_000)));
        let black = clock_config(&state, Color::Black, 300);
        assert_eq!((black.wtime, black.btime), (Some(60_000), Some(49_700)));
        let low = GameState {
            btime: 100,
            ..state
        };
        assert_eq!(
            clock_config(&low, Color::Black, 300).btime,
            Some(MIN_THINK_MS)
        );
    }

    #[test]
    fn parses_time_controls() {
        assert_eq!(parse_time_control("3+2"), Ok((180, 2)));
        assert_eq!(parse_time_control("0.5+0"), Ok((30, 0)));
        assert_eq!(parse_time_control("1.5+1"), Ok((90, 1)));
        assert_eq!(parse_time_control("0+1"), Ok((0, 1)));
        assert!(parse_time_control("2.5+0").is_err());
        assert!(parse_time_control("0+0").is_err());
        assert!(parse_time_control("3+61").is_err());
        assert!(parse_time_control("3").is_err());
    }

    #[test]
    fn parses_the_command_line() {
        let Command::Run(cfg) = parse_args(&[]).unwrap() else {
            panic!("expected run");
        };
        assert_eq!(cfg.max_games, 1);
        assert!(cfg.use_book);
        assert_eq!(cfg.speeds, ["bullet", "blitz", "rapid", "classical"]);

        let Command::Run(cfg) = parse_args(&args(&[
            "--threads",
            "4",
            "--speeds",
            "blitz,rapid",
            "--casual-only",
            "--challenge",
            "maia1",
            "--tc",
            "5+3",
            "--rated",
            "--no-book",
        ]))
        .unwrap() else {
            panic!("expected run");
        };
        assert_eq!(cfg.threads, 4);
        assert_eq!(cfg.speeds, ["blitz", "rapid"]);
        assert!(cfg.casual_only && !cfg.use_book);
        assert_eq!(
            cfg.challenge,
            Some(ChallengeRequest {
                username: "maia1".to_string(),
                limit_s: 300,
                increment_s: 3,
                rated: true,
            })
        );

        assert!(matches!(
            parse_args(&args(&["upgrade", "--yes"])).unwrap(),
            Command::Upgrade { confirmed: true }
        ));
        assert!(parse_args(&args(&["--speeds", "ultraBullet"])).is_err());
        assert!(parse_args(&args(&["--challenge", "../x"])).is_err());
        assert!(parse_args(&args(&["--bogus"])).is_err());
    }
}
