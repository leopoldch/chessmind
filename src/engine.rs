use crate::board::{Board, color_idx, piece_index};
use crate::pieces::{Color, PieceType, Piece};
use crate::game::Game;
use crate::transposition::{Table, TTEntry, Bound, TABLE_SIZE};
use std::collections::HashMap;
use std::sync::Arc;
use shakmaty::{Chess, CastlingMode, fen::Fen};
use shakmaty_syzygy::{Tablebase, Wdl};

const PAWN_PST: [i32; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0,
    5, 10, 10, -20, -20, 10, 10, 5,
    5, -5, -10, 0, 0, -10, -5, 5,
    0, 0, 0, 20, 20, 0, 0, 0,
    5, 5, 10, 25, 25, 10, 5, 5,
    10, 10, 20, 30, 30, 20, 10, 10,
    50, 50, 50, 50, 50, 50, 50, 50,
    0, 0, 0, 0, 0, 0, 0, 0,
];

const KNIGHT_PST: [i32; 64] = [
    -50,-40,-30,-30,-30,-30,-40,-50,
    -40,-20,0,0,0,0,-20,-40,
    -30,0,10,15,15,10,0,-30,
    -30,5,15,20,20,15,5,-30,
    -30,0,15,20,20,15,0,-30,
    -30,5,10,15,15,10,5,-30,
    -40,-20,0,5,5,0,-20,-40,
    -50,-40,-30,-30,-30,-30,-40,-50,
];

const BISHOP_PST: [i32; 64] = [
    -20,-10,-10,-10,-10,-10,-10,-20,
    -10,0,0,0,0,0,0,-10,
    -10,0,5,10,10,5,0,-10,
    -10,5,5,10,10,5,5,-10,
    -10,0,10,10,10,10,0,-10,
    -10,10,10,10,10,10,10,-10,
    -10,5,0,0,0,0,5,-10,
    -20,-10,-10,-10,-10,-10,-10,-20,
];

const ROOK_PST: [i32; 64] = [
    0,0,0,0,0,0,0,0,
    5,10,10,10,10,10,10,5,
    -5,0,0,0,0,0,0,-5,
    -5,0,0,0,0,0,0,-5,
    -5,0,0,0,0,0,0,-5,
    -5,0,0,0,0,0,0,-5,
    -5,0,0,0,0,0,0,-5,
    0,0,0,5,5,0,0,0,
];

const QUEEN_PST: [i32; 64] = [
    -20,-10,-10,-5,-5,-10,-10,-20,
    -10,0,0,0,0,0,0,-10,
    -10,0,5,5,5,5,0,-10,
    -5,0,5,5,5,5,0,-5,
    0,0,5,5,5,5,0,-5,
    -10,5,5,5,5,5,0,-10,
    -10,0,5,0,0,0,0,-10,
    -20,-10,-10,-5,-5,-10,-10,-20,
];

const KING_PST: [i32; 64] = [
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -20,-30,-30,-40,-40,-30,-30,-20,
    -10,-20,-20,-20,-20,-20,-20,-10,
    20,20,0,0,0,0,20,20,
    20,30,10,0,0,10,30,20,
];

const PST: [[i32;64];6] = [
    PAWN_PST,
    KNIGHT_PST,
    BISHOP_PST,
    ROOK_PST,
    QUEEN_PST,
    KING_PST,
];

const BISHOP_PAIR: i32 = 30;
// Margins used for Reverse Futility Pruning by depth (index 0 unused)
const RFP_MARGIN: [i32; 4] = [0, 150, 250, 350];
// Parameters for History Leaf Pruning
const HLP_THRESHOLD: u32 = 3;
const HLP_BASE: i32 = -50;
// Move count thresholds for Late Move Pruning by depth (index 0 unused)
const LMP_LIMITS: [usize; 5] = [0, 5, 7, 10, 14];
const SINGULAR_MARGIN: i32 = 150;

pub struct Engine {
    pub depth: u32,
    pub threads: usize,
    tt: Table,
    killers: Vec<[Option<(String,String)>;2]>,
    quiet_history: HashMap<(String,String), i32>,
    capture_history: HashMap<(String,String), i32>,
    cont_history: HashMap<((String,String),(String,String)), i32>,
    tb: Option<Arc<Tablebase<Chess>>>,
}

