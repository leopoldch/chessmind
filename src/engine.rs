use crate::board::Board; // Removed color_idx, UndoState
use crate::game::Game;
use crate::opening::book_move;
use crate::pieces::{Color, PieceType};
use crate::transposition::{Bound, TABLE_SIZE, TTEntry, Table};
use crate::types::{Move, mvv_lva_score}; // Import Move, mvv_lva_score
use shakmaty::{CastlingMode, Chess, fen::Fen};
use shakmaty_syzygy::{Tablebase, Wdl};
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct TimeConfig {
    pub wtime: Option<u64>,
    pub btime: Option<u64>,
    pub winc: Option<u64>,
    pub binc: Option<u64>,
    pub movestogo: Option<u32>,
    pub depth: Option<u32>,
    pub movetime: Option<u64>,
    pub infinite: bool,
}

impl TimeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fixed_depth(depth: u32) -> Self {
        Self {
            depth: Some(depth),
            ..Default::default()
        }
    }

    pub fn fixed_time(ms: u64) -> Self {
        Self {
            movetime: Some(ms),
            ..Default::default()
        }
    }

    pub fn infinite() -> Self {
        Self {
            infinite: true,
            ..Default::default()
        }
    }
}

/// Time allocation for one search.
///
/// * `soft_time_ms`: no new iteration is started past this point (it is stretched
///   when the best move or the score is unstable, see `should_continue_iterating`),
///   and none is started when the previous one suggests it could not finish in time.
/// * `hard_time_ms`: the only limit that aborts an iteration in progress. It is
///   already net of the move overhead, so it stays safe against the clock.
#[allow(dead_code)]
struct TimeManager {
    start_time: Instant,
    soft_time_ms: u64,
    hard_time_ms: u64,
    move_overhead_ms: u64,
    increment_ms: u64,
    in_crisis: bool,
    /// `go movetime`: the whole budget is usable, no soft-limit scaling.
    fixed_time: bool,
    stop_flag: Arc<AtomicBool>,
    node_count: Arc<AtomicU64>,
}

#[allow(dead_code)]
impl TimeManager {
    fn new(config: &TimeConfig, color: Color, stop_flag: Arc<AtomicBool>) -> Self {
        let node_count = Arc::new(AtomicU64::new(0));

        if let Some(movetime) = config.movetime {
            let move_overhead_ms = Self::move_overhead_ms(movetime, 0);
            let budget = movetime.saturating_sub(move_overhead_ms).max(10);
            return Self {
                start_time: Instant::now(),
                soft_time_ms: budget,
                hard_time_ms: budget,
                move_overhead_ms,
                increment_ms: 0,
                in_crisis: false,
                fixed_time: true,
                stop_flag,
                node_count,
            };
        }

        if config.infinite || config.depth.is_some() {
            return Self {
                start_time: Instant::now(),
                soft_time_ms: u64::MAX,
                hard_time_ms: u64::MAX,
                move_overhead_ms: 0,
                increment_ms: 0,
                in_crisis: false,
                fixed_time: false,
                stop_flag,
                node_count,
            };
        }

        let our_time = match color {
            Color::White => config.wtime.unwrap_or(60000),
            Color::Black => config.btime.unwrap_or(60000),
        };

        let increment = match color {
            Color::White => config.winc.unwrap_or(0),
            Color::Black => config.binc.unwrap_or(0),
        };

        let (soft_time_ms, hard_time_ms, move_overhead_ms, in_crisis) =
            Self::calculate_time(our_time, increment, config.movestogo);

        Self {
            start_time: Instant::now(),
            soft_time_ms,
            hard_time_ms,
            move_overhead_ms,
            increment_ms: increment,
            in_crisis,
            fixed_time: false,
            stop_flag,
            node_count,
        }
    }

    fn calculate_time(
        time_left_ms: u64,
        increment_ms: u64,
        moves_to_go: Option<u32>,
    ) -> (u64, u64, u64, bool) {
        let move_overhead_ms = Self::move_overhead_ms(time_left_ms, increment_ms);
        let usable_time_ms = time_left_ms.saturating_sub(move_overhead_ms);

        let estimated_moves = if let Some(mtg) = moves_to_go {
            mtg.max(1) as u64
        } else {
            if time_left_ms < 30_000 {
                15
            } else if time_left_ms < 60_000 {
                20
            } else if time_left_ms < 180_000 {
                25
            } else if time_left_ms < 300_000 {
                30
            } else if time_left_ms < 600_000 {
                35
            } else {
                40
            }
        };

        let base_time = usable_time_ms / estimated_moves;
        let inc_bonus = (increment_ms * 3) / 4;

        let mut soft_time_ms = base_time + inc_bonus;

        let in_crisis = if increment_ms > 0 {
            time_left_ms < 1_500
        } else {
            time_left_ms < 5_000
        };

        if in_crisis {
            soft_time_ms = if increment_ms > 0 {
                (usable_time_ms / 20).max(30).min(400)
            } else {
                (usable_time_ms / 30).max(25).min(250)
            };
        }

        // The hard limit ends an iteration in progress: a few soft budgets, but never
        // more than a slice of the clock (the increment only refills it afterwards).
        let clock_cap = if increment_ms > 0 {
            usable_time_ms / 3 + increment_ms / 2
        } else {
            usable_time_ms / 4
        };
        let mut hard_time_ms = soft_time_ms
            .saturating_mul(4)
            .min(clock_cap)
            .min(usable_time_ms * 3 / 4);

        let reserve_ms = move_overhead_ms.max(10);
        if hard_time_ms <= reserve_ms {
            hard_time_ms = reserve_ms + 10;
        }
        soft_time_ms = soft_time_ms.min(hard_time_ms.saturating_sub(reserve_ms));

        soft_time_ms = soft_time_ms.max(10);
        hard_time_ms = hard_time_ms.max(soft_time_ms + 10);

        (soft_time_ms, hard_time_ms, move_overhead_ms, in_crisis)
    }

    fn move_overhead_ms(time_left_ms: u64, increment_ms: u64) -> u64 {
        let base = if increment_ms > 0 {
            time_left_ms / 3000
        } else {
            time_left_ms / 2000
        };
        base.clamp(10, 50)
    }

    /// Mid-iteration check: only the hard limit (or an external stop) aborts a search.
    #[inline(always)]
    fn should_stop(&self) -> bool {
        if self.stop_flag.load(Ordering::Relaxed) {
            return true;
        }

        let elapsed = self.start_time.elapsed().as_millis() as u64;
        elapsed >= self.hard_time_ms
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn time_exceeded(&self) -> bool {
        let elapsed = self.start_time.elapsed().as_millis() as u64;
        elapsed >= self.hard_time_ms
    }

    #[allow(dead_code)]
    fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    #[allow(dead_code)]
    fn signal_stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
    }

    fn nodes(&self) -> u64 {
        self.node_count.load(Ordering::Relaxed)
    }

    /// Soft limit for the next iteration, in percent of `soft_time_ms`: stretched
    /// when the best move just changed or the score dropped.
    fn soft_scale_pct(
        depth_completed: u32,
        current_score: i32,
        previous_score: Option<i32>,
        current_best_move: Option<Move>,
        previous_best_move: Option<Move>,
    ) -> u64 {
        let mut pct = 100;
        if depth_completed >= 4
            && current_best_move.is_some()
            && previous_best_move.is_some()
            && current_best_move != previous_best_move
        {
            pct += 40;
        }
        if let Some(prev) = previous_score {
            let drop = prev - current_score;
            if drop >= 60 {
                pct += 40;
            } else if drop >= 25 {
                pct += 20;
            }
        }
        pct
    }

    fn should_continue_iterating(
        &self,
        depth_completed: u32,
        last_iteration_ms: Option<u64>,
        current_score: i32,
        previous_score: Option<i32>,
        current_best_move: Option<Move>,
        previous_best_move: Option<Move>,
    ) -> bool {
        if self.stop_flag.load(Ordering::Relaxed) {
            return false;
        }
        let elapsed = self.start_time.elapsed().as_millis() as u64;
        if elapsed >= self.hard_time_ms {
            return false;
        }

        let reserve = if self.increment_ms > 0 {
            self.move_overhead_ms.saturating_sub(5).max(10)
        } else {
            self.move_overhead_ms.max(10)
        };

        let soft_limit = if self.fixed_time {
            self.soft_time_ms
        } else {
            let pct = Self::soft_scale_pct(
                depth_completed,
                current_score,
                previous_score,
                current_best_move,
                previous_best_move,
            );
            (self.soft_time_ms.saturating_mul(pct) / 100)
                .min(self.hard_time_ms.saturating_sub(reserve))
                .max(self.soft_time_ms.min(self.hard_time_ms))
        };
        if elapsed >= soft_limit {
            return false;
        }

        let remaining = soft_limit - elapsed;
        if remaining <= reserve {
            return false;
        }

        if depth_completed == 0 {
            return remaining > reserve * 2;
        }

        let iter_cost = last_iteration_ms.unwrap_or_else(|| {
            let divisor = depth_completed.saturating_add(2) as u64;
            (self.soft_time_ms / divisor).max(reserve)
        });

        let score_delta = previous_score.map(|prev| (current_score - prev).abs() as u64);
        let stable_move = current_best_move.is_some()
            && current_best_move == previous_best_move
            && score_delta.is_some_and(|delta| delta <= 16);

        // The next iteration is expected to end before the (stretched) soft limit;
        // when it overruns, the hard limit still lets it complete.
        let required = if self.in_crisis {
            iter_cost.saturating_add(reserve * 2)
        } else if stable_move {
            iter_cost.saturating_mul(2).saturating_add(reserve)
        } else {
            iter_cost.saturating_mul(3) / 2 + reserve
        };

        remaining > required
    }
}

