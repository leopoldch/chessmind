use crate::board::Board; // Removed color_idx, UndoState
use crate::game::Game;
use crate::opening::book_move;
use crate::pieces::{Color, PieceType};
use crate::transposition::{Bound, TABLE_SIZE, TTEntry, Table};
use crate::types::{Move, mvv_lva_score}; // Import Move, mvv_lva_score
use shakmaty::{CastlingMode, Chess, fen::Fen};
use shakmaty_syzygy::{Tablebase, Wdl};
use std::collections::HashMap;
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

#[allow(dead_code)]
struct TimeManager {
    start_time: Instant,
    soft_time_ms: u64,
    hard_time_ms: u64,
    move_overhead_ms: u64,
    increment_ms: u64,
    in_crisis: bool,
    stop_flag: Arc<AtomicBool>,
    node_count: Arc<AtomicU64>,
}

#[allow(dead_code)]
impl TimeManager {
    fn new(config: &TimeConfig, color: Color, stop_flag: Arc<AtomicBool>) -> Self {
        let node_count = Arc::new(AtomicU64::new(0));

        if let Some(movetime) = config.movetime {
            let move_overhead_ms = Self::move_overhead_ms(movetime, 0);
            let soft_time_ms = movetime.saturating_sub(move_overhead_ms).max(10);
            return Self {
                start_time: Instant::now(),
                soft_time_ms,
                hard_time_ms: movetime.max(10),
                move_overhead_ms,
                increment_ms: 0,
                in_crisis: false,
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

        let mut hard_time_ms = if increment_ms > 0 {
            (usable_time_ms / 2) + (increment_ms / 2)
        } else {
            (usable_time_ms * 3) / 10
        };

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

    #[inline(always)]
    fn should_stop(&self) -> bool {
        if self.stop_flag.load(Ordering::Relaxed) {
            return true;
        }

        let elapsed = self.start_time.elapsed().as_millis() as u64;
        elapsed >= self.soft_time_ms || elapsed >= self.hard_time_ms
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

    #[inline(always)]
    fn check_time(&self) -> bool {
        let count = self.node_count.fetch_add(1, Ordering::Relaxed);
        if count & 2047 == 0 {
            !self.should_stop()
        } else {
            !self.stop_flag.load(Ordering::Relaxed)
        }
    }

    fn nodes(&self) -> u64 {
        self.node_count.load(Ordering::Relaxed)
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
        if elapsed >= self.hard_time_ms || elapsed >= self.soft_time_ms {
            return false;
        }

        let remaining = self.soft_time_ms - elapsed;
        let reserve = if self.increment_ms > 0 {
            self.move_overhead_ms.saturating_sub(5).max(10)
        } else {
            self.move_overhead_ms.max(10)
        };
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

        let required = if self.in_crisis {
            iter_cost.saturating_add(reserve * 2)
        } else if stable_move {
            iter_cost.saturating_mul(2).saturating_add(reserve)
        } else if score_delta.is_some_and(|delta| delta >= 80) {
            iter_cost.saturating_mul(4) / 3 + reserve
        } else {
            iter_cost.saturating_mul(3) / 2 + reserve
        };

        remaining > required
    }
}

const RFP_MARGIN: [i32; 4] = [0, 150, 250, 350];
const FUTILITY_MARGIN: [i32; 5] = [0, 120, 220, 320, 440];
const HLP_THRESHOLD: u32 = 3;
const HLP_BASE: i32 = -50;
const LMP_LIMITS: [usize; 5] = [0, 5, 7, 10, 14];
const MATE_VALUE: i32 = 10000;
const MAX_PLY: usize = 128;
const MAX_DEPTH: u32 = 64;
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
}

pub struct Engine {
    pub depth: u32,
    pub threads: usize,
    tt: Table,
    killers: Vec<[Option<Move>; 2]>,
    quiet_history: [[i32; 64]; 64],
    capture_history: [[i32; 64]; 64],
    cont_history: HashMap<(u16, u16), i32>,
    tb: Option<Arc<Tablebase<Chess>>>,
    stop_flag: Arc<AtomicBool>,
    time_manager: Option<Arc<TimeManager>>,
    search_history: Vec<u64>,
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
        }
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
            cont_history: HashMap::new(),
            tb: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            time_manager: None,
            search_history: Vec::new(),
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

    fn string_to_move(&self, board: &Board, s: &str, e: &str) -> Move {
        let (sx, sy) = Board::algebraic_to_index(s).unwrap();
        let (ex, ey) = Board::algebraic_to_index(e).unwrap();
        let from = (sy * 8 + sx) as u8;
        let to = (ey * 8 + ex) as u8;

        let piece = board.get_index(sx, sy).unwrap();
        let captured = board.get_index(ex, ey);

        let is_capture = captured.is_some();
        let mut flags = Move::FLAG_NORMAL;

        match piece.piece_type {
            PieceType::Pawn => {
                let diff_y = (ey as isize - sy as isize).abs();
                let diff_x = (ex as isize - sx as isize).abs();

                if ey == 0 || ey == 7 {
                    if is_capture {
                        flags = Move::FLAG_PROMO_QUEEN_CAP;
                    } else {
                        flags = Move::FLAG_PROMO_QUEEN;
                    }
                } else if diff_y == 2 && diff_x == 0 {
                    flags = Move::FLAG_DOUBLE_PUSH;
                } else if diff_x != 0 && !is_capture {
                    flags = Move::FLAG_EP_CAPTURE;
                } else if is_capture {
                    flags = Move::FLAG_CAPTURE;
                }
            }
            PieceType::King => {
                let diff_x = (ex as isize - sx as isize).abs();
                if diff_x == 2 {
                    if ex > sx {
                        flags = Move::FLAG_KING_CASTLE;
                    } else {
                        flags = Move::FLAG_QUEEN_CASTLE;
                    }
                } else if is_capture {
                    flags = Move::FLAG_CAPTURE;
                }
            }
            _ => {
                if is_capture {
                    flags = Move::FLAG_CAPTURE;
                }
            }
        }

        Move::new(from, to, flags)
    }

    fn generate_legal_moves(&self, board: &mut Board, color: Color) -> crate::types::MoveList {
        let mut list = crate::types::MoveList::new();
        crate::movegen::generate_moves_fast(board, color, &mut list);
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
        if depth < 3 || idx < 3 {
            return 0;
        }
        let d = (depth as f64).ln();
        let m = ((idx + 1) as f64).ln();
        let mut r = (d * m / 1.5) as i32;
        if r < 1 {
            r = 1;
        }
        if r as u32 > depth - 1 {
            r = (depth - 1) as i32;
        }
        r as u32
    }

    fn probe_syzygy(&self, board: &Board, color: Color, ply: usize) -> Option<i32> {
        let tb = self.tb.as_ref()?;
        if board.piece_count_all() > tb.max_pieces() {
            return None;
        }
        let fen = board.to_fen(color);
        let pos: Chess = fen
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

    fn move_score(&self, board: &Board, mv: Move, ply: usize, prev: Option<&Move>) -> i32 {
        let mut score = 0;
        let capture = mv.is_capture();
        let from = mv.from_sq() as usize;
        let to = mv.to_sq() as usize;

        if capture {
            score += self.capture_history[from][to];

            let victim_idx = if mv.is_ep() {
                0 // Pawn
            } else {
                let tx = to % 8;
                let ty = to / 8;
                board.piece_type_idx_at((ty * 8 + tx) as u8)
            };

            let attacker_idx = board.piece_type_idx_at(from as u8);

            if victim_idx < 6 && attacker_idx < 6 {
                score += mvv_lva_score(victim_idx, attacker_idx) * 100;
            }

            if self.static_exchange_eval(board, mv) < 0 {
                score -= 1000;
            }
        } else {
            score += self.quiet_history[from][to];
            if let Some(k) = self.killers.get(ply) {
                if let Some(m) = &k[0] {
                    if m.0 == mv.0 {
                        score += 10_000;
                    }
                }
                if let Some(m) = &k[1] {
                    if m.0 == mv.0 {
                        score += 9_000;
                    }
                }
            }
        }

        if let Some(pmv) = prev {
            score += *self.cont_history.get(&(pmv.0, mv.0)).unwrap_or(&0);
        }
        score
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
    fn tt_move_matches(mv: Move, tt_best: Option<(u8, u8)>) -> bool {
        tt_best.is_some_and(|(from, to)| mv.from_sq() == from && mv.to_sq() == to)
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

    #[inline(always)]
    fn repetition_count(&self, hash: u64) -> usize {
        let mut count = 0;
        for &seen in self.search_history.iter().rev() {
            if seen == hash {
                count += 1;
                if count >= 2 {
                    break;
                }
            }
        }
        count
    }

    #[inline(always)]
    fn is_repetition_draw(&self, hash: u64) -> bool {
        self.repetition_count(hash) >= 2
    }

    fn classify_move(
        &self,
        board: &Board,
        mv: Move,
        ply: usize,
        prev: Option<&Move>,
        tt_best: Option<(u8, u8)>,
    ) -> (u8, i32, i32) {
        if Self::tt_move_matches(mv, tt_best) {
            return (ORDER_TT, i32::MAX, 0);
        }

        let mut score = self.move_score(board, mv, ply, prev);
        let mut see = 0;

        if mv.is_promotion() {
            score += Self::promotion_bonus(mv);
            return (ORDER_PROMOTION, score, see);
        }

        if mv.is_capture() {
            see = self.static_exchange_eval(board, mv);
            score += see * 128;
            let stage = if see >= 0 {
                ORDER_GOOD_CAPTURE
            } else {
                ORDER_BAD_CAPTURE
            };
            return (stage, score, see);
        }

        if let Some(k) = self.killers.get(ply) {
            if k[0] == Some(mv) {
                return (ORDER_KILLER_1, score + 20_000, see);
            }
            if k[1] == Some(mv) {
                return (ORDER_KILLER_2, score + 15_000, see);
            }
        }

        (ORDER_QUIET, score, see)
    }

    fn pick_next_move(
        moves: &mut crate::types::MoveList,
        stages: &mut [u8; 256],
        scores: &mut [i32; 256],
        sees: &mut [i32; 256],
        start: usize,
    ) -> Move {
        let len = moves.len();
        let mut best = start;
        for idx in (start + 1)..len {
            if stages[idx] < stages[best]
                || (stages[idx] == stages[best] && scores[idx] > scores[best])
            {
                best = idx;
            }
        }

        if best != start {
            moves.swap(start, best);
            stages.swap(start, best);
            scores.swap(start, best);
            sees.swap(start, best);
        }

        moves[start]
    }

    fn find_tt_move(&self, board: &Board, color: Color, best: Option<(u8, u8)>) -> Option<Move> {
        let (from, to) = best?;
        let mut board_clone = board.clone();
        let moves = self.generate_legal_moves(&mut board_clone, color);
        for mv in moves.iter() {
            if mv.from_sq() == from && mv.to_sq() == to {
                return Some(*mv);
            }
        }
        None
    }

    #[inline(always)]
    fn should_stop(&self) -> bool {
        if self.stop_flag.load(Ordering::Relaxed) {
            return true;
        }
        if let Some(tm) = &self.time_manager {
            let count = tm.node_count.fetch_add(1, Ordering::Relaxed);
            if count & 2047 == 0 {
                if tm.should_stop() {
                    self.stop_flag.store(true, Ordering::Release);
                    return true;
                }
            }
        }
        false
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

        let hash = board.hash(color);
        if self.is_repetition_draw(hash) {
            return 0;
        }
        self.search_history.push(hash);

        let result = 'q: {
            let in_check = board.in_check_fast(color);

            if !in_check {
                let stand_pat = Self::evaluate(board, color);

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

            let list = self.generate_legal_moves(board, color);
            if list.is_empty() {
                if in_check {
                    -MATE_VALUE + ply as i32
                } else {
                    alpha
                }
            } else {
                let mut moves = crate::types::MoveList::new();
                for m in list.iter() {
                    if in_check || m.is_capture() || m.is_promotion() {
                        moves.push(*m);
                    }
                }

                let mut stages = [ORDER_QUIET; 256];
                let mut scores = [0i32; 256];
                let mut sees = [0i32; 256];

                for idx in 0..moves.len() {
                    let mv = moves[idx];
                    let (stage, score, see) = self.classify_move(board, mv, ply, None, None);
                    stages[idx] = stage;
                    scores[idx] = score;
                    sees[idx] = see;
                }

                let mut best = alpha;
                for idx in 0..moves.len() {
                    let m =
                        Self::pick_next_move(&mut moves, &mut stages, &mut scores, &mut sees, idx);

                    if !in_check && !m.is_promotion() && stages[idx] == ORDER_BAD_CAPTURE {
                        continue;
                    }

                    let undo = board.make_move_fast(m, color);
                    let score = -self.quiescence(board, opposite(color), -beta, -best, ply + 1);
                    board.unmake_move_fast(undo, color);

                    if self.stop_flag.load(Ordering::Relaxed) {
                        break 'q 0;
                    }

                    if score >= beta {
                        break 'q score;
                    }
                    if score > best {
                        best = score;
                    }
                }

                best
            }
        };

        self.search_history.pop();
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
        _use_iir: bool,
    ) -> i32 {
        if self.should_stop() {
            return 0;
        }

        let hash = board.hash(color);
        if self.is_repetition_draw(hash) {
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
            let mut tt_best: Option<(u8, u8)> = None;

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

            if let Some(tb_val) = self.probe_syzygy(board, color, ply) {
                tb_val
            } else if depth == 0 {
                self.quiescence(board, color, alpha, beta, ply)
            } else {
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

                let can_null =
                    !is_pv && !in_check && board.piece_count_total(color) > 3 && depth >= 3;
                if can_null {
                    let r = if depth > 6 { 3 } else { 2 };
                    let ep = board.en_passant;

                    board.en_passant = None;
                    let score = -self.pvs(
                        board,
                        opposite(color),
                        depth - 1 - r,
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
                        if depth > 8 {
                            let verify = self.pvs(
                                board,
                                color,
                                depth - r - 1,
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

                let mut moves_list = self.generate_legal_moves(board, color);
                if moves_list.is_empty() {
                    if in_check {
                        break 'search -MATE_VALUE + ply as i32;
                    }
                    break 'search 0;
                }

                let mut stages = [ORDER_QUIET; 256];
                let mut scores = [0i32; 256];
                let mut sees = [0i32; 256];

                for idx in 0..moves_list.len() {
                    let mv = moves_list[idx];
                    let (stage, score, see) =
                        self.classify_move(board, mv, ply, prev_move.as_ref(), tt_best);
                    stages[idx] = stage;
                    scores[idx] = score;
                    sees[idx] = see;
                }

                let mut best_move: Option<Move> = None;
                let mut best_score = -MATE_VALUE;
                let mut skip_quiets = false;

                for idx in 0..moves_list.len() {
                    let m = Self::pick_next_move(
                        &mut moves_list,
                        &mut stages,
                        &mut scores,
                        &mut sees,
                        idx,
                    );

                    let capture = m.is_capture();
                    let promotion = m.is_promotion();
                    let is_quiet = !capture && !promotion;

                    if !is_pv
                        && !in_check
                        && is_quiet
                        && depth <= 4
                        && idx >= LMP_LIMITS[depth as usize]
                    {
                        continue;
                    }
                    if skip_quiets && is_quiet {
                        continue;
                    }
                    if !is_pv && !in_check && is_quiet && depth <= HLP_THRESHOLD && idx > 0 {
                        if scores[idx] < HLP_BASE {
                            skip_quiets = true;
                            continue;
                        }
                    }
                    if !is_pv
                        && !in_check
                        && depth <= 4
                        && is_quiet
                        && static_eval
                            .is_some_and(|eval| eval + FUTILITY_MARGIN[depth as usize] <= alpha)
                    {
                        continue;
                    }

                    let undo = board.make_move_fast(m, color);
                    let gives_check = board.in_check_fast(opposite(color));

                    let mut new_depth = depth - 1;
                    if gives_check && depth < MAX_DEPTH - 1 {
                        new_depth = new_depth.saturating_add(1);
                    }

                    let can_reduce =
                        !is_pv && depth > 2 && is_quiet && !in_check && !gives_check && idx >= 3;
                    if can_reduce {
                        let r = Self::lmr_value(depth, idx + 1);
                        new_depth = new_depth.saturating_sub(r);
                    }

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
                        if score > alpha && score < beta {
                            score = -self.pvs(
                                board,
                                opposite(color),
                                depth - 1 + u32::from(gives_check),
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

                        let from = m.from_sq() as usize;
                        let to = m.to_sq() as usize;
                        let bonus = (depth * depth) as i32;

                        if capture {
                            self.capture_history[from][to] += bonus;
                        } else {
                            self.quiet_history[from][to] += bonus;
                        }

                        if let Some(pmv) = prev_move {
                            *self.cont_history.entry((pmv.0, m.0)).or_insert(0) += bonus;
                        }

                        self.tt.store(
                            hash,
                            TTEntry {
                                depth,
                                value: Self::score_to_tt(score, ply),
                                bound: Bound::Lower,
                                best: Some((from as u8, to as u8)),
                            },
                        );

                        break 'search score;
                    } else {
                        let from = m.from_sq() as usize;
                        let to = m.to_sq() as usize;
                        let penalty = (depth * depth) as i32;
                        if capture {
                            self.capture_history[from][to] -= penalty;
                        } else {
                            self.quiet_history[from][to] -= penalty;
                        }
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

                let best_idx = best_move.map(|m| (m.from_sq(), m.to_sq()));
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
                        best: best_idx,
                    },
                );

                alpha
            }
        };

        self.search_history.pop();
        result
    }

    fn ordered_root_moves(&self, board: &Board, color: Color) -> Vec<Move> {
        let root_hash = board.hash(color);
        let tt_best = self.tt.get(root_hash).and_then(|entry| entry.best);
        let mut board_clone = board.clone();
        let mut moves = self.generate_legal_moves(&mut board_clone, color);

        if moves.is_empty() {
            return Vec::new();
        }

        let mut stages = [ORDER_QUIET; 256];
        let mut scores = [0i32; 256];
        let mut sees = [0i32; 256];

        for idx in 0..moves.len() {
            let mv = moves[idx];
            let (stage, score, see) = self.classify_move(board, mv, 0, None, tt_best);
            stages[idx] = stage;
            scores[idx] = score;
            sees[idx] = see;
        }

        let mut ordered = Vec::with_capacity(moves.len());
        for idx in 0..moves.len() {
            let mv = Self::pick_next_move(&mut moves, &mut stages, &mut scores, &mut sees, idx);
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
    ) -> Option<i32> {
        if self.stop_flag.load(Ordering::Relaxed) {
            return None;
        }

        let mut board = board.clone();
        let undo = board.make_move_fast(mv, color);
        let gives_check = board.in_check_fast(opposite(color));
        let child_depth = Self::root_child_depth(depth, gives_check);
        let score = -self.pvs(
            &mut board,
            opposite(color),
            child_depth,
            -MATE_VALUE,
            MATE_VALUE,
            1,
            Some(mv),
            true,
        );
        board.unmake_move_fast(undo, color);

        if self.stop_flag.load(Ordering::Relaxed) {
            None
        } else {
            Some(score)
        }
    }

    fn search_root_parallel(&self, board: &Board, color: Color, depth: u32) -> Option<(Move, i32)> {
        let root_moves = self.ordered_root_moves(board, color);
        if root_moves.is_empty() {
            return None;
        }

        let worker_count = self.threads.max(1).min(root_moves.len());
        if worker_count == 1 {
            let mut worker = self.clone();
            let mut best: Option<RootSearchResult> = None;
            for (index, mv) in root_moves.iter().copied().enumerate() {
                let score = worker.search_root_move(board, color, depth, mv)?;
                let candidate = RootSearchResult { index, mv, score };
                let replace = match best {
                    Some(current) => {
                        candidate.score > current.score
                            || (candidate.score == current.score && candidate.index < current.index)
                    }
                    None => true,
                };
                if replace {
                    best = Some(candidate);
                }
            }
            return best.map(|result| (result.mv, result.score));
        }

        let root_moves = Arc::new(root_moves);
        let next_index = Arc::new(AtomicUsize::new(0));
        let history_base = self.search_history.clone();
        let root_board = board.clone();
        let (tx, rx) = mpsc::channel();

        thread::scope(|scope| {
            for _ in 0..worker_count {
                let tx = tx.clone();
                let root_moves = Arc::clone(&root_moves);
                let next_index = Arc::clone(&next_index);
                let history_base = history_base.clone();
                let root_board = root_board.clone();
                let mut worker = self.clone();

                scope.spawn(move || {
                    worker.search_history = history_base;

                    while !worker.stop_flag.load(Ordering::Relaxed) {
                        let index = next_index.fetch_add(1, Ordering::Relaxed);
                        if index >= root_moves.len() {
                            break;
                        }

                        let mv = root_moves[index];
                        let Some(score) = worker.search_root_move(&root_board, color, depth, mv)
                        else {
                            break;
                        };

                        if tx.send(RootSearchResult { index, mv, score }).is_err() {
                            break;
                        }
                    }
                });
            }

            drop(tx);

            let mut completed = 0usize;
            let mut best: Option<RootSearchResult> = None;

            while let Ok(result) = rx.recv() {
                completed += 1;
                let replace = match best {
                    Some(current) => {
                        result.score > current.score
                            || (result.score == current.score && result.index < current.index)
                    }
                    None => true,
                };
                if replace {
                    best = Some(result);
                }
            }

            if completed == root_moves.len() {
                best.map(|result| (result.mv, result.score))
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
        self.reset_stop();
        self.tt.next_age();

        if let Some(book_mv) = book_move(&game.history, &game.board, game.current_turn) {
            return Some((book_mv, 0));
        }

        let max_depth = config.depth.unwrap_or(MAX_DEPTH).min(MAX_DEPTH);
        let time_manager = TimeManager::new(config, game.current_turn, self.stop_flag.clone());
        self.time_manager = Some(Arc::new(time_manager));

        let result = if self.threads <= 1 || !Self::should_use_parallel_search(config, max_depth) {
            self.best_move_single(game, max_depth)
        } else {
            self.best_move_parallel(game, max_depth)
        };

        self.time_manager = None;
        result
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
                    } else if let Some((fs, ts)) = entry.best {
                        let f_str = Board::index_to_algebraic((fs % 8) as usize, (fs / 8) as usize)
                            .unwrap();
                        let t_str = Board::index_to_algebraic((ts % 8) as usize, (ts / 8) as usize)
                            .unwrap();
                        best_move = Some(self.string_to_move(&game.board, &f_str, &t_str));
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
            let result = self.search_root_parallel(&game.board, color, d);
            let Some((mv, score)) = result else {
                break;
            };

            self.tt.store(
                root_hash,
                TTEntry {
                    depth: d,
                    value: Self::score_to_tt(score, 0),
                    bound: Bound::Exact,
                    best: Some((mv.from_sq(), mv.to_sq())),
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

        engine.search_history = vec![hash];
        assert!(
            !engine.is_repetition_draw(hash),
            "One prior occurrence should not be treated as a draw"
        );

        engine.search_history.push(hash);
        assert!(
            engine.is_repetition_draw(hash),
            "Two prior occurrences should trigger a repetition draw"
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
}
