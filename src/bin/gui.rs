use chessmind::{
    board::Board,
    engine::{Engine, SearchScore, TimeConfig},
    game::{DrawReason, Game, Outcome},
    pieces::{Color, Piece, PieceType},
};
use eframe::{App, Frame, egui};
use egui::{Align2, Color32, FontFamily, FontId, Pos2, Rect, RichText, Rounding, Stroke, vec2};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

/// Repaint cadence while a clock is ticking (tenths are shown under 10 s).
const CLOCK_REPAINT: Duration = Duration::from_millis(100);

/// Search depth when no clock is used.
const UNLIMITED_DEPTH: u32 = 8;

const PIECE_FONT: &str = "pieces";

/// Fonts with good chess glyphs, tried in order. egui's bundled fonts are the fallback.
const PIECE_FONT_PATHS: &[&str] = &[
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "C:\\Windows\\Fonts\\seguisym.ttf",
];

mod palette {
    use super::Color32;

    pub const BG: Color32 = Color32::from_rgb(244, 245, 241);
    /// Backdrop behind the board: deep green-grey so the board stands out.
    pub const STAGE: Color32 = Color32::from_rgb(36, 43, 39);
    pub const STAGE_EDGE: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 22);
    pub const PANEL: Color32 = Color32::from_rgb(255, 255, 255);
    pub const CARD: Color32 = Color32::from_rgb(244, 245, 241);
    pub const CARD_HOVER: Color32 = Color32::from_rgb(234, 237, 230);
    pub const ACCENT_SOFT: Color32 = Color32::from_rgb(232, 243, 234);
    pub const BORDER: Color32 = Color32::from_rgb(227, 230, 223);
    pub const TEXT: Color32 = Color32::from_rgb(30, 35, 32);
    pub const MUTED: Color32 = Color32::from_rgb(124, 132, 126);
    pub const ACCENT: Color32 = Color32::from_rgb(63, 143, 86);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(74, 161, 99);
    pub const DANGER: Color32 = Color32::from_rgb(210, 69, 63);

    pub const LIGHT_SQ: Color32 = Color32::from_rgb(238, 240, 229);
    pub const DARK_SQ: Color32 = Color32::from_rgb(115, 153, 100);
    pub const LAST_MOVE: Color32 = Color32::from_rgba_premultiplied(84, 80, 24, 84);
    pub const SELECTED: Color32 = Color32::from_rgba_premultiplied(118, 112, 30, 118);
    pub const HINT: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 46);
    pub const CHECK: Color32 = Color32::from_rgba_premultiplied(176, 40, 40, 180);

    pub const WHITE_PIECE: Color32 = Color32::from_rgb(252, 252, 250);
    pub const BLACK_PIECE: Color32 = Color32::from_rgb(34, 37, 35);
    pub const PIECE_SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 50);
}

/// Result of a background AI search; the engine travels back with it.
struct SearchDone {
    engine: Engine,
    best: Option<((String, String), u32)>,
    elapsed: Duration,
    generation: u64,
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

fn opposite(color: Color) -> Color {
    match color {
        Color::White => Color::Black,
        Color::Black => Color::White,
    }
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "White",
        Color::Black => "Black",
    }
}

fn square_name(sq: (usize, usize)) -> String {
    Board::index_to_algebraic(sq.0, sq.1).unwrap_or_default()
}

fn parse_square(s: &str) -> Option<(usize, usize)> {
    Board::algebraic_to_index(s.get(..2)?)
}

fn promotion_piece(c: char) -> Option<PieceType> {
    match c.to_ascii_lowercase() {
        'q' => Some(PieceType::Queen),
        'r' => Some(PieceType::Rook),
        'b' => Some(PieceType::Bishop),
        'n' => Some(PieceType::Knight),
        _ => None,
    }
}

fn piece_letter(pt: PieceType) -> char {
    match pt {
        PieceType::King => 'K',
        PieceType::Queen => 'Q',
        PieceType::Rook => 'R',
        PieceType::Bishop => 'B',
        PieceType::Knight => 'N',
        PieceType::Pawn => 'P',
    }
}