const RFP_MARGIN: [i32; 4] = [0, 150, 250, 350];
const FUTILITY_MARGIN: [i32; 5] = [0, 120, 220, 320, 440];
const HLP_THRESHOLD: u32 = 3;
/// History-leaf pruning drops the remaining quiets once a quiet's combined
/// (butterfly + continuation) history falls below `HLP_BASE * depth`.
const HLP_BASE: i32 = -2_000;
/// Saturation bound of the gravity-style history tables.
const HISTORY_MAX: i32 = 16_384;
const MAX_TRIED_QUIETS: usize = 64;
const MAX_TRIED_CAPTURES: usize = 32;
const LMP_LIMITS: [usize; 5] = [0, 5, 7, 10, 14];
const MATE_VALUE: i32 = 10000;
/// Halfmove clock value at which the fifty-move rule makes the game a draw.
const FIFTY_MOVE_PLIES: u16 = 100;
const MAX_PLY: usize = 128;
const MAX_DEPTH: u32 = 64;
const NODE_FLUSH_INTERVAL: u64 = 2048;
/// Optimistic victim values (max of middlegame/endgame material) for qsearch delta pruning.
const QS_DELTA_VALUES: [i32; 6] = [120, 320, 330, 550, 1000, 0];
const QS_DELTA_MARGIN: i32 = 200;

/// Late-move reduction by `[depth][move index]`: `ln(depth) * ln(idx + 1) / 1.5`,
/// clamped to `1..=depth - 1`, and zero for `depth < 3` or `idx < 3`.
static LMR_TABLE: std::sync::LazyLock<[[u8; 64]; 64]> = std::sync::LazyLock::new(|| {
    let mut table = [[0u8; 64]; 64];
    for depth in 3..64usize {
        for idx in 3..64usize {
            let r = ((depth as f64).ln() * ((idx + 1) as f64).ln() / 1.5) as usize;
            table[depth][idx] = r.clamp(1, depth - 1) as u8;
        }
    }
    table
});
const ORDER_TT: u8 = 0;
const ORDER_PROMOTION: u8 = 1;
const ORDER_GOOD_CAPTURE: u8 = 2;
const ORDER_KILLER_1: u8 = 3;
const ORDER_KILLER_2: u8 = 4;
const ORDER_QUIET: u8 = 5;
const ORDER_BAD_CAPTURE: u8 = 6;

#[derive(Clone, Copy, Debug)]
struct RootSearchResult {
    index: usize,
    mv: Move,
    score: i32,
    /// false when the move failed low against the shared alpha (score is an upper bound).
    exact: bool,
}

struct PonderState {
    root_hash: u64,
    predicted_move: Move,
    stop_flag: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

const CONT_HISTORY_PIECE_TYPES: usize = 6;
const CONT_HISTORY_SQUARES: usize = 64;
const CONT_HISTORY_SIZE: usize = CONT_HISTORY_PIECE_TYPES
    * CONT_HISTORY_SQUARES
    * CONT_HISTORY_PIECE_TYPES
    * CONT_HISTORY_SQUARES;

type ContinuationHistory = Vec<i32>;

pub struct Engine {
    pub depth: u32,
    pub threads: usize,
    tt: Table,
    killers: Vec<[Option<Move>; 2]>,
    quiet_history: [[i32; 64]; 64],
    capture_history: [[i32; 64]; 64],
    cont_history: ContinuationHistory,
    tb: Option<Arc<Tablebase<Chess>>>,
    stop_flag: Arc<AtomicBool>,
    time_manager: Option<Arc<TimeManager>>,
    search_history: Vec<u64>,
    ponder: Option<PonderState>,
    last_nodes: u64,
    /// Nodes searched by this engine instance not yet added to the shared
    /// `TimeManager::node_count` (flushed every `NODE_FLUSH_INTERVAL` nodes).
    local_nodes: u64,
    /// Index in `search_history` before which no position can repeat the current
    /// one (set after irreversible moves and null moves).
    rep_floor: usize,
}

impl Clone for Engine {
    fn clone(&self) -> Self {
        Self {
            depth: self.depth,
            threads: self.threads,
            tt: self.tt.clone(), // Arc clone - shares the table!
            killers: self.killers.clone(),
            quiet_history: self.quiet_history,     // Array copy
            capture_history: self.capture_history, // Array copy
            cont_history: self.cont_history.clone(),
            tb: self.tb.clone(),
            stop_flag: self.stop_flag.clone(),
            time_manager: self.time_manager.clone(),
            search_history: self.search_history.clone(),
            ponder: None,
            last_nodes: 0,
            local_nodes: 0,
            rep_floor: self.rep_floor,
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Search workers are clones: make their remaining nodes visible to the parent.
        self.flush_nodes();
        self.stop_ponder();
    }
}

impl Engine {
    pub fn new(depth: u32) -> Self {
        Self::with_threads(depth, 1)
    }

    pub fn with_threads(depth: u32, threads: usize) -> Self {
        Self::with_threads_and_table(depth, threads, TABLE_SIZE)
    }

    pub fn with_threads_and_table(depth: u32, threads: usize, table_size: usize) -> Self {
        Self {
            depth,
            threads,
            tt: Table::new(table_size.max(1)),
            killers: vec![[None, None]; MAX_PLY],
            quiet_history: [[0; 64]; 64],
            capture_history: [[0; 64]; 64],
            cont_history: vec![0; CONT_HISTORY_SIZE],
            tb: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            time_manager: None,
            search_history: Vec::new(),
            ponder: None,
            last_nodes: 0,
            local_nodes: 0,
            rep_floor: 0,
        }
    }

    pub fn set_threads(&mut self, threads: usize) {
        self.threads = threads;
    }