impl Clone for Engine {
    fn clone(&self) -> Self {
        Self {
            depth: self.depth,
            threads: self.threads,
            tt: self.tt.clone(),
            killers: self.killers.clone(),
            quiet_history: self.quiet_history.clone(),
            capture_history: self.capture_history.clone(),
            cont_history: self.cont_history.clone(),
            tb: self.tb.clone(),
        }
    }
}

impl Engine {
    pub fn new(depth: u32) -> Self {
        Self::with_threads(depth, 1)
    }

    pub fn with_threads(depth: u32, threads: usize) -> Self {
        Self {
            depth,
            threads,
            tt: Table::new(TABLE_SIZE),
            killers: vec![[None, None]; (depth as usize)+1],
            quiet_history: HashMap::new(),
            capture_history: HashMap::new(),
            cont_history: HashMap::new(),
            tb: None,
        }
    }

    pub fn set_threads(&mut self, threads: usize) {
        self.threads = threads;
    }

    pub fn load_syzygy(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut tb = Tablebase::new();
        tb.add_directory(path)?;
        self.tb = Some(Arc::new(tb));
        Ok(())
    }

    #[inline(always)]
    fn piece_value(t: PieceType) -> i32 {
        match t {
            PieceType::Pawn => 100,
            PieceType::Knight | PieceType::Bishop => 320,
            PieceType::Rook => 500,
            PieceType::Queen => 900,
            PieceType::King => 0,
        }
    }

    const MOBILITY_WEIGHT: i32 = 2;
    const DOUBLED_PAWN_PENALTY: i32 = 8;
    const ISOLATED_PAWN_PENALTY: i32 = 10;
    const PASSED_PAWN_BONUS: i32 = 20;
    const KING_SAFETY_PENALTY: i32 = 10;