/// SAN of a move without the check suffix, computed before the move is played.
fn san_without_suffix(board: &Board, legal: &[(String, String)], from: &str, to: &str) -> String {
    let (Some((fx, fy)), Some((tx, ty))) = (parse_square(from), parse_square(to)) else {
        return format!("{from}{to}");
    };
    let Some(piece) = board.get_index(fx, fy) else {
        return format!("{from}{to}");
    };
    let dest = &to[..2];
    let file = |x: usize| (b'a' + x as u8) as char;

    match piece.piece_type {
        PieceType::King if fx.abs_diff(tx) == 2 => {
            if tx > fx {
                "O-O".into()
            } else {
                "O-O-O".into()
            }
        }
        PieceType::Pawn => {
            let mut san = String::new();
            if fx != tx {
                san.push(file(fx));
                san.push('x');
            }
            san.push_str(dest);
            if ty == 0 || ty == 7 {
                let promo = to
                    .chars()
                    .nth(2)
                    .and_then(promotion_piece)
                    .unwrap_or(PieceType::Queen);
                san.push('=');
                san.push(piece_letter(promo));
            }
            san
        }
        pt => {
            let mut san = String::from(piece_letter(pt));
            let rivals: Vec<(usize, usize)> = legal
                .iter()
                .filter(|(s, e)| s != from && e.get(..2) == Some(dest))
                .filter_map(|(s, _)| parse_square(s))
                .filter(|&(x, y)| matches!(board.get_index(x, y), Some(p) if p.piece_type == pt))
                .collect();
            if !rivals.is_empty() {
                if rivals.iter().all(|&(x, _)| x != fx) {
                    san.push(file(fx));
                } else if rivals.iter().all(|&(_, y)| y != fy) {
                    san.push_str(&(fy + 1).to_string());
                } else {
                    san.push_str(from);
                }
            }
            if board.get_index(tx, ty).is_some() {
                san.push('x');
            }
            san.push_str(dest);
            san
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TimePreset {
    Bullet1,   // 1+0
    Bullet2,   // 2+1
    Blitz3,    // 3+0
    Blitz3Inc, // 3+2
    Blitz5,    // 5+0
    Blitz5Inc, // 5+3
    Rapid10,   // 10+0
    Rapid15,   // 15+10
    Classical, // 30+0
    Custom,    // User-defined
    Unlimited, // No clock (fixed depth)
}

impl TimePreset {
    const ALL: [TimePreset; 11] = [
        TimePreset::Bullet1,
        TimePreset::Bullet2,
        TimePreset::Blitz3,
        TimePreset::Blitz3Inc,
        TimePreset::Blitz5,
        TimePreset::Blitz5Inc,
        TimePreset::Rapid10,
        TimePreset::Rapid15,
        TimePreset::Classical,
        TimePreset::Custom,
        TimePreset::Unlimited,
    ];

    fn name(&self) -> &'static str {
        match self {
            TimePreset::Bullet1 => "1+0 · Bullet",
            TimePreset::Bullet2 => "2+1 · Bullet",
            TimePreset::Blitz3 => "3+0 · Blitz",
            TimePreset::Blitz3Inc => "3+2 · Blitz",
            TimePreset::Blitz5 => "5+0 · Blitz",
            TimePreset::Blitz5Inc => "5+3 · Blitz",
            TimePreset::Rapid10 => "10+0 · Rapid",
            TimePreset::Rapid15 => "15+10 · Rapid",
            TimePreset::Classical => "30+0 · Classical",
            TimePreset::Custom => "Custom",
            TimePreset::Unlimited => "No clock · depth 8",
        }
    }

    fn base_time_secs(&self) -> u32 {
        match self {
            TimePreset::Bullet1 => 60,
            TimePreset::Bullet2 => 120,
            TimePreset::Blitz3 | TimePreset::Blitz3Inc => 180,
            TimePreset::Blitz5 | TimePreset::Blitz5Inc => 300,
            TimePreset::Rapid10 => 600,
            TimePreset::Rapid15 => 900,
            TimePreset::Classical => 1800,
            TimePreset::Custom => 300,
            TimePreset::Unlimited => 0,
        }
    }

    fn increment_secs(&self) -> u32 {
        match self {
            TimePreset::Bullet2 => 1,
            TimePreset::Blitz3Inc => 2,
            TimePreset::Blitz5Inc => 3,
            TimePreset::Rapid15 => 10,
            _ => 0,
        }
    }
}

struct ChessClock {
    white_time_ms: u64,
    black_time_ms: u64,
    increment_ms: u64,
    last_update: Instant,
    running: bool,
    active_color: Color,
}

impl ChessClock {
    fn new(base_time_secs: u32, increment_secs: u32) -> Self {
        Self {
            white_time_ms: base_time_secs as u64 * 1000,
            black_time_ms: base_time_secs as u64 * 1000,
            increment_ms: increment_secs as u64 * 1000,
            last_update: Instant::now(),
            running: false,
            active_color: Color::White,
        }
    }

    fn start(&mut self, color: Color) {
        self.running = true;
        self.active_color = color;
        self.last_update = Instant::now();
    }

    fn stop(&mut self) {
        self.update();
        self.running = false;
    }

    fn switch(&mut self, new_color: Color) {
        self.update();
        if self.active_color == Color::White {
            self.white_time_ms = self.white_time_ms.saturating_add(self.increment_ms);
        } else {
            self.black_time_ms = self.black_time_ms.saturating_add(self.increment_ms);
        }
        self.active_color = new_color;
        self.last_update = Instant::now();
    }

    /// Hands the clock to `color` without adding an increment (used by undo).
    fn set_active(&mut self, color: Color) {
        self.update();
        self.active_color = color;
    }

    fn update(&mut self) {
        if !self.running {
            return;
        }
        let elapsed = self.last_update.elapsed().as_millis() as u64;
        self.last_update = Instant::now();

        if self.active_color == Color::White {
            self.white_time_ms = self.white_time_ms.saturating_sub(elapsed);
        } else {
            self.black_time_ms = self.black_time_ms.saturating_sub(elapsed);
        }
    }

    fn time(&self, color: Color) -> u64 {
        if color == Color::White {
            self.white_time_ms
        } else {
            self.black_time_ms
        }
    }

    fn format_time(ms: u64) -> String {
        let total_secs = ms / 1000;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        let tenths = (ms % 1000) / 100;

        if total_secs < 10 {
            format!("{}:{:02}.{}", mins, secs, tenths)
        } else {
            format!("{}:{:02}", mins, secs)
        }
    }

    fn reset(&mut self, base_time_secs: u32, increment_secs: u32) {
        *self = Self::new(base_time_secs, increment_secs);
    }
}

pub struct GuiApp {
    game: Game,
    /// Legal moves in the current position, refreshed after every move.
    legal: Vec<(String, String)>,
    /// Final result, draws included; `None` while the game goes on.
    outcome: Option<Outcome>,
    flag_winner: Option<Color>,
    san_moves: Vec<String>,
    last_move: Option<((usize, usize), (usize, usize))>,

    /// `None` while the engine is lent to the search thread.
    engine: Option<Engine>,
    search_rx: Option<Receiver<SearchDone>>,
    /// Bumped on restart/undo so results of an outdated search are dropped.
    generation: u64,
    vs_ai: bool,
    ai_color: Color,
    last_ai_time: Option<Duration>,
    last_ai_score: Option<SearchScore>,
    /// White's expected score in percent, from the engine's last search.
    white_win_pct: Option<f64>,

    selected: Option<(usize, usize)>,
    /// Square a piece is being dragged from while the button is held.
    drag_from: Option<(usize, usize)>,
    pending_promotion: Option<(String, String)>,
    flipped: bool,

    clock: ChessClock,
    time_preset: TimePreset,
    custom_base_mins: u32,
    custom_increment_secs: u32,
    game_started: bool,
}

impl GuiApp {
    pub fn new() -> Self {
        let mut engine = Engine::from_env(UNLIMITED_DEPTH, num_cpus::get());
        if let Ok(Some(path)) = engine.load_syzygy_from_env() {
            println!("Loaded Syzygy tablebases from {}", path);
        }
        let mut app = Self {
            game: Game::new(),
            legal: Vec::new(),
            outcome: None,
            flag_winner: None,
            san_moves: Vec::new(),
            last_move: None,

            engine: Some(engine),
            search_rx: None,
            generation: 0,
            vs_ai: true,
            ai_color: Color::Black,
            last_ai_time: None,
            last_ai_score: None,
            white_win_pct: None,

            selected: None,
            drag_from: None,
            pending_promotion: None,
            flipped: false,

            clock: ChessClock::new(300, 0),
            time_preset: TimePreset::Blitz5,
            custom_base_mins: 5,
            custom_increment_secs: 0,
            game_started: false,
        };
        app.refresh_position();
        app
    }

    fn clock_enabled(&self) -> bool {
        self.time_preset != TimePreset::Unlimited
    }

    fn game_over(&self) -> bool {
        self.outcome.is_some() || self.flag_winner.is_some()
    }

    fn human_color(&self) -> Color {
        opposite(self.ai_color)
    }

    /// True when Black is drawn at the bottom of the board.
    fn black_at_bottom(&self) -> bool {
        (self.vs_ai && self.human_color() == Color::Black) != self.flipped
    }

    fn is_ai_turn(&self) -> bool {
        self.vs_ai && !self.game_over() && self.game.current_turn == self.ai_color
    }

    fn ai_thinking(&self) -> bool {
        self.search_rx.is_some() && self.is_ai_turn()
    }

    fn can_move(&self, piece: Piece) -> bool {
        !self.game_over()
            && self.pending_promotion.is_none()
            && piece.color == self.game.current_turn
            && !(self.vs_ai && piece.color == self.ai_color)
    }

    fn refresh_position(&mut self) {
        let turn = self.game.current_turn;
        self.legal = self.game.board.all_legal_moves_fast(turn);
        self.outcome = self.game.outcome();
    }

    fn targets_from(&self, from: (usize, usize)) -> Vec<(usize, usize)> {
        let from = square_name(from);
        let mut targets: Vec<_> = self
            .legal
            .iter()
            .filter(|(s, _)| *s == from)
            .filter_map(|(_, e)| parse_square(e))
            .collect();
        targets.dedup();
        targets
    }

    fn time_config(&mut self) -> TimeConfig {
        if !self.clock_enabled() {
            return TimeConfig::fixed_depth(UNLIMITED_DEPTH);
        }
        self.clock.update();
        TimeConfig {
            wtime: Some(self.clock.white_time_ms),
            btime: Some(self.clock.black_time_ms),
            winc: Some(self.clock.increment_ms),
            binc: Some(self.clock.increment_ms),
            movestogo: None,
            depth: None,
            movetime: None,
            infinite: false,
        }
    }

    /// Plays `from`-`to` (`to` may carry a promotion letter) for whoever is to
    /// move, and updates the move list, the clock and the game state.
    fn apply_move(&mut self, from: &str, to: &str) -> bool {
        let san = san_without_suffix(&self.game.board, &self.legal, from, to);
        if !self.game.make_move(from, to) {
            return false;
        }
        self.refresh_position();
        let suffix = match self.outcome {
            Some(Outcome::Win(_)) => "#",
            _ if self.game.board.in_check_fast(self.game.current_turn) => "+",
            _ => "",
        };
        self.san_moves.push(san + suffix);
        self.last_move = parse_square(from).zip(parse_square(to));
        self.selected = None;

        if self.clock_enabled() {
            if !self.game_started {
                self.clock.start(self.game.current_turn);
            } else {
                self.clock.switch(self.game.current_turn);
            }
        }
        self.game_started = true;
        if self.outcome.is_some() {
            self.clock.stop();
        }
        true
    }

    /// Tries a human move between two squares; asks for the piece on promotion.
    fn try_move(&mut self, from: (usize, usize), to: (usize, usize)) -> bool {
        let (from_s, to_s) = (square_name(from), square_name(to));
        let candidates: Vec<&String> = self
            .legal
            .iter()
            .filter(|(s, e)| *s == from_s && e.get(..2) == Some(to_s.as_str()))
            .map(|(_, e)| e)
            .collect();
        if candidates.is_empty() {
            return false;
        }
        let is_pawn = matches!(self.game.board.get_index(from.0, from.1), Some(p) if p.piece_type == PieceType::Pawn);
        if is_pawn && (to.1 == 0 || to.1 == 7) {
            self.pending_promotion = Some((from_s, to_s));
            self.selected = None;
            return true;
        }
        self.apply_move(&from_s, &to_s)
    }

    /// Collects a finished background search, if any, and plays its move when
    /// it still applies to the current position.
    fn poll_search(&mut self) {
        let Some(rx) = &self.search_rx else {
            return;
        };
        let done = match rx.try_recv() {
            Ok(done) => done,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                eprintln!("AI search thread died; recreating the engine");
                self.search_rx = None;
                self.engine = Some(Engine::from_env(UNLIMITED_DEPTH, num_cpus::get()));
                return;
            }
        };
        self.search_rx = None;
        let score = done.engine.last_search_score();
        self.engine = Some(done.engine);

        if done.generation != self.generation || !self.is_ai_turn() {
            return;
        }
        let Some(((s, e), depth)) = done.best else {
            return;
        };
        let phase = chessmind::eval::game_phase(&self.game.board);
        if !self.apply_move(&s, &e) {
            return;
        }
        self.last_ai_time = Some(done.elapsed);
        self.last_ai_score = score;
        match score {
            Some(score) => {
                let ai_pct = score.win_percent_at(phase);
                self.white_win_pct = Some(if self.ai_color == Color::White {
                    ai_pct
                } else {
                    100.0 - ai_pct
                });
                println!(
                    "AI move {s}{e} in {:?} (depth {depth}) eval {score} win {ai_pct:.1}%",
                    done.elapsed
                );
            }
            None => println!("AI move {s}{e} in {:?} (book)", done.elapsed),
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        self.poll_search();

        if self.game_over() {
            return;
        }
        if self.clock_enabled() && self.clock.running {
            self.clock.update();
            for color in [Color::White, Color::Black] {
                if self.clock.time(color) == 0 {
                    self.flag_winner = Some(opposite(color));
                    self.clock.stop();
                    return;
                }
            }
        }

        if self.search_rx.is_some() || !self.is_ai_turn() {
            return;
        }
        let Some(mut engine) = self.engine.take() else {
            return;
        };

        let time_config = self.time_config();
        let mut game = clone_game(&self.game);
        let generation = self.generation;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.search_rx = Some(rx);
        thread::spawn(move || {
            let start = Instant::now();
            let best = engine.best_move_timed(&mut game, &time_config);
            let _ = tx.send(SearchDone {
                engine,
                best,
                elapsed: start.elapsed(),
                generation,
            });
            ctx.request_repaint();
        });
    }

    fn reset_clock(&mut self) {
        if self.time_preset == TimePreset::Custom {
            self.clock
                .reset(self.custom_base_mins * 60, self.custom_increment_secs);
        } else {
            self.clock.reset(
                self.time_preset.base_time_secs(),
                self.time_preset.increment_secs(),
            );
        }
    }

    fn new_game(&mut self) {
        // A search still running for the old game is ignored when it returns.
        self.generation += 1;
        self.game = Game::new();
        self.refresh_position();
        self.flag_winner = None;
        self.san_moves.clear();
        self.last_move = None;
        self.selected = None;
        self.drag_from = None;
        self.pending_promotion = None;
        self.last_ai_time = None;
        self.last_ai_score = None;
        self.white_win_pct = None;
        self.game_started = false;
        self.reset_clock();
    }

    fn can_undo(&self) -> bool {
        !self.game.history.is_empty() && self.flag_winner.is_none()
    }

    /// Takes back the last move, or the last two against the engine so that
    /// it is the player's turn again.
    fn undo(&mut self) {
        if !self.can_undo() {
            return;
        }
        let len = self.game.history.len();
        let turn_after = |n: usize| {
            if (len - n) % 2 == 0 {
                Color::White
            } else {
                Color::Black
            }
        };
        let mut n = 1;
        if self.vs_ai && len > 1 && turn_after(1) == self.ai_color {
            n = 2;
        }

        self.generation += 1;
        let history: Vec<_> = self.game.history[..len - n].to_vec();
        self.game = Game::new();
        for (s, e) in &history {
            self.game.make_move(s, e);
        }
        self.refresh_position();
        self.san_moves.truncate(history.len());
        self.last_move = history
            .last()
            .and_then(|(s, e)| parse_square(s).zip(parse_square(e)));
        self.selected = None;
        self.drag_from = None;
        self.pending_promotion = None;
        if self.clock.running {
            self.clock.set_active(self.game.current_turn);
        } else if self.clock_enabled() && !history.is_empty() {
            // Undoing out of a finished game: the clock stopped, restart it.
            self.clock.start(self.game.current_turn);
        }
        if history.is_empty() {
            self.game_started = false;
            self.reset_clock();
        }
    }

    fn status_text(&self) -> (String, Color32) {
        if let Some(winner) = self.flag_winner {
            return (
                format!("{} wins on time", color_name(winner)),
                palette::DANGER,
            );
        }
        match self.outcome {
            Some(Outcome::Win(winner)) => (
                format!("Checkmate · {} wins", color_name(winner)),
                palette::ACCENT,
            ),
            Some(Outcome::Draw(reason)) => {
                let why = match reason {
                    DrawReason::Stalemate => "stalemate",
                    DrawReason::FiftyMoveRule => "50-move rule",
                    DrawReason::ThreefoldRepetition => "threefold repetition",
                    DrawReason::InsufficientMaterial => "insufficient material",
                };
                (format!("Draw · {why}"), palette::TEXT)
            }
            None if self.ai_thinking() => ("Engine is thinking…".into(), palette::MUTED),
            None if self.vs_ai => {
                if self.game.current_turn == self.human_color() {
                    ("Your move".into(), palette::TEXT)
                } else {
                    ("Engine to move".into(), palette::MUTED)
                }
            }
            None => (
                format!("{} to move", color_name(self.game.current_turn)),
                palette::TEXT,
            ),
        }
    }

    fn player_name(&self, color: Color) -> &'static str {
        match (self.vs_ai, color == self.ai_color) {
            (true, true) => "Engine",
            (true, false) => "You",
            (false, _) => color_name(color),
        }
    }
}

// Drawing helpers

fn piece_glyphs(pt: PieceType) -> (char, char) {
    // (filled, outline)
    match pt {
        PieceType::King => ('♚', '♔'),
        PieceType::Queen => ('♛', '♕'),
        PieceType::Rook => ('♜', '♖'),
        PieceType::Bishop => ('♝', '♗'),
        PieceType::Knight => ('♞', '♘'),
        PieceType::Pawn => ('♟', '♙'),
    }
}

/// White pieces are the filled glyph painted white under the outline glyph, so
/// they keep their inner details; black pieces are the filled glyph.
fn draw_piece(painter: &egui::Painter, center: Pos2, size: f32, piece: Piece) {
    let font = FontId::new(size * 0.84, FontFamily::Name(PIECE_FONT.into()));
    let (filled, outline) = piece_glyphs(piece.piece_type);
    let center = center + vec2(0.0, size * 0.02);
    let shadow = center + vec2(0.0, size * 0.035);
    painter.text(
        shadow,
        Align2::CENTER_CENTER,
        filled,
        font.clone(),
        palette::PIECE_SHADOW,
    );
    match piece.color {
        Color::White => {
            painter.text(
                center,
                Align2::CENTER_CENTER,
                filled,
                font.clone(),
                palette::WHITE_PIECE,
            );
            painter.text(
                center,
                Align2::CENTER_CENTER,
                outline,
                font,
                palette::BLACK_PIECE,
            );
        }
        Color::Black => {
            painter.text(
                center,
                Align2::CENTER_CENTER,
                filled,
                font,
                palette::BLACK_PIECE,
            );
        }
    }
}

impl GuiApp {
    fn square_rect(&self, board: Rect, sq: (usize, usize)) -> Rect {
        let s = board.width() / 8.0;
        let (col, row) = if self.black_at_bottom() {
            (7 - sq.0, sq.1)
        } else {
            (sq.0, 7 - sq.1)
        };
        Rect::from_min_size(board.min + vec2(col as f32 * s, row as f32 * s), vec2(s, s))
    }

    fn square_at(&self, board: Rect, pos: Pos2) -> Option<(usize, usize)> {
        if !board.contains(pos) {
            return None;
        }
        let s = board.width() / 8.0;
        let col = (((pos.x - board.left()) / s) as usize).min(7);
        let row = (((pos.y - board.top()) / s) as usize).min(7);
        Some(if self.black_at_bottom() {
            (7 - col, row)
        } else {
            (col, 7 - row)
        })
    }

    fn handle_board_input(&mut self, ctx: &egui::Context, board: Rect, response: &egui::Response) {
        if self.pending_promotion.is_some() {
            self.drag_from = None;
            return;
        }
        let (pos, pressed, released, down) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.primary_down(),
            )
        });
        let square = pos.and_then(|p| self.square_at(board, p));

        if pressed && response.hovered() {
            if let Some(sq) = square {
                let moved = match self.selected {
                    Some(sel) if sel != sq => self.try_move(sel, sq),
                    _ => false,
                };
                if !moved {
                    match self.game.board.get_index(sq.0, sq.1) {
                        Some(p) if self.can_move(p) => {
                            self.selected = Some(sq);
                            self.drag_from = Some(sq);
                        }
                        _ => self.selected = None,
                    }
                }
            }
        }

        if let Some(from) = self.drag_from {
            if released {
                self.drag_from = None;
                if let Some(sq) = square {
                    if sq != from && !self.try_move(from, sq) {
                        self.selected = None;
                    }
                }
            } else if !down {
                self.drag_from = None;
            }
        }
    }

    fn draw_board(&self, ui: &egui::Ui, board: Rect) {
        let painter = ui.painter_at(board.expand(2.0));
        let s = board.width() / 8.0;
        let radius = 8.0;

        for y in 0..8 {
            for x in 0..8 {
                let rect = self.square_rect(board, (x, y));
                // a1 (0, 0) is a dark square.
                let light = (x + y) % 2 == 1;
                let fill = if light {
                    palette::LIGHT_SQ
                } else {
                    palette::DARK_SQ
                };
                let mut rounding = Rounding::ZERO;
                let at = |c: Pos2| (c - rect.center()).length() < s;
                if at(board.left_top()) {
                    rounding.nw = radius;
                }
                if at(board.right_top()) {
                    rounding.ne = radius;
                }
                if at(board.left_bottom()) {
                    rounding.sw = radius;
                }
                if at(board.right_bottom()) {
                    rounding.se = radius;
                }
                painter.rect_filled(rect, rounding, fill);

                if let Some((from, to)) = self.last_move {
                    if (x, y) == from || (x, y) == to {
                        painter.rect_filled(rect, rounding, palette::LAST_MOVE);
                    }
                }
                if self.selected == Some((x, y)) {
                    painter.rect_filled(rect, rounding, palette::SELECTED);
                }

                // Coordinates on the bottom row and the left column.
                let label_color = if light {
                    palette::DARK_SQ
                } else {
                    palette::LIGHT_SQ
                };
                let font = FontId::proportional((s * 0.17).max(9.0));
                if (rect.bottom() - board.bottom()).abs() < 1.0 {
                    painter.text(
                        rect.right_bottom() + vec2(-s * 0.06, -s * 0.03),
                        Align2::RIGHT_BOTTOM,
                        (b'a' + x as u8) as char,
                        font.clone(),
                        label_color,
                    );
                }
                if (rect.left() - board.left()).abs() < 1.0 {
                    painter.text(
                        rect.left_top() + vec2(s * 0.06, s * 0.03),
                        Align2::LEFT_TOP,
                        (y + 1).to_string(),
                        font,
                        label_color,
                    );
                }
            }
        }

        // King in check.
        if !self.game_over() || matches!(self.outcome, Some(Outcome::Win(_))) {
            let side = self.game.current_turn;
            if self.game.board.in_check_fast(side) {
                if let Some(king) = self.game.board.find_king(side) {
                    let rect = self.square_rect(board, king);
                    painter.circle_filled(rect.center(), s * 0.46, palette::CHECK);
                }
            }
        }

        let pointer = ui.ctx().input(|i| i.pointer.interact_pos());
        let lifted = match (self.drag_from, pointer) {
            (Some(from), Some(p)) if !self.square_rect(board, from).contains(p) => Some((from, p)),
            _ => None,
        };

        for y in 0..8 {
            for x in 0..8 {
                if lifted.is_some_and(|(from, _)| from == (x, y)) {
                    continue;
                }
                if let Some(p) = self.game.board.get_index(x, y) {
                    draw_piece(&painter, self.square_rect(board, (x, y)).center(), s, p);
                }
            }
        }

        // Legal move hints for the selected piece.
        if let Some(sel) = self.selected {
            for t in self.targets_from(sel) {
                let rect = self.square_rect(board, t);
                if self.game.board.get_index(t.0, t.1).is_some() {
                    painter.circle_stroke(
                        rect.center(),
                        s * 0.44,
                        Stroke::new(s * 0.07, palette::HINT),
                    );
                } else {
                    painter.circle_filled(rect.center(), s * 0.15, palette::HINT);
                }
            }
        }

        if let Some((from, p)) = lifted {
            if let Some(sq) = self.square_at(board, p) {
                let rect = self.square_rect(board, sq);
                painter.rect_stroke(
                    rect.shrink(1.5),
                    2.0,
                    Stroke::new(3.0_f32, Color32::from_white_alpha(170)),
                );
            }
            if let Some(piece) = self.game.board.get_index(from.0, from.1) {
                let top = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("dragged-piece"),
                ));
                draw_piece(&top, p, s * 1.08, piece);
            }
        }
    }

    fn draw_eval_bar(&self, ui: &egui::Ui, bar: Rect) {
        let painter = ui.painter();
        let white = (self.white_win_pct.unwrap_or(50.0) / 100.0).clamp(0.0, 1.0) as f32;
        painter.rect_filled(bar, 5.0, Color32::from_rgb(20, 24, 22));
        let h = bar.height() * white;
        let white_rect = if self.black_at_bottom() {
            Rect::from_min_size(bar.min, vec2(bar.width(), h))
        } else {
            Rect::from_min_size(
                Pos2::new(bar.left(), bar.bottom() - h),
                vec2(bar.width(), h),
            )
        };
        painter.rect_filled(white_rect, 5.0, palette::WHITE_PIECE);
        painter.line_segment(
            [bar.left_center(), bar.right_center()],
            Stroke::new(1.0_f32, palette::MUTED),
        );
        painter.rect_stroke(bar, 5.0, Stroke::new(1.0_f32, palette::STAGE_EDGE));
    }

    fn player_card(&self, ui: &mut egui::Ui, color: Color) {
        let active = !self.game_over() && self.game.current_turn == color;
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 64.0), egui::Sense::hover());
        let painter = ui.painter();
        let (fill, border) = if active {
            (palette::ACCENT_SOFT, palette::ACCENT)
        } else {
            (palette::CARD, palette::BORDER)
        };
        painter.rect(rect, 12.0, fill, Stroke::new(1.0_f32, border));

        let dot = Pos2::new(rect.left() + 22.0, rect.center().y);
        let dot_fill = if color == Color::White {
            palette::WHITE_PIECE
        } else {
            palette::BLACK_PIECE
        };
        painter.circle(dot, 7.0, dot_fill, Stroke::new(1.0_f32, palette::MUTED));
        let x = rect.left() + 40.0;
        painter.text(
            Pos2::new(x, rect.center().y + 1.0),
            Align2::LEFT_BOTTOM,
            self.player_name(color),
            FontId::proportional(15.0),
            palette::TEXT,
        );
        painter.text(
            Pos2::new(x, rect.center().y + 3.0),
            Align2::LEFT_TOP,
            color_name(color),
            FontId::proportional(12.0),
            palette::MUTED,
        );

        let ms = self.clock.time(color);
        let low = self.clock_enabled() && ms < 20_000;
        let text = if self.clock_enabled() {
            ChessClock::format_time(ms)
        } else {
            "–".into()
        };
        let pill = Rect::from_center_size(
            Pos2::new(rect.right() - 12.0 - 46.0, rect.center().y),
            vec2(92.0, 38.0),
        );
        let (pill_fill, text_color) = match (active, low) {
            (true, true) => (palette::DANGER, Color32::WHITE),
            (true, false) => (palette::ACCENT, Color32::WHITE),
            (false, true) => (palette::PANEL, palette::DANGER),
            (false, false) => (palette::PANEL, palette::MUTED),
        };
        painter.rect(
            pill,
            9.0,
            pill_fill,
            Stroke::new(1.0_f32, if active { pill_fill } else { palette::BORDER }),
        );
        painter.text(
            pill.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::monospace(20.0),
            text_color,
        );
        if active && self.ai_thinking() {
            let spot = Rect::from_center_size(
                Pos2::new(pill.left() - 18.0, rect.center().y),
                vec2(16.0, 16.0),
            );
            ui.put(spot, egui::Spinner::new().size(14.0).color(palette::ACCENT));
        }
    }

    fn move_list(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.san_moves.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("No moves yet").color(palette::MUTED));
                    return;
                }
                let last = self.san_moves.len() - 1;
                egui::Grid::new("moves")
                    .num_columns(3)
                    .striped(true)
                    .spacing(vec2(12.0, 6.0))
                    .min_col_width(28.0)
                    .show(ui, |ui| {
                        for (i, pair) in self.san_moves.chunks(2).enumerate() {
                            ui.label(
                                RichText::new(format!("{}.", i + 1))
                                    .color(palette::MUTED)
                                    .monospace(),
                            );
                            for j in 0..2 {
                                let idx = i * 2 + j;
                                match pair.get(j) {
                                    Some(san) => {
                                        let mut text = RichText::new(san).monospace().size(14.0);
                                        if idx == last {
                                            text = text.strong().color(palette::ACCENT);
                                        }
                                        ui.add(egui::Label::new(text).wrap(false));
                                    }
                                    None => {
                                        ui.label("");
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("♞")
                .family(FontFamily::Name(PIECE_FONT.into()))
                .size(40.0)
                .color(palette::ACCENT),
        );
        ui.add_space(12.0);

        let section = |ui: &mut egui::Ui, title: &str| {
            ui.add_space(10.0);
            ui.label(
                RichText::new(title.to_uppercase())
                    .size(11.0)
                    .color(palette::MUTED)
                    .strong(),
            );
            ui.add_space(4.0);
        };
        let full = ui.available_width();
        let segmented = |ui: &mut egui::Ui, selected: bool, text: &str| {
            let (fill, color) = if selected {
                (palette::ACCENT, Color32::WHITE)
            } else {
                (palette::CARD, palette::TEXT)
            };
            let button = egui::Button::new(RichText::new(text).size(13.5).color(color))
                .fill(fill)
                .stroke(Stroke::NONE)
                .min_size(vec2((full - 6.0) / 2.0, 30.0));
            ui.add(button).clicked()
        };

        section(ui, "Mode");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if segmented(ui, self.vs_ai, "vs Engine") {
                self.vs_ai = true;
            }
            if segmented(ui, !self.vs_ai, "Two players") {
                self.vs_ai = false;
            }
        });

        if self.vs_ai {
            section(ui, "You play");
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let human = self.human_color();
                if segmented(ui, human == Color::White, "White") {
                    self.ai_color = Color::Black;
                    self.selected = None;
                }
                if segmented(ui, human == Color::Black, "Black") {
                    self.ai_color = Color::White;
                    self.selected = None;
                }
            });
        }

        section(ui, "Time control");
        let locked = self.game_started && !self.game_over();
        ui.add_enabled_ui(!locked, |ui| {
            let mut changed = false;
            egui::ComboBox::from_id_source("time-control")
                .width(full)
                .selected_text(self.time_preset.name())
                .show_ui(ui, |ui| {
                    for preset in TimePreset::ALL {
                        changed |= ui
                            .selectable_value(&mut self.time_preset, preset, preset.name())
                            .changed();
                    }
                });
            if self.time_preset == TimePreset::Custom {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Minutes");
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.custom_base_mins).clamp_range(1..=120))
                        .changed();
                    ui.label("Increment");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.custom_increment_secs)
                                .clamp_range(0..=60)
                                .suffix(" s"),
                        )
                        .changed();
                });
            }
            if changed && !self.game_started {
                self.reset_clock();
            }
        });
        if locked {
            ui.label(
                RichText::new("Locked during a game")
                    .size(11.5)
                    .color(palette::MUTED),
            );
        }

        ui.add_space(24.0);
        let primary = egui::Button::new(
            RichText::new("New game")
                .size(15.0)
                .strong()
                .color(Color32::WHITE),
        )
        .fill(palette::ACCENT)
        .min_size(vec2(full, 38.0));
        if ui.add(primary).clicked() {
            self.new_game();
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let half = vec2((full - 6.0) / 2.0, 32.0);
            if ui
                .add_enabled(self.can_undo(), egui::Button::new("Undo").min_size(half))
                .on_hover_text("Take back a move (Ctrl+Z)")
                .clicked()
            {
                self.undo();
            }
            if ui
                .add(egui::Button::new("Flip board").min_size(half))
                .on_hover_text("Flip the board (F)")
                .clicked()
            {
                self.flipped = !self.flipped;
            }
        });
    }
}