    pub fn from_env(default_depth: u32, default_threads: usize) -> Self {
        let depth = env::var("CHESSMIND_DEPTH")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(default_depth);
        let threads = env::var("CHESSMIND_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(default_threads);
        let tt_size = env::var("CHESSMIND_TT_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(TABLE_SIZE);
        Self::with_threads_and_table(depth, threads, tt_size)
    }

    pub fn load_syzygy(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut tb = Tablebase::new();
        tb.add_directory(path)?;
        self.tb = Some(Arc::new(tb));
        Ok(())
    }

    pub fn load_syzygy_from_env(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        if let Ok(path) = env::var("SYZYGY_PATH") {
            self.load_syzygy(&path)?;
            return Ok(Some(path));
        }
        Ok(None)
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
    }

    fn reset_stop(&mut self) {
        self.stop_flag = Arc::new(AtomicBool::new(false));
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

    fn current_ponder_move_internal(&self) -> Option<Move> {
        self.ponder.as_ref().map(|state| state.predicted_move)
    }

    fn predict_ponder_move(&self, game: &Game) -> Option<Move> {
        let root_hash = game.board.hash(game.current_turn);
        if let Some(entry) = self.tt.get(root_hash) {
            if let Some(mv) = self.find_tt_move(&game.board, game.current_turn, entry.best) {
                return Some(mv);
            }
        }

        self.ordered_root_moves(&game.board, game.current_turn)
            .into_iter()
            .next()
    }

    fn search_with_current_stop_flag(
        &mut self,
        game: &mut Game,
        config: &TimeConfig,
        allow_book: bool,
    ) -> Option<((String, String), u32)> {
        self.tt.next_age();

        if allow_book {
            if let Some(book_mv) = book_move(&game.history, &game.board, game.current_turn) {
                return Some((book_mv, 0));
            }
        }

        let max_depth = config.depth.unwrap_or(MAX_DEPTH).min(MAX_DEPTH);
        let time_manager = TimeManager::new(config, game.current_turn, self.stop_flag.clone());
        self.time_manager = Some(Arc::new(time_manager));

        let result = if self.threads <= 1 || !Self::should_use_parallel_search(config, max_depth) {
            self.best_move_single(game, max_depth)
        } else {
            self.best_move_parallel(game, max_depth)
        };

        self.flush_nodes();
        self.last_nodes = self.time_manager.as_ref().map_or(0, |tm| tm.nodes());
        self.time_manager = None;
        result
    }

    /// Number of nodes visited by the most recent search.
    pub fn last_search_nodes(&self) -> u64 {
        self.last_nodes
    }

    fn ponder_matches(&self, game: &Game, actual_move: Move) -> bool {
        let Some(state) = &self.ponder else {
            return false;
        };

        state.root_hash == game.board.hash(game.current_turn) && state.predicted_move == actual_move
    }

    /// Returns the currently predicted opponent move, if a ponder search is active.
    pub fn current_ponder_move(&self) -> Option<(String, String)> {
        self.current_ponder_move_internal()
            .map(Self::move_to_strings)
    }

    /// Reports whether a background ponder search is currently running.
    pub fn is_pondering(&self) -> bool {
        self.ponder.is_some()
    }

    /// Starts a background ponder on a single predicted reply from the current position.
    pub fn start_ponder(&mut self, game: &Game) -> Option<(String, String)> {
        self.stop_ponder();

        let predicted_move = self.predict_ponder_move(game)?;
        let root_hash = game.board.hash(game.current_turn);
        let ponder_stop = Arc::new(AtomicBool::new(false));
        let mut ponder_engine = self.clone();
        let (start, end) = Self::move_to_strings(predicted_move);
        let mut ponder_game = Self::clone_game(game);

        if !ponder_game.make_move(&start, &end) {
            return None;
        }

        ponder_engine.stop_flag = ponder_stop.clone();
        ponder_engine.time_manager = None;

        let handle = thread::spawn(move || {
            let _ = ponder_engine.search_with_current_stop_flag(
                &mut ponder_game,
                &TimeConfig::infinite(),
                false,
            );
        });

        self.ponder = Some(PonderState {
            root_hash,
            predicted_move,
            stop_flag: ponder_stop,
            handle,
        });

        Some((start, end))
    }

    /// Stops the background ponder search, if any, and waits for its thread to exit.
    pub fn stop_ponder(&mut self) {
        let Some(state) = self.ponder.take() else {
            return;
        };

        state.stop_flag.store(true, Ordering::Release);
        let _ = state.handle.join();
    }

    /// Validates the played move against the current ponder prediction, then stops ponder cleanly.
    pub fn ponder_hit(&mut self, game: &Game, start: &str, end: &str) -> bool {
        let hit = game
            .board
            .encode_move(start, end, game.current_turn)
            .is_some_and(|mv| self.ponder_matches(game, mv));
        self.stop_ponder();
        hit
    }

    fn generate_legal_moves(&self, board: &mut Board, color: Color) -> crate::types::MoveList {
        let mut list = crate::types::MoveList::new();
        crate::movegen::generate_moves_fast(board, color, &mut list);
        list
    }

    fn generate_in_check_moves(&self, board: &mut Board, color: Color) -> crate::types::MoveList {
        let mut list = crate::types::MoveList::new();
        crate::movegen::generate_evasions_fast(board, color, &mut list);
        list
    }

    fn generate_quiescence_moves(
        &self,
        board: &mut Board,
        color: Color,
        in_check: bool,
    ) -> crate::types::MoveList {
        let mut list = crate::types::MoveList::new();
        if in_check {
            crate::movegen::generate_evasions_fast(board, color, &mut list);
        } else {
            crate::movegen::generate_captures_fast(board, color, &mut list);
        }
        list
    }

    #[inline(always)]
    fn evaluate(board: &Board, color: Color) -> i32 {
        crate::eval::evaluate(board, color)
    }

    #[inline(always)]
    fn static_exchange_eval(&self, board: &Board, mv: Move) -> i32 {
        crate::see::static_exchange_eval(board, mv)
    }

    #[inline(always)]
    fn lmr_value(depth: u32, idx: usize) -> u32 {
        LMR_TABLE[(depth as usize).min(63)][idx.min(63)] as u32
    }

    /// FEN for the tablebase probe, with the real halfmove clock
    /// (`Board::to_fen` always ends with " 0 1").
    fn syzygy_fen(board: &Board, color: Color) -> String {
        let fen = board.to_fen(color);
        match fen.strip_suffix(" 0 1") {
            Some(base) => format!("{base} {} 1", board.halfmove),
            None => fen,
        }
    }

    fn probe_syzygy(&self, board: &Board, color: Color, ply: usize) -> Option<i32> {
        let tb = self.tb.as_ref()?;
        if board.piece_count_all() > tb.max_pieces() {
            return None;
        }
        let pos: Chess = Self::syzygy_fen(board, color)
            .parse::<Fen>()
            .ok()?
            .into_position(CastlingMode::Standard)
            .ok()?;
        let wdl = tb.probe_wdl(&pos).ok()?.after_zeroing();
        Some(match wdl {
            Wdl::Win | Wdl::CursedWin => MATE_VALUE - ply as i32,
            Wdl::Loss | Wdl::BlessedLoss => -MATE_VALUE + ply as i32,
            Wdl::Draw => 0,
        })
    }

    /// Ordering score of a move (without the stage), as used by `classify_move`.
    #[cfg(test)]
    fn move_score(&self, board: &Board, mv: Move, ply: usize, prev: Option<&Move>) -> i32 {
        let cont_base = Self::cont_history_base(board, prev);
        self.classify_move(board, mv, ply, cont_base, None).1
    }

    /// Offset of the continuation-history row selected by `prev`, the move that led to
    /// `board`. Computed once per node; `None` when there is no usable previous move.
    #[inline(always)]
    fn cont_history_base(board: &Board, prev: Option<&Move>) -> Option<usize> {
        let prev_to = prev?.to_sq() as usize;
        let prev_piece = board.piece_type_idx_at(prev_to as u8);
        if prev_piece >= 6 {
            return None;
        }
        Some(Self::cont_history_index(prev_piece, prev_to, 0, 0))
    }

    #[inline(always)]
    fn cont_history_slot(board: &Board, base: usize, mv: Move) -> Option<usize> {
        let curr_piece = board.piece_type_idx_at(mv.from_sq());
        if curr_piece >= 6 {
            return None;
        }
        Some(base + curr_piece * CONT_HISTORY_SQUARES + mv.to_sq() as usize)
    }

    #[inline(always)]
    fn continuation_history_score(
        cont_history: &[i32],
        board: &Board,
        base: usize,
        mv: Move,
    ) -> i32 {
        Self::cont_history_slot(board, base, mv).map_or(0, |idx| cont_history[idx])
    }

    #[inline(always)]
    fn history_bonus(depth: u32) -> i32 {
        let d = depth.min(MAX_DEPTH) as i32;
        (16 * d * d + 32 * d).min(1_200)
    }

    /// Gravity update: keeps the entry within +-HISTORY_MAX and makes large values
    /// harder to push further in the same direction.
    #[inline(always)]
    fn apply_history(entry: &mut i32, delta: i32) {
        *entry += delta - *entry * delta.abs() / HISTORY_MAX;
    }

    #[inline(always)]
    fn update_continuation_history(
        cont_history: &mut [i32],
        board: &Board,
        base: usize,
        mv: Move,
        delta: i32,
    ) {
        if let Some(idx) = Self::cont_history_slot(board, base, mv) {
            Self::apply_history(&mut cont_history[idx], delta);
        }
    }

    /// History update after a beta cutoff by `best`: reward it and penalise the moves of
    /// the same kind that were searched before it without producing the cutoff.
    fn update_histories_on_cutoff(
        &mut self,
        board: &Board,
        depth: u32,
        best: Move,
        cont_base: Option<usize>,
        quiets_tried: &[Move],
        captures_tried: &[Move],
    ) {
        let bonus = Self::history_bonus(depth);

        if best.is_capture() {
            let (from, to) = (best.from_sq() as usize, best.to_sq() as usize);
            Self::apply_history(&mut self.capture_history[from][to], bonus);
        } else if !best.is_promotion() {
            let (from, to) = (best.from_sq() as usize, best.to_sq() as usize);
            Self::apply_history(&mut self.quiet_history[from][to], bonus);
            if let Some(base) = cont_base {
                Self::update_continuation_history(&mut self.cont_history, board, base, best, bonus);
            }
            for &mv in quiets_tried {
                let (from, to) = (mv.from_sq() as usize, mv.to_sq() as usize);
                Self::apply_history(&mut self.quiet_history[from][to], -bonus);
                if let Some(base) = cont_base {
                    Self::update_continuation_history(
                        &mut self.cont_history,
                        board,
                        base,
                        mv,
                        -bonus,
                    );
                }
            }
        }

        for &mv in captures_tried {
            let (from, to) = (mv.from_sq() as usize, mv.to_sq() as usize);
            Self::apply_history(&mut self.capture_history[from][to], -bonus);
        }
    }

    /// Called at the start of each search: keep what was learned, but let it decay.
    fn age_histories(&mut self) {
        for row in self.quiet_history.iter_mut() {
            for v in row.iter_mut() {
                *v /= 2;
            }
        }
        for row in self.capture_history.iter_mut() {
            for v in row.iter_mut() {
                *v /= 2;
            }
        }
        for v in self.cont_history.iter_mut() {
            *v /= 2;
        }
        for k in self.killers.iter_mut() {
            *k = [None, None];
        }
    }

    #[inline(always)]
    fn cont_history_index(
        prev_piece: usize,
        prev_to: usize,
        curr_piece: usize,
        curr_to: usize,
    ) -> usize {
        debug_assert!(prev_piece < CONT_HISTORY_PIECE_TYPES);
        debug_assert!(prev_to < CONT_HISTORY_SQUARES);
        debug_assert!(curr_piece < CONT_HISTORY_PIECE_TYPES);
        debug_assert!(curr_to < CONT_HISTORY_SQUARES);

        (((prev_piece * CONT_HISTORY_SQUARES + prev_to) * CONT_HISTORY_PIECE_TYPES + curr_piece)
            * CONT_HISTORY_SQUARES)
            + curr_to
    }

    #[inline(always)]
    fn score_to_tt(score: i32, ply: usize) -> i32 {
        if score >= MATE_VALUE - MAX_PLY as i32 {
            score + ply as i32
        } else if score <= -MATE_VALUE + MAX_PLY as i32 {
            score - ply as i32
        } else {
            score
        }
    }

    #[inline(always)]
    fn score_from_tt(score: i32, ply: usize) -> i32 {
        if score >= MATE_VALUE - MAX_PLY as i32 {
            score - ply as i32
        } else if score <= -MATE_VALUE + MAX_PLY as i32 {
            score + ply as i32
        } else {
            score
        }
    }

    #[inline(always)]
    fn tt_move_matches(mv: Move, tt_best: Option<Move>) -> bool {
        tt_best == Some(mv)
    }

    #[inline(always)]
    fn promotion_bonus(mv: Move) -> i32 {
        match mv.promotion_piece() {
            Some(PieceType::Queen) => 40_000,
            Some(PieceType::Rook) => 35_000,
            Some(PieceType::Bishop) => 30_000,
            Some(PieceType::Knight) => 30_000,
            None => 0,
            _ => 0,
        }
    }

    /// Counts earlier occurrences of `hash`, the position about to be pushed onto
    /// `search_history` at `ply`. Returns 2 (a draw) on the second occurrence, or
    /// on the first one when it lies strictly inside the search tree (fewer than
    /// `ply` plies back). Only positions with the same side to move (every second
    /// entry) at least four plies back and after the last irreversible move
    /// (`rep_floor`) can match.
    #[inline(always)]
    fn repetition_count(&self, hash: u64, ply: usize) -> usize {
        let history = &self.search_history;
        let len = history.len();
        let floor = self.rep_floor;
        let mut count = 0;
        let Some(mut idx) = len.checked_sub(4) else {
            return 0;
        };
        while idx >= floor {
            if history[idx] == hash {
                count += 1;
                if count >= 2 || len - idx < ply {
                    return 2;
                }
            }
            if idx < 2 {
                break;
            }
            idx -= 2;
        }
        count
    }

    /// Whether `prev` (the move that reached `board`) makes every earlier position
    /// unrepeatable: captures, promotions, pawn moves and null moves (`None`).
    #[inline(always)]
    fn is_irreversible(board: &Board, prev: Option<Move>) -> bool {
        match prev {
            None => true,
            Some(mv) => {
                mv.is_capture() || mv.is_promotion() || board.piece_type_idx_at(mv.to_sq()) == 0
            }
        }
    }

    #[inline(always)]
    fn is_repetition_draw(&self, hash: u64, ply: usize) -> bool {
        self.repetition_count(hash, ply) >= 2
    }

    fn classify_move(
        &self,
        board: &Board,
        mv: Move,
        ply: usize,
        cont_base: Option<usize>,
        tt_best: Option<Move>,
    ) -> (u8, i32) {
        if Self::tt_move_matches(mv, tt_best) {
            return (ORDER_TT, i32::MAX);
        }

        let from = mv.from_sq() as usize;
        let to = mv.to_sq() as usize;
        let mut score = match cont_base {
            Some(base) => Self::continuation_history_score(&self.cont_history, board, base, mv),
            None => 0,
        };

        if mv.is_capture() {
            score += self.capture_history[from][to];

            let victim_idx = if mv.is_ep() {
                0 // Pawn
            } else {
                board.piece_type_idx_at(to as u8)
            };
            let attacker_idx = board.piece_type_idx_at(from as u8);
            if victim_idx < 6 && attacker_idx < 6 {
                score += mvv_lva_score(victim_idx, attacker_idx) * 100;
            }

            let see = self.static_exchange_eval(board, mv);
            if see < 0 {
                score -= 1000;
            }

            if mv.is_promotion() {
                return (ORDER_PROMOTION, score + Self::promotion_bonus(mv));
            }

            score += see * 128;
            let stage = if see >= 0 {
                ORDER_GOOD_CAPTURE
            } else {
                ORDER_BAD_CAPTURE
            };
            return (stage, score);
        }

        score += self.quiet_history[from][to];

        if mv.is_promotion() {
            return (ORDER_PROMOTION, score + Self::promotion_bonus(mv));
        }

        if let Some(k) = self.killers.get(ply) {
            if k[0] == Some(mv) {
                return (ORDER_KILLER_1, score + 30_000);
            }
            if k[1] == Some(mv) {
                return (ORDER_KILLER_2, score + 24_000);
            }
        }

        (ORDER_QUIET, score)
    }

    /// Packs (stage, score) into one key: larger key = searched earlier.
    #[inline(always)]
    fn order_key(stage: u8, score: i32) -> i64 {
        (((ORDER_BAD_CAPTURE - stage) as i64) << 32) | ((score as u32) ^ 0x8000_0000) as i64
    }

    #[inline(always)]
    fn key_score(key: i64) -> i32 {
        ((key as u32) ^ 0x8000_0000) as i32
    }

    #[inline(always)]
    fn key_stage(key: i64) -> u8 {
        ORDER_BAD_CAPTURE - (key >> 32) as u8
    }

    /// Scores every move into `buf` (only the first `moves.len()` slots are written).
    fn score_moves<'a>(
        &self,
        board: &Board,
        moves: &crate::types::MoveList,
        ply: usize,
        cont_base: Option<usize>,
        tt_best: Option<Move>,
        buf: &'a mut [std::mem::MaybeUninit<i64>; crate::types::MAX_MOVES],
    ) -> &'a mut [i64] {
        let len = moves.len();
        for idx in 0..len {
            let (stage, score) = self.classify_move(board, moves[idx], ply, cont_base, tt_best);
            buf[idx].write(Self::order_key(stage, score));
        }
        // SAFETY: the first `len` elements were initialised above.
        unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut i64, len) }
    }

