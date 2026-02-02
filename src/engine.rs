use crate::board::Board; // Removed color_idx, UndoState
use crate::game::Game;
use crate::opening::book_move;
use crate::pieces::{Color, Piece, PieceType};
use crate::transposition::{Bound, TABLE_SIZE, TTEntry, Table};
use crate::types::{Move, mvv_lva_score}; // Import Move, mvv_lva_score
use shakmaty::{CastlingMode, Chess, fen::Fen};
use shakmaty_syzygy::{Tablebase, Wdl};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    allocated_time_ms: u64,
    max_time_ms: u64,
    in_crisis: bool,
    stop_flag: Arc<AtomicBool>,
    node_count: Arc<AtomicU64>,
}

#[allow(dead_code)]
impl TimeManager {
    fn new(config: &TimeConfig, color: Color, stop_flag: Arc<AtomicBool>) -> Self {
        let node_count = Arc::new(AtomicU64::new(0));

        if let Some(movetime) = config.movetime {
            return Self {
                start_time: Instant::now(),
                allocated_time_ms: movetime,
                max_time_ms: movetime,
                in_crisis: false,
                stop_flag,
                node_count,
            };
        }

        if config.infinite || config.depth.is_some() {
            return Self {
                start_time: Instant::now(),
                allocated_time_ms: u64::MAX,
                max_time_ms: u64::MAX,
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

        let (allocated, max_time, in_crisis) =
            Self::calculate_time(our_time, increment, config.movestogo);

        Self {
            start_time: Instant::now(),
            allocated_time_ms: allocated,
            max_time_ms: max_time,
            in_crisis,
            stop_flag,
            node_count,
        }
    }

    fn calculate_time(
        time_left_ms: u64,
        increment_ms: u64,
        moves_to_go: Option<u32>,
    ) -> (u64, u64, bool) {
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

        let base_time = time_left_ms / estimated_moves;
        let inc_bonus = (increment_ms * 9) / 10; // Use 90% of increment

        let mut allocated = base_time + inc_bonus;

        let total_time_estimate = time_left_ms + increment_ms * 20; // Rough estimate of total game time
        let in_crisis = if total_time_estimate < 300_000 {
            time_left_ms < 1000
        } else {
            time_left_ms < 5000
        };

        if in_crisis {
            allocated = (time_left_ms / 30).max(50).min(500);
        }

        let max_time = if increment_ms > 0 {
            (time_left_ms / 2) + increment_ms
        } else {
            time_left_ms * 3 / 10
        };

        allocated = allocated.min(max_time);

        allocated = allocated.max(10);
        let max_time = max_time.max(10);

        (allocated, max_time, in_crisis)
    }

    #[inline(always)]
    fn should_stop(&self) -> bool {
        if self.stop_flag.load(Ordering::Relaxed) {
            return true;
        }

        let elapsed = self.start_time.elapsed().as_millis() as u64;
        elapsed >= self.allocated_time_ms
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn time_exceeded(&self) -> bool {
        let elapsed = self.start_time.elapsed().as_millis() as u64;
        elapsed >= self.max_time_ms
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

    fn should_continue_iterating(&self) -> bool {
        if self.stop_flag.load(Ordering::Relaxed) {
            return false;
        }
        let elapsed = self.start_time.elapsed().as_millis() as u64;
        elapsed < self.allocated_time_ms / 2
    }
}

const RFP_MARGIN: [i32; 4] = [0, 150, 250, 350];
const HLP_THRESHOLD: u32 = 3;
const HLP_BASE: i32 = -50;
const LMP_LIMITS: [usize; 5] = [0, 5, 7, 10, 14];
const MATE_VALUE: i32 = 10000;
const MAX_PLY: usize = 128;
const MAX_DEPTH: u32 = 64;

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

    #[inline(always)]
    fn piece_value(t: PieceType) -> i32 {
        crate::types::PieceValues::value(t)
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

    fn cheapest_attacker(
        board: &mut Board,
        color: Color,
        tx: usize,
        ty: usize,
    ) -> Option<((usize, usize), Piece)> {
        let target = Board::index_to_algebraic(tx, ty)?;
        let mut best: Option<((usize, usize), Piece)> = None;
        for y in 0..8 {
            for x in 0..8 {
                if let Some(p) = board.get_index(x, y) {
                    if p.color == color {
                        if let Some(from) = Board::index_to_algebraic(x, y) {
                            if board.pseudo_legal_moves(&from).iter().any(|m| m == &target)
                                && board.is_legal(&from, &target, color)
                            {
                                if best.as_ref().map_or(true, |(_, bp)| {
                                    Self::piece_value(p.piece_type)
                                        < Self::piece_value(bp.piece_type)
                                }) {
                                    best = Some(((x, y), p));
                                }
                            }
                        }
                    }
                }
            }
        }
        best
    }

    fn see_rec(&self, board: &mut Board, color: Color, tx: usize, ty: usize) -> i32 {
        if let Some(((sx, sy), _piece)) = Self::cheapest_attacker(board, color, tx, ty) {
            let from = Board::index_to_algebraic(sx, sy).unwrap();
            let to = Board::index_to_algebraic(tx, ty).unwrap();
            if let Some(state) = board.make_move_state(&from, &to) {
                let captured_val = state
                    .captured
                    .map_or(0, |p| Self::piece_value(p.piece_type));
                let gain = captured_val - self.see_rec(board, opposite(color), tx, ty);
                board.unmake_move(state);
                return gain.max(0);
            }
        }
        0
    }

    #[inline(always)]
    fn static_exchange_eval(&self, board: &Board, mv: Move) -> i32 {
        let sx = (mv.from_sq() % 8) as usize;
        let sy = (mv.from_sq() / 8) as usize;
        let ex = (mv.to_sq() % 8) as usize;
        let ey = (mv.to_sq() / 8) as usize;

        if board.get_index(sx, sy).is_none() {
            return 0; // Should not happen for legal moves
        }

        let mut b = board.clone(); // Clone is expensive! Use with caution.

        let color = board.get_index(sx, sy).unwrap().color;

        let undo = b.make_move_fast(mv, color);

        let captured_val = if undo.has_capture() {
            crate::types::PieceValues::value_by_idx(undo.captured as usize)
        } else {
            0
        };

        let gain = captured_val - self.see_rec(&mut b, opposite(color), ex, ey);
        b.unmake_move_fast(undo, color);
        return gain;
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

        let stand_pat = Self::evaluate(board, color);

        if stand_pat >= beta {
            return beta;
        }

        if stand_pat > alpha {
            alpha = stand_pat;
        }

        const DELTA: i32 = 1000;
        if stand_pat + DELTA < alpha {
            return alpha;
        }

        let list = self.generate_legal_moves(board, color);
        let mut capt_list = crate::types::MoveList::new();
        for m in list.iter() {
            if m.is_capture() {
                capt_list.push(*m);
            }
        }
        let mut moves = capt_list;

        let len = moves.len();
        if len > 1 {
            let slice = moves.as_mut_slice();
            slice.sort_by(|a, b| {
                let score_a = self.move_score(board, *a, ply, None);
                let score_b = self.move_score(board, *b, ply, None);
                score_b.cmp(&score_a)
            });
        }

        for m in moves.iter() {
            let see_value = self.static_exchange_eval(board, *m);
            if see_value < 0 {
                continue;
            }

            let undo = board.make_move_fast(*m, color);

            let score = -self.quiescence(board, opposite(color), -beta, -alpha, ply + 1);

            board.unmake_move_fast(undo, color);

            if self.stop_flag.load(Ordering::Relaxed) {
                return 0;
            }

            if score >= beta {
                return beta;
            }
            if score > alpha {
                alpha = score;
            }
        }

        alpha
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

        if ply > 0 {
            let current_hash = board.hash(color);
            for &h in &self.search_history {
                if h == current_hash {
                    return 0; // Draw by repetition
                }
            }
        }

        let mate_max = MATE_VALUE - ply as i32;
        if beta > mate_max {
            beta = mate_max;
        }
        let mate_min = -mate_max;
        if alpha < mate_min {
            alpha = mate_min;
        }
        if alpha >= beta {
            return alpha;
        }

        let alpha_orig = alpha;
        let hash = board.hash(color); // board.hash is u64 (Zobrist)
        let mut tt_best: Option<Move> = None;

        if let Some(entry) = self.tt.get(hash) {
            if entry.depth >= depth {
                match entry.bound {
                    Bound::Exact => return entry.value,
                    Bound::Lower => alpha = alpha.max(entry.value),
                    Bound::Upper => {}
                }
                if alpha >= beta {
                    return entry.value;
                }
            }
            if let Some((fs, ts)) = entry.best {
                let f_str =
                    Board::index_to_algebraic((fs % 8) as usize, (fs / 8) as usize).unwrap();
                let t_str =
                    Board::index_to_algebraic((ts % 8) as usize, (ts / 8) as usize).unwrap();
                tt_best = Some(self.string_to_move(board, &f_str, &t_str));
            }
        }

        if let Some(tb_val) = self.probe_syzygy(board, color, ply) {
            return tb_val;
        }

        if depth == 0 {
            return self.quiescence(board, color, alpha, beta, ply);
        }

        let in_check = board.in_check(color);

        if depth <= 3 && !in_check {
            let eval = Self::evaluate(board, color);
            if eval - RFP_MARGIN[depth as usize] >= beta {
                return eval;
            }
        }

        let can_null = !in_check && board.piece_count_total(color) > 3 && depth >= 3;
        if can_null {
            let r = if depth > 6 { 3 } else { 2 };
            let ep = board.en_passant; // Backup EP

            board.en_passant = None;
            let score = -self.pvs(
                board,
                opposite(color),
                depth - 1 - r,
                -beta,
                -beta + 1,
                ply + 1,
                None, // Prev move is null
                false,
            );
            board.en_passant = ep; // Restore EP

            if self.stop_flag.load(Ordering::Relaxed) {
                return 0;
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
                        return beta;
                    }
                } else {
                    return beta;
                }
            }
        }

        let mut moves_list = self.generate_legal_moves(board, color);
        if moves_list.len() == 0 {
            if in_check {
                return -MATE_VALUE + ply as i32;
            }
            return 0;
        }

        let moves_slice = moves_list.as_mut_slice();

        let mut scores = [0i32; 256];
        for (i, m) in moves_slice.iter().enumerate() {
            scores[i] = self.move_score(board, *m, ply, prev_move.as_ref());
            if let Some(ttm) = tt_best {
                if m.0 == ttm.0 {
                    scores[i] = 1_000_000;
                }
            }
        }

        let len = moves_slice.len();
        for i in 0..len {
            for j in 0..len - 1 - i {
                if scores[j] < scores[j + 1] {
                    scores.swap(j, j + 1);
                    moves_slice.swap(j, j + 1);
                }
            }
        }

        let mut best_move: Option<Move> = None;
        let mut skip_quiets = false;

        for (idx, m) in moves_slice.iter().enumerate() {
            let capture = m.is_capture();

            if !in_check && !capture && depth <= 4 && idx >= LMP_LIMITS[depth as usize] {
                continue;
            }
            if skip_quiets && !capture {
                continue;
            }
            if !in_check && !capture && depth <= HLP_THRESHOLD && idx > 0 {
                let s = self.move_score(board, *m, ply, prev_move.as_ref());
                if s < HLP_BASE {
                    skip_quiets = true;
                    continue;
                }
            }

            let undo = board.make_move_fast(*m, color);
            let gives_check = board.in_check_fast(opposite(color)); // Fast check

            let mut new_depth = depth - 1;
            if gives_check && depth < MAX_DEPTH - 1 {
                new_depth = new_depth.saturating_add(1);
            }

            if depth > 2 && !capture && !in_check && !gives_check && idx >= 3 {
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
                    Some(*m),
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
                    Some(*m),
                    true,
                );
                if score > alpha && score < beta {
                    score = -self.pvs(
                        board,
                        opposite(color),
                        new_depth,
                        -beta,
                        -alpha,
                        ply + 1,
                        Some(*m),
                        true,
                    );
                }
            }

            board.unmake_move_fast(undo, color);

            if self.stop_flag.load(Ordering::Relaxed) {
                return 0;
            }

            if score >= beta {
                if !capture {
                    if self.killers.len() <= ply {
                        self.killers.resize(ply + 1, [None, None]);
                    }
                    let k = &mut self.killers[ply];
                    if k[0] != Some(*m) {
                        k[1] = k[0];
                        k[0] = Some(*m);
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
                        value: beta,
                        bound: Bound::Lower,
                        best: Some((from as u8, to as u8)),
                    },
                );

                return beta;
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

            if score > alpha {
                alpha = score;
                best_move = Some(*m);
            }
        }

        let bound = if alpha <= alpha_orig {
            Bound::Upper
        } else {
            Bound::Exact
        };

        let best_idx = best_move.map(|m| (m.from_sq(), m.to_sq()));

        self.tt.store(
            hash,
            TTEntry {
                depth,
                value: alpha,
                bound,
                best: best_idx,
            },
        );

        alpha
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

        let result = self.best_move_single(game, max_depth);

        self.time_manager = None;
        result
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

        self.search_history = game.hash_history.clone();

        for d in 1..=max_depth {
            if let Some(ref tm) = self.time_manager {
                if !tm.should_continue_iterating() && d > 1 {
                    break;
                }
            }

            let mut alpha = -100000;
            let mut beta = 100000;
            if d > 1 {
                alpha = guess - ASPIRATION;
                beta = guess + ASPIRATION;
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
                    alpha = (alpha - ASPIRATION * 2).max(-100000);
                    continue;
                }
                if score >= beta {
                    beta = (beta + ASPIRATION * 2).min(100000);
                    continue;
                }

                guess = score;

                if let Some(entry) = self.tt.get(root_hash) {
                    if let Some((fs, ts)) = entry.best {
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
            if self.stop_flag.load(Ordering::Relaxed) {
                break;
            }
        }

        best_move.map(|m| {
            let f =
                Board::index_to_algebraic((m.from_sq() % 8) as usize, (m.from_sq() / 8) as usize)
                    .unwrap();
            let t = Board::index_to_algebraic((m.to_sq() % 8) as usize, (m.to_sq() / 8) as usize)
                .unwrap();
            let mut t_string = t;
            if m.is_promotion() {
                if let Some(pt) = m.promotion_piece() {
                    let c = match pt {
                        PieceType::Queen => 'q',
                        PieceType::Rook => 'r',
                        PieceType::Bishop => 'b',
                        PieceType::Knight => 'n',
                        _ => 'q',
                    };
                    t_string.push(c);
                }
            }
            ((f, t_string), reached_depth)
        })
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
}