impl App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        if self.clock.running {
            ctx.request_repaint_after(CLOCK_REPAINT);
        }

        if !ctx.wants_keyboard_input() {
            let (flip, undo, new) = ctx.input(|i| {
                (
                    i.key_pressed(egui::Key::F),
                    i.modifiers.command && i.key_pressed(egui::Key::Z),
                    i.modifiers.command && i.key_pressed(egui::Key::N),
                )
            });
            if flip {
                self.flipped = !self.flipped;
            }
            if undo {
                self.undo();
            }
            if new {
                self.new_game();
            }
        }

        self.tick(ctx);

        let panel_frame = egui::Frame::none().fill(palette::PANEL);
        let inner = egui::Frame::none().inner_margin(egui::Margin::same(20.0));

        egui::SidePanel::left("controls")
            .resizable(false)
            .exact_width(250.0)
            .frame(panel_frame)
            .show(ctx, |ui| {
                inner.show(ui, |ui| self.controls(ui));
            });

        egui::SidePanel::right("info")
            .resizable(false)
            .exact_width(300.0)
            .frame(panel_frame)
            .show(ctx, |ui| {
                inner.show(ui, |ui| {
                    self.clock.update();
                    let top = if self.black_at_bottom() {
                        Color::White
                    } else {
                        Color::Black
                    };
                    self.player_card(ui, top);
                    ui.add_space(18.0);

                    let (status, color) = self.status_text();
                    ui.add_space(4.0);
                    ui.label(RichText::new(status).size(16.0).color(color));
                    if let (Some(score), Some(pct)) = (self.last_ai_score, self.white_win_pct) {
                        let you = if self.vs_ai && self.human_color() == Color::Black {
                            100.0 - pct
                        } else {
                            pct
                        };
                        let who = if self.vs_ai { "You" } else { "White" };
                        let time = self
                            .last_ai_time
                            .map(|t| format!(" · {:.1} s", t.as_secs_f32()))
                            .unwrap_or_default();
                        ui.label(
                            RichText::new(format!("{who} {you:.0}% · eval {score}{time}"))
                                .size(12.5)
                                .color(palette::MUTED),
                        );
                    }
                    ui.add_space(10.0);

                    let card_height = 64.0;
                    let list_height = (ui.available_height() - card_height - 12.0).max(80.0);
                    egui::Frame::none()
                        .fill(palette::CARD)
                        .rounding(12.0)
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.set_height(list_height - 24.0);
                            ui.set_width(ui.available_width());
                            self.move_list(ui);
                        });
                    ui.add_space(12.0);
                    self.player_card(ui, opposite(top));
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(palette::STAGE)
                    .inner_margin(egui::Margin::same(40.0)),
            )
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                let (bar_w, gap) = (12.0, 12.0);
                let size = (avail.width() - bar_w - gap).min(avail.height()).max(200.0);
                let size = (size / 8.0).floor() * 8.0;
                let left = avail.center().x - (size + bar_w + gap) / 2.0;
                let top = avail.center().y - size / 2.0;
                let bar = Rect::from_min_size(Pos2::new(left, top), vec2(bar_w, size));
                let board =
                    Rect::from_min_size(Pos2::new(left + bar_w + gap, top), vec2(size, size));

                ui.painter().add(
                    egui::epaint::Shadow {
                        offset: vec2(0.0, 14.0),
                        blur: 40.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(110),
                    }
                    .tessellate(board, 8.0),
                );
                let response = ui.allocate_rect(board, egui::Sense::click_and_drag());
                self.handle_board_input(ctx, board, &response);
                self.draw_eval_bar(ui, bar);
                self.draw_board(ui, board);
            });

        self.promotion_dialog(ctx);
    }
}