    #[inline(always)]
    fn pick_next_move(moves: &mut crate::types::MoveList, keys: &mut [i64], start: usize) -> Move {
        let mut best = start;
        let mut best_key = keys[start];
        for idx in (start + 1)..keys.len() {
            let key = keys[idx];
            if key > best_key {
                best = idx;
                best_key = key;
            }
        }

        if best != start {
            moves.swap(start, best);
            keys.swap(start, best);
        }

        moves[start]
    }

    /// Returns the TT move if it is legal in `board` (guards against hash collisions).
    fn find_tt_move(&self, board: &Board, color: Color, best: Option<Move>) -> Option<Move> {
        let best = best?;
        let mut board_clone = board.clone();
        let moves = self.generate_legal_moves(&mut board_clone, color);
        moves.iter().copied().find(|mv| *mv == best)
    }

    /// Counts one node and reports whether the search must stop. The shared
    /// node counter and the clock are only touched every `NODE_FLUSH_INTERVAL` nodes.
    #[inline(always)]
    fn should_stop(&mut self) -> bool {
        if self.stop_flag.load(Ordering::Relaxed) {
            return true;
        }
        self.local_nodes += 1;
        if self.local_nodes >= NODE_FLUSH_INTERVAL {
            let batch = std::mem::take(&mut self.local_nodes);
            if let Some(tm) = &self.time_manager {
                tm.node_count.fetch_add(batch, Ordering::Relaxed);
                if tm.should_stop() {
                    self.stop_flag.store(true, Ordering::Release);
                    return true;
                }
            }
        }
        false
    }

    /// Adds the locally counted nodes to the shared counter.
    fn flush_nodes(&mut self) {
        if self.local_nodes == 0 {
            return;
        }
        if let Some(tm) = &self.time_manager {
            tm.node_count.fetch_add(self.local_nodes, Ordering::Relaxed);
        }
        self.local_nodes = 0;
    }

    #[inline(always)]
    fn quiescence(
        &mut self,
        board: &mut Board,
        color: Color,
        mut alpha: i32,
        beta: i32,
        ply: usize,
    ) -> i32 {
        if self.should_stop() {
            return 0;
        }

        // No repetition detection here: the entry position was checked by `pvs`,
        // and the side not in check only plays (irreversible) captures. The ply
        // cap bounds the one exception, endless mutual quiet check evasions.
        let in_check = board.in_check_fast(color);
        if ply >= MAX_PLY - 1 {
            return Self::evaluate(board, color);
        }

        let hash = board.hash(color);
        if let Some(entry) = self.tt.get(hash) {
            let value = Self::score_from_tt(entry.value, ply);
            match entry.bound {
                Bound::Exact => return value,
                Bound::Lower if value >= beta => return value,
                Bound::Upper if value <= alpha => return value,
                _ => {}
            }
        }

        let alpha_orig = alpha;
        let mut best_move: Option<Move> = None;
        let mut stand_pat = -MATE_VALUE;

        let result = 'q: {
            if !in_check {
                // Stalemate at the horizon: only checked when the side to move has
                // nothing but king and pawns, where it is both plausible and cheap.
                if !board.has_non_pawn_material(color) && !board.has_legal_move(color) {
                    break 'q 0;
                }
                stand_pat = Self::evaluate(board, color);

                if stand_pat >= beta {
                    break 'q stand_pat;
                }

                if stand_pat > alpha {
                    alpha = stand_pat;
                }

                const DELTA: i32 = 1000;
                if stand_pat + DELTA < alpha {
                    break 'q alpha;
                }
            }

            let mut moves = self.generate_quiescence_moves(board, color, in_check);
            if moves.is_empty() {
                if in_check {
                    -MATE_VALUE + ply as i32
                } else {
                    alpha
                }
            } else {
                let mut key_buf = [std::mem::MaybeUninit::<i64>::uninit(); crate::types::MAX_MOVES];
                let keys = self.score_moves(board, &moves, ply, None, None, &mut key_buf);

                let mut best = alpha;
                for idx in 0..moves.len() {
                    let m = Self::pick_next_move(&mut moves, keys, idx);

                    if !in_check
                        && !m.is_promotion()
                        && Self::key_stage(keys[idx]) == ORDER_BAD_CAPTURE
                    {
                        continue;
                    }
                    // Delta pruning: even winning the victim for free cannot reach alpha.
                    if !in_check && !m.is_promotion() {
                        let victim = if m.is_ep() {
                            0
                        } else {
                            board.piece_type_idx_at(m.to_sq())
                        };
                        let gain = QS_DELTA_VALUES.get(victim).copied().unwrap_or(0);
                        if stand_pat + gain + QS_DELTA_MARGIN <= best {
                            continue;
                        }
                    }

                    let undo = board.make_move_fast(m, color);
                    let score = -self.quiescence(board, opposite(color), -beta, -best, ply + 1);
                    board.unmake_move_fast(undo, color);

                    if self.stop_flag.load(Ordering::Relaxed) {
                        break 'q 0;
                    }

                    if score >= beta {
                        best_move = Some(m);
                        break 'q score;
                    }
                    if score > best {
                        best = score;
                        best_move = Some(m);
                    }
                }

                best
            }
        };

        if !self.stop_flag.load(Ordering::Relaxed) {
            let bound = if result >= beta {
                Bound::Lower
            } else if result > alpha_orig {
                Bound::Exact
            } else {
                Bound::Upper
            };
            self.tt.store(
                hash,
                TTEntry {
                    depth: 0,
                    value: Self::score_to_tt(result, ply),
                    bound,
                    best: best_move,
                },
            );
        }
        result
    }