    #[inline(always)]
    fn evaluate(board: &Board, color: Color) -> i32 {
        const VALUES: [i32;6] = [100,320,330,500,900,0];
        let mut score = 0;
        for c in [Color::White, Color::Black] {
            let sign = if c == color { 1 } else { -1 };
            let cidx = color_idx(c);
            for p in 0..6 {
                let mut bb = board.bitboards[cidx][p];
                let val = VALUES[p];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as usize;
                    let idx = if c == Color::White {
                        sq
                    } else {
                        let x = sq % 8;
                        let y = sq / 8;
                        (7 - y) * 8 + x
                    };
                    score += sign * (val + PST[p][idx]);
                    if p != piece_index(PieceType::Pawn) && p != piece_index(PieceType::King) {
                        if let Some(pos) = Board::index_to_algebraic(sq % 8, sq / 8) {
                            let mob = board.pseudo_legal_moves(&pos).len() as i32;
                            score += sign * Self::MOBILITY_WEIGHT * mob;
                        }
                    }
                    bb &= bb - 1;
                }
            }

            // Pawn structure evaluation
            let pawns = board.bitboards[cidx][piece_index(PieceType::Pawn)];
            let opp_pawns = board.bitboards[color_idx(opposite(c))][piece_index(PieceType::Pawn)];
            for file in 0usize..8 {
                let mut count = 0;
                for rank in 0usize..8 {
                    let sq = rank * 8 + file;
                    if (pawns >> sq) & 1 != 0 { count += 1; }
                }
                if count > 1 {
                    score -= sign * Self::DOUBLED_PAWN_PENALTY * (count as i32 - 1);
                }
            }
            for rank in 0usize..8 {
                for file in 0usize..8 {
                    let sq = rank * 8 + file;
                    if (pawns >> sq) & 1 == 0 { continue; }
                    let mut isolated = true;
                    if file > 0 {
                        for r in 0usize..8 {
                            let idx = r * 8 + (file - 1);
                            if (pawns >> idx) & 1 != 0 { isolated = false; break; }
                        }
                    }
                    if file < 7 {
                        for r in 0usize..8 {
                            let idx = r * 8 + (file + 1);
                            if (pawns >> idx) & 1 != 0 { isolated = false; break; }
                        }
                    }
                    if isolated {
                        score -= sign * Self::ISOLATED_PAWN_PENALTY;
                    }
                    let mut blocked = false;
                    if c == Color::White {
                        for r in (rank+1)..8 {
                            for f in file.saturating_sub(1usize)..=usize::min(file+1,7) {
                                let idx = r * 8 + f;
                                if (opp_pawns >> idx) & 1 != 0 { blocked = true; break; }
                            }
                            if blocked { break; }
                        }
                    } else {
                        for r in (0..rank).rev() {
                            for f in file.saturating_sub(1usize)..=usize::min(file+1,7) {
                                let idx = r * 8 + f;
                                if (opp_pawns >> idx) & 1 != 0 { blocked = true; break; }
                            }
                            if blocked { break; }
                        }
                    }
                    if !blocked {
                        score += sign * Self::PASSED_PAWN_BONUS;
                    }
                }
            }

            // King safety
            if let Some((kx, ky)) = board.find_king(c) {
                let dir: isize = if c == Color::White { 1 } else { -1 };
                let ny = ky as isize + dir;
                if ny >= 0 && ny < 8 {
                    for dx in -1..=1 {
                        let nx = kx as isize + dx;
                        if nx >= 0 && nx < 8 {
                            let px = nx as usize;
                            let py = ny as usize;
                            if board.get_index(px, py).filter(|p| p.piece_type == PieceType::Pawn && p.color == c).is_none() {
                                score -= sign * Self::KING_SAFETY_PENALTY;
                            }
                        }
                    }
                }
            }
        }
        if board.piece_count_color(PieceType::Bishop, Color::White) >= 2 {
            score += if color == Color::White { BISHOP_PAIR } else { -BISHOP_PAIR };
        }
        if board.piece_count_color(PieceType::Bishop, Color::Black) >= 2 {
            score += if color == Color::Black { BISHOP_PAIR } else { -BISHOP_PAIR };
        }
        score
    }

    fn cheapest_attacker(board: &mut Board, color: Color, tx: usize, ty: usize) -> Option<((usize,usize), Piece)> {
        let target = Board::index_to_algebraic(tx, ty)?;
        let mut best: Option<((usize,usize), Piece)> = None;
        for y in 0..8 { for x in 0..8 {
            if let Some(p) = board.get_index(x,y) {
                if p.color == color {
                    if let Some(from) = Board::index_to_algebraic(x,y) {
                        if board.pseudo_legal_moves(&from).iter().any(|m| m == &target) && board.is_legal(&from, &target, color) {
                            if best.as_ref().map_or(true, |(_,bp)| Self::piece_value(p.piece_type) < Self::piece_value(bp.piece_type)) {
                                best = Some(((x,y), p));
                            }
                        }
                    }
                }
            }
        }}
        best
    }

    fn see_rec(&self, board: &mut Board, color: Color, tx: usize, ty: usize) -> i32 {
        if let Some(((sx,sy), _piece)) = Self::cheapest_attacker(board, color, tx, ty) {
            let from = Board::index_to_algebraic(sx, sy).unwrap();
            let to = Board::index_to_algebraic(tx, ty).unwrap();
            if let Some(state) = board.make_move_state(&from, &to) {
                let gain = Self::piece_value(state.captured.unwrap().piece_type) - self.see_rec(board, opposite(color), tx, ty);
                board.unmake_move(state);
                return gain.max(0);
            }
        }
        0
    }

    #[inline(always)]
    fn static_exchange_eval(&self, board: &Board, s: &String, e: &String) -> i32 {
        if let (Some((sx,sy)), Some((ex,ey))) = (Board::algebraic_to_index(s), Board::algebraic_to_index(e)) {
            if board.get_index(sx,sy).is_none() || board.get_index(ex,ey).is_none() { return 0; }
            let mut b = board.clone();
            let from = Board::index_to_algebraic(sx, sy).unwrap();
            let to = Board::index_to_algebraic(ex, ey).unwrap();
            if let Some(state) = b.make_move_state(&from, &to) {
                let gain = Self::piece_value(state.captured.unwrap().piece_type) - self.see_rec(&mut b, opposite(board.get_index(sx,sy).unwrap().color), ex, ey);
                b.unmake_move(state);
                return gain;
            }
        }
        0
    }

    #[inline(always)]
    fn lmr_value(depth: u32, idx: usize) -> u32 {
        if depth < 3 || idx < 3 {
            return 0;
        }
        let d = (depth as f64).ln();
        let m = ((idx + 1) as f64).ln();
        let mut r = (d * m / 1.5) as i32;
        if r < 1 { r = 1; }
        if r as u32 > depth - 1 { r = (depth - 1) as i32; }
        r as u32
    }

    fn probe_syzygy(&self, board: &Board, color: Color, ply: usize) -> Option<i32> {
        let tb = self.tb.as_ref()?;
        if board.piece_count_all() > tb.max_pieces() {
            return None;
        }
        let fen = board.to_fen(color);
        let pos: Chess = fen.parse::<Fen>().ok()?.into_position(CastlingMode::Standard).ok()?;
        let wdl = tb.probe_wdl(&pos).ok()?.after_zeroing();
        Some(match wdl {
            Wdl::Win | Wdl::CursedWin => 10000 - ply as i32,
            Wdl::Loss | Wdl::BlessedLoss => -10000 + ply as i32,
            Wdl::Draw => 0,
        })
    }

    fn move_score(&self, board: &Board, s: &String, e: &String, ply: usize, prev: Option<&(String,String)>) -> i32 {
        let mut score = 0;
        let capture = if let Some((ex,ey)) = Board::algebraic_to_index(e) {
            board.get_index(ex,ey).is_some()
        } else { false };
        if capture {
            score += *self.capture_history.get(&(s.clone(), e.clone())).unwrap_or(&0);
            if let Some((ex,ey)) = Board::algebraic_to_index(e) {
                if let Some(p) = board.get_index(ex,ey) {
                    score += Self::piece_value(p.piece_type) * 10;
                }
            }
            if self.static_exchange_eval(board,s,e) < 0 {
                score -= 1000;
            }
        } else {
            score += *self.quiet_history.get(&(s.clone(), e.clone())).unwrap_or(&0);
            if let Some(k) = self.killers.get(ply) {
                if let Some(m) = &k[0] { if m.0 == *s && m.1 == *e { score += 10_000; } }
                if let Some(m) = &k[1] { if m.0 == *s && m.1 == *e { score += 9_000; } }
            }
        }
        if let Some(pmv) = prev {
            score += *self.cont_history.get(&(pmv.clone(), (s.clone(),e.clone()))).unwrap_or(&0);
        }
        score
    }

    #[inline(always)]
    fn quiescence(&mut self, board: &mut Board, color: Color, mut alpha: i32, beta: i32) -> i32 {
        let stand_pat = Self::evaluate(board, color);
        if stand_pat >= beta { return beta; }
        if stand_pat > alpha { alpha = stand_pat; }
        let moves = board.capture_moves_fast(color);
        for (s,e) in moves {
            if let Some(state) = board.make_move_state(&s,&e) {
                let score = -self.quiescence(board, opposite(color), -beta, -alpha);
                board.unmake_move(state);
                if score >= beta { return beta; }
                if score > alpha { alpha = score; }
            }
        }
        alpha
    }

    fn pvs(&mut self, board: &mut Board, color: Color, depth: u32, mut alpha: i32, mut beta: i32, ply: usize, prev_move: Option<(String,String)>, use_iir: bool) -> i32 {
        const MATE_VALUE: i32 = 10000;

        // Mate Distance Pruning
        let mate_max = MATE_VALUE - ply as i32;
        if beta > mate_max { beta = mate_max; }
        let mate_min = -mate_max;
        if alpha < mate_min { alpha = mate_min; }
        if alpha >= beta { return alpha; }
        if use_iir && depth >= 5 && !board.in_check(color) {
            let mut d = depth - 2;
            let mut score = self.pvs(board, color, d, alpha, beta, ply, prev_move.clone(), false);
            while d < depth && score > alpha && score < beta {
                d += 1;
                score = self.pvs(board, color, d, alpha, beta, ply, prev_move.clone(), false);
            }
            return score;
        }
        let alpha_orig = alpha;
        let hash = board.hash(color);
        let mut tt_best: Option<(String, String)> = None;
        if let Some(entry) = self.tt.get(hash) {
            if entry.depth >= depth {
                match entry.bound {
                    Bound::Exact => return entry.value,
                    Bound::Lower => alpha = alpha.max(entry.value),
                    Bound::Upper => {}
                }
                if alpha >= beta { return entry.value; }
            }
            if let Some((fs,ts)) = entry.best {
                let f = Board::index_to_algebraic((fs % 8) as usize, (fs / 8) as usize);
                let t = Board::index_to_algebraic((ts % 8) as usize, (ts / 8) as usize);
                if let (Some(ff), Some(tt)) = (f,t) { tt_best = Some((ff,tt)); }
            }
        }

        if let Some(tb_val) = self.probe_syzygy(board, color, ply) {
            return tb_val;
        }

        if depth == 0 { return self.quiescence(board, color, alpha, beta); }

        // Reverse Futility Pruning
        if depth <= 3 && !board.in_check(color) {
            let eval = Self::evaluate(board, color);
            if eval - RFP_MARGIN[depth as usize] >= beta {
                return eval;
            }
        }

        if depth >= 3 && !board.in_check(color) && board.piece_count_total(color) > 3 {
            let r = if depth > 6 { 3 } else { 2 };
            let ep = board.en_passant;
            let score = -self.pvs(board, opposite(color), depth - 1 - r, -beta, -beta + 1, ply + 1, prev_move.clone(), true);
            board.en_passant = ep;
            if score >= beta { return beta; }
        }

        let all_moves = board.all_legal_moves_fast(color);
        if all_moves.is_empty() {
            if board.in_check(color) { return -10000 + ply as i32; }
            return 0;
        }
        let mut captures = Vec::new();
        let mut quiets = Vec::new();
        for m in all_moves {
            if let Some((ex,ey)) = Board::algebraic_to_index(&m.1) {
                if board.get_index(ex,ey).is_some() {
                    captures.push(m);
                } else {
                    quiets.push(m);
                }
            }
        }
        captures.sort_by_key(|(s,e)| -self.move_score(board,s,e,ply,prev_move.as_ref()));
        quiets.sort_by_key(|(s,e)| -self.move_score(board,s,e,ply,prev_move.as_ref()));
        let mut moves = captures;
        moves.extend(quiets);
        if let Some((bs, be)) = tt_best {
            if let Some(pos) = moves.iter().position(|(s,e)| *s == bs && *e == be) {
                moves.swap(0, pos);
            }
        }

        let mut best_move = None;
        let in_check_now = board.in_check(color);
        let mut skip_quiets = false;
        for (idx,(s,e)) in moves.iter().enumerate() {
            let capture_flag = if let Some((ex,ey)) = Board::algebraic_to_index(e) {
                board.get_index(ex,ey).is_some()
            } else { false };
            if !in_check_now && !capture_flag && depth <= 4 && idx >= LMP_LIMITS[depth as usize] {
                continue;
            }
            if skip_quiets && !capture_flag { continue; }
            if !in_check_now && !capture_flag && depth <= HLP_THRESHOLD && idx > 0 {
                let mv_score = self.move_score(board,s,e,ply,prev_move.as_ref());
                if mv_score < HLP_BASE {
                    skip_quiets = true;
                    continue;
                }
            }
            if let Some(state) = board.make_move_state(s,e) {
                let mut new_depth = depth - 1;
                let capture = state.captured.is_some();
                if depth > 2 && !capture && !in_check_now {
                    let mut r = Self::lmr_value(depth, idx + 1);
                    if board.in_check(opposite(color)) { r = r.saturating_sub(1); }
                    new_depth = new_depth.saturating_sub(r);
                }
                let mut score;
                if idx == 0 {
                    score = -self.pvs(board, opposite(color), new_depth, -beta, -alpha, ply + 1, Some((s.clone(),e.clone())), true);
                } else {
                    score = -self.pvs(board, opposite(color), new_depth, -alpha-1, -alpha, ply + 1, Some((s.clone(),e.clone())), true);
                    if score > alpha && score < beta {
                        score = -self.pvs(board, opposite(color), new_depth, -beta, -alpha, ply + 1, Some((s.clone(),e.clone())), true);
                    }
                }
                if depth > 1 && score >= beta - SINGULAR_MARGIN {
                    let ext_depth = std::cmp::min(new_depth.saturating_add(1), depth - 1);
                    if ext_depth > 0 {
                        let ext_score = -self.pvs(board, opposite(color), ext_depth, -beta, -alpha, ply + 1, Some((s.clone(),e.clone())), true);
                        score = ext_score;
                    }
                }
                board.unmake_move(state);
                if score >= beta {
                    if !capture {
                        if self.killers.len() <= ply {
                            self.killers.resize(ply + 1, [None, None]);
                        }
                        let k = &mut self.killers[ply];
                        if k[0].as_ref() != Some(&(s.clone(),e.clone())) {
                            k[1] = k[0].clone();
                            k[0] = Some((s.clone(),e.clone()));
                        }
                    }
                    if capture {
                        *self.capture_history.entry((s.clone(),e.clone())).or_insert(0) += (depth * depth) as i32;
                    } else {
                        *self.quiet_history.entry((s.clone(),e.clone())).or_insert(0) += (depth * depth) as i32;
                    }
                    if let Some(pmv) = &prev_move {
                        *self.cont_history.entry((pmv.clone(), (s.clone(),e.clone()))).or_insert(0) += (depth * depth) as i32;
                    }
                    let best_idx = Board::algebraic_to_index(s).and_then(|(sx,sy)| {
                        Board::algebraic_to_index(e).map(|(ex,ey)| ((sy*8+sx) as u8, (ey*8+ex) as u8))
                    });
                    self.tt.store(hash, TTEntry { depth, value: beta, bound: Bound::Lower, best: best_idx });
                    return beta;
                } else {
                    if capture {
                        *self.capture_history.entry((s.clone(),e.clone())).or_insert(0) -= (depth * depth) as i32;
                    } else {
                        *self.quiet_history.entry((s.clone(),e.clone())).or_insert(0) -= (depth * depth) as i32;
                    }
                    if let Some(pmv) = &prev_move {
                        *self.cont_history.entry((pmv.clone(), (s.clone(),e.clone()))).or_insert(0) -= (depth * depth) as i32;
                    }
                }
                if score > alpha {
                    alpha = score;
                    best_move = Some((s.clone(), e.clone()));
                }
            }
        }

        let bound = if alpha <= alpha_orig { Bound::Upper } else { Bound::Exact };
        let best_idx = best_move.as_ref().and_then(|(s,e)| {
            let fs = Board::algebraic_to_index(s)?;
            let ts = Board::algebraic_to_index(e)?;
            Some(((fs.1*8 + fs.0) as u8, (ts.1*8 + ts.0) as u8))
        });
        self.tt.store(hash, TTEntry { depth, value: alpha, bound, best: best_idx });
        alpha
    }

    #[allow(dead_code)]
    fn negamax(&mut self, board: &mut Board, color: Color, depth: u32, alpha: i32, beta: i32, ply: usize, prev_move: Option<(String,String)>) -> i32 {
        self.pvs(board, color, depth, alpha, beta, ply, prev_move, true)
    }

    fn best_move_single(&mut self, game: &mut Game) -> Option<(String, String)> {
        const ASPIRATION: i32 = 50;
        let color = game.current_turn;
        let root_hash = game.board.hash(color);
        let mut guess = 0;
        let mut best_move = None;

        for d in 1..=self.depth {
            let mut alpha = -100000;
            let mut beta = 100000;
            if d > 1 {
                alpha = guess - ASPIRATION;
                beta = guess + ASPIRATION;
            }

            loop {
                let mut board = game.board.clone();
                let score = self.pvs(&mut board, color, d, alpha, beta, 0, None, true);

                if score <= alpha {
                    alpha -= ASPIRATION;
                    continue;
                }
                if score >= beta {
                    beta += ASPIRATION;
                    continue;
                }

                guess = score;
                if let Some(entry) = self.tt.get(root_hash) {
                    if let Some((fs,ts)) = entry.best {
                        if let (Some(f), Some(t)) = (
                            Board::index_to_algebraic((fs % 8) as usize, (fs / 8) as usize),
                            Board::index_to_algebraic((ts % 8) as usize, (ts / 8) as usize),
                        ) {
                            best_move = Some((f,t));
                        }
                    }
                }
                break;
            }
        }

        best_move
    }

    fn best_move_parallel(&self, game: &mut Game) -> Option<(String, String)> {
        use rayon::prelude::*;
        let color = game.current_turn;
        let moves = game.board.all_legal_moves_fast(color);
        if moves.is_empty() { return None; }
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.threads)
            .build()
            .expect("thread pool");
        let depth = self.depth;
        let res = pool.install(|| {
            moves.par_iter()
                .map(|(s,e)| {
                    let mut engine = self.clone();
                    let mut board = game.board.clone();
                    if let Some(state) = board.make_move_state(s,e) {
                        let score = -engine.pvs(&mut board, opposite(color), depth - 1, -100000, 100000, 0, None, true);
                        board.unmake_move(state);
                        (score, s.clone(), e.clone())
                    } else {
                        (i32::MIN, s.clone(), e.clone())
                    }
                })
                .max_by_key(|(sc,_,_)| *sc)
        });
        res.map(|(_,s,e)| (s,e))
    }

    pub fn best_move(&mut self, game: &mut Game) -> Option<(String, String)> {
        self.tt.next_age();
        if self.threads <= 1 {
            self.best_move_single(game)
        } else {
            self.best_move_parallel(game)
        }
    }
}

#[inline(always)]
fn opposite(c: Color) -> Color {
    match c { Color::White => Color::Black, Color::Black => Color::White }
}