impl GuiApp {
    fn promotion_dialog(&mut self, ctx: &egui::Context) {
        let Some((from, to)) = self.pending_promotion.clone() else {
            return;
        };
        let color = self.game.current_turn;
        let mut choice = None;
        let mut cancel = false;
        egui::Window::new("Promote")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(palette::PANEL)
                    .rounding(14.0)
                    .inner_margin(egui::Margin::same(18.0)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new("Promote to").strong().size(15.0));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for (pt, letter) in [
                        (PieceType::Queen, 'q'),
                        (PieceType::Rook, 'r'),
                        (PieceType::Bishop, 'b'),
                        (PieceType::Knight, 'n'),
                    ] {
                        let (rect, resp) =
                            ui.allocate_exact_size(vec2(68.0, 68.0), egui::Sense::click());
                        let fill = if resp.hovered() {
                            palette::DARK_SQ
                        } else {
                            palette::LIGHT_SQ
                        };
                        ui.painter().rect_filled(rect, 8.0, fill);
                        draw_piece(
                            ui.painter(),
                            rect.center(),
                            68.0,
                            Piece {
                                piece_type: pt,
                                color,
                            },
                        );
                        if resp.clicked() {
                            choice = Some(letter);
                        }
                    }
                });
                ui.add_space(6.0);
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if let Some(letter) = choice {
            self.pending_promotion = None;
            self.apply_move(&from, &format!("{to}{letter}"));
        } else if cancel {
            self.pending_promotion = None;
        }
    }
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut family = Vec::new();
    if let Some(bytes) = PIECE_FONT_PATHS.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts
            .font_data
            .insert(PIECE_FONT.into(), egui::FontData::from_owned(bytes));
        family.push(PIECE_FONT.to_owned());
    }
    family.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(PIECE_FONT.into()), family);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = vec2(8.0, 6.0);
    style.spacing.button_padding = vec2(12.0, 6.0);
    style.spacing.interact_size.y = 28.0;
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.0));

    let v = &mut style.visuals;
    *v = egui::Visuals::light();
    v.panel_fill = palette::PANEL;
    v.window_fill = palette::PANEL;
    v.extreme_bg_color = palette::BG;
    v.faint_bg_color = palette::CARD_HOVER;
    v.override_text_color = Some(palette::TEXT);
    v.selection.bg_fill = palette::ACCENT_SOFT;
    v.selection.stroke = Stroke::new(1.0_f32, palette::ACCENT);
    v.window_shadow = egui::epaint::Shadow {
        offset: vec2(0.0, 8.0),
        blur: 24.0,
        spread: 0.0,
        color: Color32::from_black_alpha(30),
    };
    v.window_rounding = Rounding::same(12.0);
    v.window_stroke = Stroke::new(1.0_f32, palette::BORDER);
    v.menu_rounding = Rounding::same(8.0);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.rounding = Rounding::same(8.0);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, palette::BORDER);
    v.widgets.inactive.weak_bg_fill = palette::CARD;
    v.widgets.inactive.bg_fill = palette::CARD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, palette::BORDER);
    v.widgets.hovered.weak_bg_fill = palette::CARD_HOVER;
    v.widgets.hovered.bg_fill = palette::CARD_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, palette::BORDER);
    v.widgets.active.weak_bg_fill = palette::CARD_HOVER;
    v.widgets.active.bg_fill = palette::CARD_HOVER;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, palette::ACCENT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, palette::BORDER);
    ctx.set_style(style);
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Chess")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Chess",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Box::new(GuiApp::new())
        }),
    )
    .unwrap();
}