    fn pvs(
        &mut self,
        board: &mut Board,
        color: Color,
        depth: u32,
        mut alpha: i32,
        mut beta: i32,
        ply: usize,
        prev_move: Option<Move>,
        use_iir: bool,
    ) -> i32 {
        if self.should_stop() {
            return 0;
        }

        let hash = board.hash(color);
        let saved_rep_floor = self.rep_floor;
        if ply > 0 && Self::is_irreversible(board, prev_move) {
            self.rep_floor = self.search_history.len();
        }
        if self.is_repetition_draw(hash, ply) {
            self.rep_floor = saved_rep_floor;
            return 0;
        }
        // Fifty-move rule: a draw, unless the side to move is checkmated.
        if ply > 0 && board.halfmove >= FIFTY_MOVE_PLIES {
            self.rep_floor = saved_rep_floor;
            if board.in_check_fast(color) && !board.has_legal_move(color) {
                return -MATE_VALUE + ply as i32;
            }
            return 0;
        }
        self.search_history.push(hash);

        let result = 'search: {
            let mate_max = MATE_VALUE - ply as i32;
            if beta > mate_max {
                beta = mate_max;
            }
            let mate_min = -mate_max;
            if alpha < mate_min {
                alpha = mate_min;
            }
            if alpha >= beta {
                break 'search alpha;
            }

            let is_pv = beta - alpha > 1;
            let alpha_orig = alpha;
            let mut tt_best: Option<Move> = None;

            if let Some(entry) = self.tt.get(hash) {
                let value = Self::score_from_tt(entry.value, ply);
                if entry.depth >= depth {
                    match entry.bound {
                        Bound::Exact => {
                            break 'search value;
                        }
                        Bound::Lower => alpha = alpha.max(value),
                        Bound::Upper => beta = beta.min(value),
                    }
                    if alpha >= beta {
                        break 'search value;
                    }
                }
                tt_best = entry.best;
            }

            // Never probe at the root (it must return a move) or at the horizon
            // (the FEN round-trip is too slow for leaf nodes).
            let tb_val = if ply > 0 && depth >= 1 && self.tb.is_some() {
                self.probe_syzygy(board, color, ply)
            } else {
                None
            };
            if let Some(tb_val) = tb_val {
                tb_val
            } else if depth == 0 {
                self.quiescence(board, color, alpha, beta, ply)
            } else {
                // Internal iterative reduction: without a TT move, ordering is poor
                // and the node is likely unimportant, so search it one ply shallower.
                let depth = if use_iir && tt_best.is_none() && depth >= 4 {
                    depth - 1
                } else {
                    depth
                };
                let in_check = board.in_check_fast(color);
                let static_eval = if in_check {
                    None
                } else {
                    Some(Self::evaluate(board, color))
                };

                if !is_pv && depth <= 3 && !in_check {
                    let eval = static_eval.unwrap();
                    if eval - RFP_MARGIN[depth as usize] >= beta {
                        break 'search eval;
                    }
                }

                // Null move: skipped right after another null move (`prev_move` is
                // None) and when the static eval does not already beat beta.
                let null_eval = static_eval.unwrap_or(-MATE_VALUE);
                let can_null = !is_pv
                    && !in_check
                    && prev_move.is_some()
                    && depth >= 3
                    && null_eval >= beta
                    && board.has_non_pawn_material(color);
                if can_null {
                    let r = 3 + depth / 4 + ((null_eval - beta) / 200).clamp(0, 3) as u32;
                    let ep = board.en_passant;

                    board.en_passant = None;
                    let score = -self.pvs(
                        board,
                        opposite(color),
                        depth.saturating_sub(1 + r),
                        -beta,
                        -beta + 1,
                        ply + 1,
                        None,
                        false,
                    );
                    board.en_passant = ep;

                    if self.stop_flag.load(Ordering::Relaxed) {
                        break 'search 0;
                    }

                    if score >= beta {
                        // Do not trust unproven mate scores from a null-move search.
                        let score = if score >= MATE_VALUE - MAX_PLY as i32 {
                            beta
                        } else {
                            score
                        };
                        if depth > 8 {
                            let verify = self.pvs(
                                board,
                                color,
                                depth.saturating_sub(1 + r),
                                beta - 1,
                                beta,
                                ply,
                                prev_move,
                                false,
                            );
                            if verify >= beta {
                                break 'search verify;
                            }
                        } else {
                            break 'search score;
                        }
                    }
                }

                let mut moves_list = if in_check {
                    self.generate_in_check_moves(board, color)
                } else {
                    self.generate_legal_moves(board, color)
                };
                if moves_list.is_empty() {
                    if in_check {
                        break 'search -MATE_VALUE + ply as i32;
                    }
                    break 'search 0;
                }

                let mut key_buf = [std::mem::MaybeUninit::<i64>::uninit(); crate::types::MAX_MOVES];
                let cont_base = Self::cont_history_base(board, prev_move.as_ref());
                let keys =
                    self.score_moves(board, &moves_list, ply, cont_base, tt_best, &mut key_buf);

                let mut best_move: Option<Move> = None;
                let mut best_score = -MATE_VALUE;
                let mut skip_quiets = false;
                let mut quiets_tried = [Move::NONE; MAX_TRIED_QUIETS];
                let mut quiet_count = 0usize;
                let mut captures_tried = [Move::NONE; MAX_TRIED_CAPTURES];
                let mut capture_count = 0usize;

                for idx in 0..moves_list.len() {
                    let m = Self::pick_next_move(&mut moves_list, keys, idx);

                    let capture = m.is_capture();
                    let promotion = m.is_promotion();
                    let is_quiet = !capture && !promotion;

                    // Quiet-move pruning (late move pruning, history pruning, futility).
                    // Quiet checks are never pruned: they are exactly the moves the
                    // static eval and the history tables misjudge. The first move
                    // (the TT move when there is one) is always searched.
                    if idx > 0 && !is_pv && !in_check && is_quiet {
                        let late = depth <= 4 && idx >= LMP_LIMITS[depth as usize];
                        let bad_history = depth <= HLP_THRESHOLD
                            && Self::key_score(keys[idx]) < HLP_BASE * depth as i32;
                        let futile = depth <= 4
                            && static_eval.is_some_and(|eval| {
                                eval + FUTILITY_MARGIN[depth as usize] <= alpha
                            });
                        if bad_history {
                            skip_quiets = true;
                        }
                        if (late || skip_quiets || futile) && !board.gives_check(m, color) {
                            continue;
                        }
                    }

                    let undo = board.make_move_fast(m, color);
                    let gives_check = board.in_check_fast(opposite(color));

                    let mut new_depth = depth - 1;
                    if gives_check && depth < MAX_DEPTH - 1 {
                        new_depth = new_depth.saturating_add(1);
                    }

                    let can_reduce =
                        !is_pv && depth > 2 && is_quiet && !in_check && !gives_check && idx >= 3;
                    let reduced_depth = if can_reduce {
                        new_depth.saturating_sub(Self::lmr_value(depth, idx + 1))
                    } else {
                        new_depth
                    };

                    let mut score;
                    if idx == 0 {
                        score = -self.pvs(
                            board,
                            opposite(color),
                            new_depth,
                            -beta,
                            -alpha,
                            ply + 1,
                            Some(m),
                            true,
                        );
                    } else {
                        // Null-window search, reduced for late quiet moves.
                        score = -self.pvs(
                            board,
                            opposite(color),
                            reduced_depth,
                            -alpha - 1,
                            -alpha,
                            ply + 1,
                            Some(m),
                            true,
                        );
                        // A reduced move that beats alpha is verified at full depth.
                        if score > alpha && reduced_depth < new_depth {
                            score = -self.pvs(
                                board,
                                opposite(color),
                                new_depth,
                                -alpha - 1,
                                -alpha,
                                ply + 1,
                                Some(m),
                                true,
                            );
                        }
                        // Inside a PV window, a move that beats alpha needs its exact score.
                        if score > alpha && score < beta {
                            score = -self.pvs(
                                board,
                                opposite(color),
                                new_depth,
                                -beta,
                                -alpha,
                                ply + 1,
                                Some(m),
                                true,
                            );
                        }
                    }

                    board.unmake_move_fast(undo, color);

                    if self.stop_flag.load(Ordering::Relaxed) {
                        break 'search 0;
                    }

                    if score >= beta {
                        if is_quiet {
                            if self.killers.len() <= ply {
                                self.killers.resize(ply + 1, [None, None]);
                            }
                            let k = &mut self.killers[ply];
                            if k[0] != Some(m) {
                                k[1] = k[0];
                                k[0] = Some(m);
                            }
                        }

                        self.update_histories_on_cutoff(
                            board,
                            depth,
                            m,
                            cont_base,
                            &quiets_tried[..quiet_count],
                            &captures_tried[..capture_count],
                        );

                        self.tt.store(
                            hash,
                            TTEntry {
                                depth,
                                value: Self::score_to_tt(score, ply),
                                bound: Bound::Lower,
                                best: Some(m),
                            },
                        );

                        break 'search score;
                    } else if is_quiet {
                        if quiet_count < MAX_TRIED_QUIETS {
                            quiets_tried[quiet_count] = m;
                            quiet_count += 1;
                        }
                    } else if capture && capture_count < MAX_TRIED_CAPTURES {
                        captures_tried[capture_count] = m;
                        capture_count += 1;
                    }

                    if score > best_score {
                        best_score = score;
                    }
                    if score > alpha {
                        alpha = score;
                        best_move = Some(m);
                    }
                }

                let bound = if alpha <= alpha_orig {
                    Bound::Upper
                } else {
                    Bound::Exact
                };

                let tt_value = if alpha <= alpha_orig {
                    best_score.max(alpha)
                } else {
                    alpha
                };

                self.tt.store(
                    hash,
                    TTEntry {
                        depth,
                        value: Self::score_to_tt(tt_value, ply),
                        bound,
                        best: best_move,
                    },
                );

                alpha
            }
        };

        self.search_history.pop();
        self.rep_floor = saved_rep_floor;
        result
    }

    fn ordered_root_moves(&self, board: &Board, color: Color) -> Vec<Move> {
        let root_hash = board.hash(color);
        let tt_best = self.tt.get(root_hash).and_then(|entry| entry.best);
        let mut board_clone = board.clone();
        let mut moves = if board_clone.in_check_fast(color) {
            self.generate_in_check_moves(&mut board_clone, color)
        } else {
            self.generate_legal_moves(&mut board_clone, color)
        };

        if moves.is_empty() {
            return Vec::new();
        }

        let mut key_buf = [std::mem::MaybeUninit::<i64>::uninit(); crate::types::MAX_MOVES];
        let keys = self.score_moves(board, &moves, 0, None, tt_best, &mut key_buf);

        let mut ordered = Vec::with_capacity(moves.len());
        for idx in 0..moves.len() {
            let mv = Self::pick_next_move(&mut moves, keys, idx);
            ordered.push(mv);
        }

        ordered
    }

    fn root_child_depth(depth: u32, gives_check: bool) -> u32 {
        let mut child_depth = depth.saturating_sub(1);
        if gives_check && depth < MAX_DEPTH - 1 {
            child_depth = child_depth.saturating_add(1);
        }
        child_depth
    }

    fn search_root_move(
        &mut self,
        board: &Board,
        color: Color,
        depth: u32,
        mv: Move,
        alpha: i32,
        beta: i32,
    ) -> Option<i32> {
        if self.stop_flag.load(Ordering::Relaxed) {
            return None;
        }

        let mut board = board.clone();
        // `search_history` ends just before the root (as for `pvs` at ply 0): push the
        // root so the child at ply 1 sees the same history as in the single-thread search.
        self.search_history.push(board.hash(color));
        let undo = board.make_move_fast(mv, color);
        let gives_check = board.in_check_fast(opposite(color));
        let child_depth = Self::root_child_depth(depth, gives_check);
        let score = -self.pvs(
            &mut board,
            opposite(color),
            child_depth,
            -beta,
            -alpha,
            1,
            Some(mv),
            true,
        );
        board.unmake_move_fast(undo, color);
        self.search_history.pop();

        if self.stop_flag.load(Ordering::Relaxed) {
            None
        } else {
            Some(score)
        }
    }

    /// Root search split across `workers` (kept alive across iterations so their
    /// histories accumulate). The first (PV) move is searched alone, inside an aspiration
    /// window around `guess` when available; the remaining moves are searched in parallel
    /// with a null window around the shared best score and re-searched on fail high.
    fn search_root_parallel(
        workers: &mut [Engine],
        board: &Board,
        color: Color,
        depth: u32,
        guess: Option<i32>,
    ) -> Option<(Move, i32)> {
        const ASPIRATION: i32 = 40;
        let root_moves = workers.first()?.ordered_root_moves(board, color);
        let (&first, rest) = root_moves.split_first()?;

        let lead = &mut workers[0];
        let mut first_score = None;
        if let Some(g) = guess.filter(|_| depth >= 5) {
            let (lo, hi) = (g - ASPIRATION, g + ASPIRATION);
            let score = lead.search_root_move(board, color, depth, first, lo, hi)?;
            if score > lo && score < hi {
                first_score = Some(score);
            }
        }
        let first_score = match first_score {
            Some(score) => score,
            None => lead.search_root_move(board, color, depth, first, -MATE_VALUE, MATE_VALUE)?,
        };
        if rest.is_empty() {
            return Some((first, first_score));
        }

        let shared_alpha = std::sync::atomic::AtomicI32::new(first_score);
        let next_index = AtomicUsize::new(0);
        let worker_count = workers.len().min(rest.len());
        let (tx, rx) = mpsc::channel();

        thread::scope(|scope| {
            for worker in workers.iter_mut().take(worker_count) {
                let tx = tx.clone();
                let shared_alpha = &shared_alpha;
                let next_index = &next_index;

                scope.spawn(move || {
                    while !worker.stop_flag.load(Ordering::Relaxed) {
                        let offset = next_index.fetch_add(1, Ordering::Relaxed);
                        let Some(&mv) = rest.get(offset) else {
                            break;
                        };

                        let alpha = shared_alpha.load(Ordering::Relaxed);
                        let Some(mut score) =
                            worker.search_root_move(board, color, depth, mv, alpha, alpha + 1)
                        else {
                            break;
                        };
                        let mut exact = false;
                        if score > alpha {
                            let alpha = shared_alpha.load(Ordering::Relaxed);
                            let Some(full) =
                                worker.search_root_move(board, color, depth, mv, alpha, MATE_VALUE)
                            else {
                                break;
                            };
                            score = full;
                            if score > alpha {
                                exact = true;
                                shared_alpha.fetch_max(score, Ordering::Relaxed);
                            }
                        }

                        let result = RootSearchResult {
                            index: offset + 1,
                            mv,
                            score,
                            exact,
                        };
                        if tx.send(result).is_err() {
                            break;
                        }
                    }
                });
            }

            drop(tx);

            let mut completed = 0usize;
            let mut best = RootSearchResult {
                index: 0,
                mv: first,
                score: first_score,
                exact: true,
            };

            while let Ok(result) = rx.recv() {
                completed += 1;
                if result.exact
                    && (result.score > best.score
                        || (result.score == best.score && result.index < best.index))
                {
                    best = result;
                }
            }

            if completed == rest.len() {
                Some((best.mv, best.score))
            } else {
                None
            }
        })
    }

    pub fn best_move_timed(
        &mut self,
        game: &mut Game,
        config: &TimeConfig,
    ) -> Option<((String, String), u32)> {
        self.stop_ponder();
        self.reset_stop();
        self.search_with_current_stop_flag(game, config, true)
    }

    /// Like `best_move_timed`, but lets the caller disable the opening book and
    /// supply the stop flag, so another thread (e.g. a UCI `stop`) can end the search.
    pub fn best_move_timed_opts(
        &mut self,
        game: &mut Game,
        config: &TimeConfig,
        allow_book: bool,
        stop_flag: Arc<AtomicBool>,
    ) -> Option<((String, String), u32)> {
        self.stop_ponder();
        self.stop_flag = stop_flag;
        self.search_with_current_stop_flag(game, config, allow_book)
    }

    fn should_use_parallel_search(config: &TimeConfig, max_depth: u32) -> bool {
        if let Some(depth) = config.depth {
            return depth >= 9 && max_depth >= 9;
        }

        if let Some(movetime) = config.movetime {
            return movetime >= 1_000;
        }

        if config.infinite {
            return true;
        }

        let remaining = config
            .wtime
            .into_iter()
            .chain(config.btime)
            .max()
            .unwrap_or(0);

        remaining >= 60_000
    }

    pub fn best_move(&mut self, game: &mut Game) -> Option<(String, String)> {
        let config = TimeConfig::fixed_depth(self.depth);
        self.best_move_timed(game, &config).map(|(m, _)| m)
    }

    fn best_move_single(
        &mut self,
        game: &mut Game,
        max_depth: u32,
    ) -> Option<((String, String), u32)> {
        const ASPIRATION: i32 = 50;
        let color = game.current_turn;
        let root_hash = game.board.hash(color);
        let mut guess = 0;
        let mut best_move: Option<Move> = None;
        let mut reached_depth = 0;
        let mut last_iteration_ms: Option<u64> = None;
        let mut last_score: Option<i32> = None;
        let mut prev_score: Option<i32> = None;
        let mut last_best_move: Option<Move> = None;
        let mut prev_best_move: Option<Move> = None;

        self.search_history = game.hash_history.clone();
        self.search_history.pop();
        self.age_histories();

        for d in 1..=max_depth {
            if let Some(ref tm) = self.time_manager {
                if d > 1
                    && !tm.should_continue_iterating(
                        d - 1,
                        last_iteration_ms,
                        last_score.unwrap_or(guess),
                        prev_score,
                        last_best_move,
                        prev_best_move,
                    )
                {
                    break;
                }
            }

            let iteration_start_ms = self
                .time_manager
                .as_ref()
                .map(|tm| tm.elapsed_ms())
                .unwrap_or(0);
            let mut delta = ASPIRATION;
            let mut alpha = -MATE_VALUE;
            let mut beta = MATE_VALUE;
            if d > 1 {
                alpha = (guess - delta).max(-MATE_VALUE);
                beta = (guess + delta).min(MATE_VALUE);
            }

            loop {
                if self.stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                let mut board = game.board.clone();

                let score = self.pvs(&mut board, color, d, alpha, beta, 0, None, true);

                if self.stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                if score <= alpha {
                    delta = (delta * 2).min(MATE_VALUE);
                    alpha = (guess - delta).max(-MATE_VALUE);
                    beta = (guess + delta).min(MATE_VALUE);
                    continue;
                }
                if score >= beta {
                    delta = (delta * 2).min(MATE_VALUE);
                    alpha = (guess - delta).max(-MATE_VALUE);
                    beta = (guess + delta).min(MATE_VALUE);
                    continue;
                }

                guess = score;

                if let Some(entry) = self.tt.get(root_hash) {
                    if let Some(mv) = self.find_tt_move(&game.board, color, entry.best) {
                        best_move = Some(mv);
                    }
                }
                break;
            }
            reached_depth = d;

            if let Some(ref tm) = self.time_manager {
                let elapsed_ms = tm.elapsed_ms().saturating_sub(iteration_start_ms);
                last_iteration_ms = Some(elapsed_ms);
            }

            prev_score = last_score;
            last_score = Some(guess);
            prev_best_move = last_best_move;
            last_best_move = best_move;

            if self.stop_flag.load(Ordering::Relaxed) {
                break;
            }
        }

        best_move.map(|m| (Self::move_to_strings(m), reached_depth))
    }

    fn best_move_parallel(
        &mut self,
        game: &mut Game,
        max_depth: u32,
    ) -> Option<((String, String), u32)> {
        let color = game.current_turn;
        let root_hash = game.board.hash(color);
        let mut best_move: Option<Move> = None;
        let mut reached_depth = 0;
        let mut last_iteration_ms: Option<u64> = None;
        let mut last_score: Option<i32> = None;
        let mut prev_score: Option<i32> = None;
        let mut last_best_move: Option<Move> = None;
        let mut prev_best_move: Option<Move> = None;

        self.search_history = game.hash_history.clone();
        self.search_history.pop();
        self.age_histories();

        // Workers live for the whole search so their histories and killers carry over
        // between iterations; the first worker's tables are kept for the next search.
        let mut workers: Vec<Engine> = (0..self.threads.max(1)).map(|_| self.clone()).collect();

        for d in 1..=max_depth {
            if let Some(ref tm) = self.time_manager {
                if d > 1
                    && !tm.should_continue_iterating(
                        d - 1,
                        last_iteration_ms,
                        last_score.unwrap_or(0),
                        prev_score,
                        last_best_move,
                        prev_best_move,
                    )
                {
                    break;
                }
            }

            let iteration_start_ms = self
                .time_manager
                .as_ref()
                .map(|tm| tm.elapsed_ms())
                .unwrap_or(0);
            let result =
                Self::search_root_parallel(&mut workers, &game.board, color, d, last_score);
            let Some((mv, score)) = result else {
                break;
            };

            self.tt.store(
                root_hash,
                TTEntry {
                    depth: d,
                    value: Self::score_to_tt(score, 0),
                    bound: Bound::Exact,
                    best: Some(mv),
                },
            );

            best_move = Some(mv);
            reached_depth = d;
            prev_score = last_score;
            last_score = Some(score);
            prev_best_move = last_best_move;
            last_best_move = Some(mv);
            last_iteration_ms = self
                .time_manager
                .as_ref()
                .map(|tm| tm.elapsed_ms().saturating_sub(iteration_start_ms));

            if self.stop_flag.load(Ordering::Relaxed) {
                break;
            }
        }

        if let Some(lead) = workers.first_mut() {
            self.quiet_history = lead.quiet_history;
            self.capture_history = lead.capture_history;
            self.cont_history = std::mem::take(&mut lead.cont_history);
        }

        best_move.map(|m| (Self::move_to_strings(m), reached_depth))
    }

    fn move_to_strings(m: Move) -> (String, String) {
        let from =
            Board::index_to_algebraic((m.from_sq() % 8) as usize, (m.from_sq() / 8) as usize)
                .unwrap();
        let to =
            Board::index_to_algebraic((m.to_sq() % 8) as usize, (m.to_sq() / 8) as usize).unwrap();
        let mut to_string = to;

        if m.is_promotion() {
            if let Some(pt) = m.promotion_piece() {
                let promo = match pt {
                    PieceType::Queen => 'q',
                    PieceType::Rook => 'r',
                    PieceType::Bishop => 'b',
                    PieceType::Knight => 'n',
                    _ => 'q',
                };
                to_string.push(promo);
            }
        }

        (from, to_string)
    }
}

#[inline(always)]
fn opposite(c: Color) -> Color {
    match c {
        Color::White => Color::Black,
        Color::Black => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pieces::Piece;

    fn setup_game() -> Game {
        Game::new()
    }

    fn setup_ponder_position() -> Game {
        let mut game = Game::new();
        game.board = Board::new();
        game.current_turn = Color::White;
        game.history.clear();
        game.result = None;

        game.board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        game.board.set(
            "d1",
            Some(Piece {
                piece_type: PieceType::Queen,
                color: Color::White,
            }),
        );
        game.board.set(
            "h8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        game.board.set(
            "d8",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        game.board.set(
            "g7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        let hash = game.board.hash(game.current_turn);
        game.hash_history = vec![hash];
        game.hash_counts = std::collections::HashMap::from([(hash, 1usize)]);
        game
    }

    #[test]
    fn test_engine_returns_valid_move() {
        let mut game = setup_game();
        let mut engine = Engine::new(4);

        let result = engine.best_move(&mut game);
        assert!(
            result.is_some(),
            "Engine should return a move from starting position"
        );

        let (from, to) = result.unwrap();
        assert!(
            game.board.is_legal(&from, &to, Color::White),
            "Engine returned illegal move: {} -> {}",
            from,
            to
        );
    }

    #[test]
    fn test_engine_finds_obvious_capture() {
        let mut game = Game::new();
        game.board = Board::new();

        game.board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        game.board.set(
            "d1",
            Some(Piece {
                piece_type: PieceType::Queen,
                color: Color::White,
            }),
        );

        game.board.set(
            "h8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        game.board.set(
            "d8",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );

        let mut engine = Engine::new(3);
        let config = TimeConfig::fixed_depth(3);
        let result = engine.best_move_timed(&mut game, &config);

        assert!(result.is_some());
        let ((from, to), _depth) = result.unwrap();

        assert_eq!(from, "d1", "Queen should move from d1");
        assert_eq!(to, "d8", "Queen should capture rook on d8");
    }

    #[test]
    fn test_mate_in_one() {
        let mut game = Game::new();
        game.board = Board::new();

        game.board.set(
            "g1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        game.board.set(
            "a1",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::White,
            }),
        );
        game.board.set(
            "h8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        game.board.set(
            "g7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        game.board.set(
            "h7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        let mut engine = Engine::new(2);
        let config = TimeConfig::fixed_depth(2);
        let result = engine.best_move_timed(&mut game, &config);

        assert!(result.is_some());
        let ((from, to), _depth) = result.unwrap();

        assert_eq!(from, "a1", "Rook should move from a1");
        assert_eq!(to, "a8", "Rook should deliver mate on a8");
    }

    #[test]
    fn test_fixed_depth_search() {
        let mut game = setup_game();
        let mut engine = Engine::new(3);

        let config = TimeConfig::fixed_depth(3);
        let result = engine.best_move_timed(&mut game, &config);

        assert!(result.is_some(), "Should return a move");
    }

    #[test]
    fn test_engine_evaluates_material() {
        let mut game = Game::new();
        game.board = Board::new();

        game.board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        game.board.set(
            "d1",
            Some(Piece {
                piece_type: PieceType::Queen,
                color: Color::White,
            }),
        );
        game.board.set(
            "e8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );

        let eval = Engine::evaluate(&game.board, Color::White);
        assert!(
            eval > 800,
            "White up a queen should have high eval, got {}",
            eval
        );
    }

    #[test]
    fn test_time_config_creation() {
        let fixed = TimeConfig::fixed_depth(5);
        assert_eq!(fixed.depth, Some(5));

        let timed = TimeConfig::fixed_time(1000);
        assert_eq!(timed.movetime, Some(1000));

        let infinite = TimeConfig::infinite();
        assert!(infinite.infinite);
    }

    #[test]
    fn test_time_budget_reserves_overhead_and_uses_increment() {
        let (soft_no_inc, hard_no_inc, overhead_no_inc, crisis_no_inc) =
            TimeManager::calculate_time(60_000, 0, None);
        let (soft_inc, hard_inc, overhead_inc, crisis_inc) =
            TimeManager::calculate_time(60_000, 2_000, None);

        assert_eq!(overhead_no_inc, 30);
        assert_eq!(overhead_inc, 20);
        assert!(!crisis_no_inc);
        assert!(!crisis_inc);
        assert!(
            soft_no_inc < 2_600,
            "soft budget should leave a safety buffer"
        );
        assert!(
            soft_inc > soft_no_inc,
            "increment should increase the practical search budget"
        );
        assert!(
            hard_inc > hard_no_inc,
            "increment should raise the hard limit"
        );
    }

    #[test]
    fn test_time_budget_enters_crisis_mode() {
        let (soft, hard, _, crisis) = TimeManager::calculate_time(800, 0, None);

        assert!(crisis);
        assert!(soft <= 30, "crisis mode should spend very little time");
        assert!(hard > soft);
    }

    #[test]
    fn test_only_hard_limit_aborts_an_iteration() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let config = TimeConfig {
            wtime: Some(10_000),
            ..Default::default()
        };
        let mut tm = TimeManager::new(&config, Color::White, stop_flag);
        tm.soft_time_ms = 100;
        tm.hard_time_ms = 1_000;
        tm.start_time = std::time::Instant::now() - std::time::Duration::from_millis(300);

        assert!(
            !tm.should_stop(),
            "past soft but before hard: keep searching"
        );
        assert!(
            !tm.should_continue_iterating(5, Some(50), 0, Some(0), None, None),
            "past soft: do not start another iteration"
        );

        tm.start_time = std::time::Instant::now() - std::time::Duration::from_millis(1_000);
        assert!(tm.should_stop(), "the hard limit aborts the iteration");
    }

    #[test]
    fn test_soft_limit_stretches_on_instability() {
        let stable = Move::normal(12, 28);
        let changed = Move::normal(11, 27);
        assert_eq!(
            TimeManager::soft_scale_pct(8, 10, Some(12), Some(stable), Some(stable)),
            100
        );
        assert_eq!(
            TimeManager::soft_scale_pct(8, 10, Some(12), Some(changed), Some(stable)),
            140
        );
        assert_eq!(
            TimeManager::soft_scale_pct(8, -60, Some(10), Some(changed), Some(stable)),
            180
        );
    }

    #[test]
    fn test_hard_limit_leaves_clock_margin() {
        for &(time, inc) in &[
            (200u64, 10u64),
            (1_000, 20),
            (300, 0),
            (5_000, 0),
            (60_000, 0),
            (60_000, 2_000),
            (1_000, 5_000),
        ] {
            let (soft, hard, overhead, _) = TimeManager::calculate_time(time, inc, None);
            assert!(soft < hard, "{time}+{inc}: soft {soft} hard {hard}");
            assert!(
                hard + overhead <= time.max(40),
                "{time}+{inc}: hard {hard} must stay within the clock"
            );
        }
    }

    #[test]
    fn test_iteration_gate_prefers_stable_positions_to_stop_earlier() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let config = TimeConfig {
            wtime: Some(10_000),
            ..Default::default()
        };
        let mut tm = TimeManager::new(&config, Color::White, stop_flag);

        tm.start_time = std::time::Instant::now() - std::time::Duration::from_millis(810);
        tm.soft_time_ms = 1_000;
        tm.hard_time_ms = 1_500;
        tm.move_overhead_ms = 20;
        tm.in_crisis = false;

        let stable_move = Move::normal(12, 28);
        let changed_move = Move::normal(11, 27);

        assert!(
            !tm.should_continue_iterating(
                4,
                Some(100),
                22,
                Some(20),
                Some(stable_move),
                Some(stable_move)
            ),
            "stable best moves should make us stop earlier when the remaining budget is thin"
        );
        assert!(
            tm.should_continue_iterating(
                4,
                Some(100),
                140,
                Some(20),
                Some(changed_move),
                Some(stable_move)
            ),
            "a score swing plus a move change should justify another iteration"
        );
    }

    #[test]
    fn test_engine_cloning() {
        let engine = Engine::new(5);
        let cloned = engine.clone();

        assert_eq!(engine.depth, cloned.depth);
        assert_eq!(engine.threads, cloned.threads);
    }

    #[test]
    fn test_generate_legal_moves_count() {
        let mut board = Board::new();
        board.setup_standard();

        let engine = Engine::new(1);
        let moves = engine.generate_legal_moves(&mut board, Color::White);

        assert_eq!(
            moves.len(),
            20,
            "Starting position should have 20 legal moves"
        );
    }

    #[test]
    fn test_continuation_history_table_uses_piece_and_target_indices() {
        let mut board = Board::new();
        board.setup_standard();

        let prev = Move::new(12, 28, Move::FLAG_DOUBLE_PUSH); // e2 -> e4
        let _undo = board.make_move_fast(prev, Color::White);
        let curr = Move::new(62, 45, Move::FLAG_NORMAL); // g8 -> f6

        let engine = Engine::new(1);
        let mut boosted = engine.clone();
        let idx = Engine::cont_history_index(0, 28, 1, 45);
        boosted.cont_history[idx] = 777;

        let base = engine.move_score(&board, curr, 0, Some(&prev));
        let boosted_score = boosted.move_score(&board, curr, 0, Some(&prev));

        assert_eq!(boosted_score - base, 777);
    }

    #[test]
    fn test_search_doesnt_hang() {
        let mut game = setup_game();
        let mut engine = Engine::new(4);

        let config = TimeConfig::fixed_depth(4);
        let start = std::time::Instant::now();
        let _result = engine.best_move_timed(&mut game, &config);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < 10,
            "Search took too long: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_transposition_table_usage() {
        let mut game = setup_game();
        let mut engine = Engine::new(3);

        let config = TimeConfig::fixed_depth(3);
        let result1 = engine.best_move_timed(&mut game, &config);

        let result2 = engine.best_move_timed(&mut game, &config);

        assert_eq!(result1, result2, "Same position should return same move");
    }

    #[test]
    fn test_transposition_table_consistency_with_forced_collisions() {
        let mut game = setup_game();
        let mut engine = Engine::with_threads_and_table(3, 1, 4);

        let config = TimeConfig::fixed_depth(3);
        let result1 = engine.best_move_timed(&mut game, &config);
        let result2 = engine.best_move_timed(&mut game, &config);

        assert_eq!(
            result1, result2,
            "Forced-collision TT should still produce stable repeated search results"
        );
    }

    #[test]
    fn test_start_ponder_predicts_legal_reply_and_stops_cleanly() {
        let mut game = setup_ponder_position();
        let mut engine = Engine::new(3);
        let config = TimeConfig::fixed_depth(3);
        let ((from, to), _) = engine.best_move_timed(&mut game, &config).unwrap();

        assert!(game.make_move(&from, &to));

        let predicted = engine
            .start_ponder(&game)
            .expect("ponder should predict one reply");

        assert!(engine.is_pondering());
        assert!(
            game.board
                .is_legal(&predicted.0, &predicted.1, game.current_turn),
            "predicted ponder move should be legal"
        );

        engine.stop_ponder();
        assert!(!engine.is_pondering());
        assert_eq!(engine.current_ponder_move(), None);
    }

    #[test]
    fn test_ponder_hit_and_miss_stop_background_search() {
        let mut game = Game::new();
        assert!(game.make_move("e2", "e4"));

        let mut engine = Engine::new(3);

        let predicted = engine
            .start_ponder(&game)
            .expect("ponder should predict one legal black reply");
        assert!(engine.ponder_hit(&game, &predicted.0, &predicted.1));
        assert!(!engine.is_pondering());

        let predicted = engine
            .start_ponder(&game)
            .expect("ponder should restart after a clean stop");
        let alternative = game
            .legal_moves()
            .into_iter()
            .find(|mv| mv != &predicted)
            .expect("test position should have a second legal reply");

        assert!(!engine.ponder_hit(&game, &alternative.0, &alternative.1));
        assert!(!engine.is_pondering());
    }

    #[test]
    fn test_parallel_search_matches_single_thread_on_forcing_position() {
        let mut single_game = Game::new();
        single_game.board = Board::new();
        single_game.board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        single_game.board.set(
            "d1",
            Some(Piece {
                piece_type: PieceType::Queen,
                color: Color::White,
            }),
        );
        single_game.board.set(
            "h8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        single_game.board.set(
            "d8",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );

        let mut parallel_game = Game::new();
        parallel_game.board = single_game.board.clone();

        let config = TimeConfig::fixed_depth(3);
        let mut single = Engine::with_threads(3, 1);
        let mut parallel = Engine::with_threads(3, 4);

        let single_result = single.best_move_timed(&mut single_game, &config);
        let parallel_result = parallel.best_move_timed(&mut parallel_game, &config);

        assert_eq!(single_result, parallel_result);
    }

    #[test]
    fn test_parallel_search_is_stable_across_repeated_runs() {
        let mut engine = Engine::with_threads_and_table(3, 4, 16);
        let config = TimeConfig::fixed_depth(3);
        let mut baseline = None;

        for _ in 0..3 {
            let mut game = setup_game();
            let result = engine.best_move_timed(&mut game, &config);
            assert!(
                result.is_some(),
                "Parallel search should always return a move"
            );

            if let Some(ref expected) = baseline {
                assert_eq!(result.as_ref(), Some(expected));
            } else {
                baseline = result;
            }
        }
    }

    #[test]
    fn test_parallel_search_respects_time_limit() {
        let mut game = setup_game();
        let mut engine = Engine::with_threads(6, 4);
        let config = TimeConfig::fixed_time(50);
        let start = Instant::now();

        let result = engine.best_move_timed(&mut game, &config);
        let elapsed = start.elapsed();

        assert!(
            result.is_some(),
            "Timed parallel search should still return a move"
        );
        assert!(
            elapsed.as_secs_f32() < 2.0,
            "Timed parallel search exceeded expected wall time: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_quiescence_prevents_horizon_effect() {
        let mut game = Game::new();
        game.board = Board::new();

        game.board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        game.board.set(
            "e4",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        game.board.set(
            "e8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        game.board.set(
            "d6",
            Some(Piece {
                piece_type: PieceType::Knight,
                color: Color::Black,
            }),
        );

        let mut engine = Engine::new(4);
        let config = TimeConfig::fixed_depth(4);
        let result = engine.best_move_timed(&mut game, &config);

        assert!(result.is_some());
    }

    #[test]
    fn test_repetition_requires_two_prior_occurrences() {
        let mut board = Board::new();
        board.setup_standard();

        let mut engine = Engine::new(1);
        let hash = board.hash(Color::White);

        // A position can only recur an even number (>= 4) of plies later.
        let cycle = [hash, 1, 2, 3];
        engine.search_history = cycle.to_vec();
        assert!(
            !engine.is_repetition_draw(hash, 0),
            "One prior occurrence before the root should not be treated as a draw"
        );
        assert!(
            engine.is_repetition_draw(hash, 5),
            "One prior occurrence inside the search tree is a draw"
        );

        engine.search_history.extend_from_slice(&cycle);
        assert!(
            engine.is_repetition_draw(hash, 0),
            "Two prior occurrences should trigger a repetition draw"
        );

        engine.rep_floor = 1;
        assert!(
            !engine.is_repetition_draw(hash, 0),
            "Positions before the last irreversible move cannot repeat"
        );
    }

    #[test]
    fn test_quiescence_returns_mate_when_in_check_with_no_evasions() {
        let mut board = Board::new();
        board.set(
            "h1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "g2",
            Some(Piece {
                piece_type: PieceType::Queen,
                color: Color::Black,
            }),
        );
        board.set(
            "h2",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "a8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );

        let mut engine = Engine::new(1);
        let score = engine.pvs(
            &mut board,
            Color::White,
            0,
            -MATE_VALUE,
            MATE_VALUE,
            0,
            None,
            true,
        );

        assert!(board.in_check_fast(Color::White));
        assert_eq!(
            engine.generate_legal_moves(&mut board, Color::White).len(),
            0
        );
        assert_eq!(
            score, -MATE_VALUE,
            "Depth-0 search should see mate in qsearch"
        );
    }

    #[test]
    fn test_quiescence_searches_evasions_when_in_check() {
        let mut board = Board::new();
        board.set(
            "h1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "g1",
            Some(Piece {
                piece_type: PieceType::Queen,
                color: Color::White,
            }),
        );
        board.set(
            "h2",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "a8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );

        let mut engine = Engine::new(1);
        let score = engine.pvs(
            &mut board,
            Color::White,
            0,
            -MATE_VALUE,
            MATE_VALUE,
            0,
            None,
            true,
        );

        assert!(board.in_check_fast(Color::White));
        assert!(
            engine.generate_legal_moves(&mut board, Color::White).len() > 0,
            "Position should have at least one legal evasion"
        );
        assert!(
            score > -MATE_VALUE,
            "Depth-0 search should explore evasions instead of treating any check node as lost"
        );
    }

    /// Black (a queen up) to move, in a position seen twice already; h8g8 would
    /// recreate a position that also occurred twice before: a threefold draw.
    fn repetition_avoidance_game() -> Game {
        let mut game =
            crate::fen::game_from_fen("6k1/5ppp/8/8/3q4/8/8/R6K w - - 0 1").expect("valid FEN");
        for mv in ["a1a2", "g8h8", "a2a1", "h8g8", "a1a2", "g8h8", "a2a1"] {
            let (from, to) = mv.split_at(2);
            assert!(game.make_move(from, to), "illegal move {mv}");
        }
        assert_eq!(game.current_turn, Color::Black);
        game
    }

    #[test]
    fn test_parallel_root_search_sees_game_history_repetitions() {
        let game = repetition_avoidance_game();
        let color = game.current_turn;
        let repeat = game.board.encode_move("h8", "g8", color).unwrap();
        let depth = 5;

        // Single-thread path: the root pvs sees the draw at ply 1.
        let mut single = Engine::new(depth);
        single.search_history = game.hash_history.clone();
        single.search_history.pop();
        let mut board = game.board.clone();
        let undo = board.make_move_fast(repeat, color);
        single.search_history.push(game.board.hash(color));
        let single_score = -single.pvs(
            &mut board,
            opposite(color),
            depth - 1,
            -MATE_VALUE,
            MATE_VALUE,
            1,
            Some(repeat),
            true,
        );
        board.unmake_move_fast(undo, color);
        assert_eq!(single_score, 0);

        // Parallel path: same history as `best_move_parallel` sets up.
        let mut worker = Engine::with_threads(depth, 3);
        worker.search_history = game.hash_history.clone();
        worker.search_history.pop();
        let len_before = worker.search_history.len();
        let score = worker
            .search_root_move(&game.board, color, depth, repeat, -MATE_VALUE, MATE_VALUE)
            .unwrap();
        assert_eq!(
            score, 0,
            "the repetition must be a draw in the parallel search too"
        );
        assert_eq!(worker.search_history.len(), len_before);
    }

    #[test]
    fn test_single_and_parallel_search_both_avoid_repetition() {
        for threads in [1, 3] {
            let mut game = repetition_avoidance_game();
            let mut engine = Engine::with_threads(9, threads);
            let ((from, to), _) = engine
                .best_move_timed(&mut game, &TimeConfig::fixed_depth(9))
                .expect("a move");
            assert_ne!(
                (from.as_str(), to.as_str()),
                ("h8", "g8"),
                "threads={threads}: the winning side must not allow the threefold"
            );
        }
    }

    #[test]
    fn test_quiescence_scores_stalemate_as_draw() {
        // Black to move: Ka8 has no legal move and is not in check.
        let game = crate::fen::game_from_fen("k7/8/1Q6/8/8/8/8/7K b - - 0 1").unwrap();
        let mut board = game.board.clone();
        let mut engine = Engine::new(1);
        let score = engine.quiescence(&mut board, Color::Black, -MATE_VALUE, MATE_VALUE, 1);
        assert_eq!(score, 0);
        // Same when the window would let stand-pat fail low or high.
        let mut engine = Engine::new(1);
        assert_eq!(engine.quiescence(&mut board, Color::Black, -50, 50, 1), 0);
    }

    #[test]
    fn test_syzygy_fen_carries_the_halfmove_clock() {
        let mut game = crate::fen::game_from_fen("8/8/8/4k3/8/2K5/P7/7Q w - - 0 1").unwrap();
        game.board.halfmove = 137;
        let fen = Engine::syzygy_fen(&game.board, Color::White);
        assert!(fen.ends_with(" 137 1"), "{fen}");
        let setup = fen.parse::<Fen>().unwrap();
        let pos: Chess = setup.into_position(CastlingMode::Standard).unwrap();
        use shakmaty::Position;
        assert_eq!(pos.halfmoves(), 137);
    }
}
