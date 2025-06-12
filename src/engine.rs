use crate::board::{Board, color_idx};
use crate::pieces::{Color, PieceType};
use crate::game::Game;
use crate::transposition::{Table, TTEntry, Bound, TABLE_SIZE};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use rayon::prelude::*;

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

#[derive(Clone)]
pub struct Engine {
    pub depth: u32,
    tt: Table,
    killers: Vec<[Option<(String,String)>;2]>,
    history: HashMap<(String,String), i32>,
}

impl Engine {
    pub fn new(depth: u32) -> Self {
        Self {
            depth,
            tt: Table::new(NonZeroUsize::new(TABLE_SIZE).unwrap()),
            killers: vec![[None, None]; (depth as usize)+1],
            history: HashMap::new(),
        }
    }

    fn piece_value(t: PieceType) -> i32 {
        match t {
            PieceType::Pawn => 100,
            PieceType::Knight | PieceType::Bishop => 320,
            PieceType::Rook => 500,
            PieceType::Queen => 900,
            PieceType::King => 0,
        }
    }

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
                    bb &= bb - 1;
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

    fn move_score(&self, board: &Board, s: &String, e: &String, ply: usize) -> i32 {
        let mut score = *self.history.get(&(s.clone(), e.clone())).unwrap_or(&0);
        if let Some(k) = self.killers.get(ply) {
            if let Some(m) = &k[0] { if m.0 == *s && m.1 == *e { score += 10_000; } }
            if let Some(m) = &k[1] { if m.0 == *s && m.1 == *e { score += 9_000; } }
        }
        if let Some((ex,ey)) = Board::algebraic_to_index(e) {
            if let Some(p) = board.get_index(ex,ey) {
                score += Self::piece_value(p.piece_type) * 10;
            }
        }
        score
    }

    fn quiescence(&mut self, board: &mut Board, color: Color, mut alpha: i32, beta: i32) -> i32 {
        let stand_pat = Self::evaluate(board, color);
        if stand_pat >= beta { return beta; }
        if stand_pat > alpha { alpha = stand_pat; }
        let moves = board.capture_moves(color);
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

    fn negamax(&mut self, board: &mut Board, color: Color, depth: u32, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        let alpha_orig = alpha;
        let hash = board.hash(color);
        if let Some(entry) = self.tt.get(&hash) {
            if entry.depth >= depth {
                match entry.bound {
                    Bound::Exact => return entry.value,
                    Bound::Lower => alpha = alpha.max(entry.value),
                    Bound::Upper => {}
                }
                if alpha >= beta { return entry.value; }
            }
        }

        if depth == 0 { return self.quiescence(board, color, alpha, beta); }

        if depth >= 3 && !board.in_check(color) {
            let ep = board.en_passant;
            let score = -self.negamax(board, opposite(color), depth - 1 - 2, -beta, -beta+1, ply+1);
            board.en_passant = ep;
            if score >= beta { return beta; }
        }

        let mut moves = board.all_legal_moves(color);
        if moves.is_empty() {
            if board.in_check(color) { return -10000 + ply as i32; }
            return 0;
        }
        moves.sort_by_key(|(s,e)| -self.move_score(board,s,e,ply));

        let mut best_move = None;
        let mut best = -100000;
        for (idx,(s,e)) in moves.iter().enumerate() {
            if let Some(state) = board.make_move_state(s,e) {
                let mut new_depth = depth -1;
                let capture = board.get_index(state.end.0,state.end.1).is_some();
                if idx >= 3 && depth > 2 && !capture {
                    new_depth = new_depth.saturating_sub(1);
                }
                let score = -self.negamax(board, opposite(color), new_depth, -beta, -alpha, ply+1);
                board.unmake_move(state);
                if score > best {
                    best = score;
                    best_move = Some((s.clone(), e.clone()));
                }
                if score > alpha { alpha = score; }
                if alpha >= beta {
                    if !capture {
                        let k = &mut self.killers[ply];
                        if k[0].as_ref() != Some(&(s.clone(),e.clone())) {
                            k[1] = k[0].clone();
                            k[0] = Some((s.clone(),e.clone()));
                        }
                    }
                    *self.history.entry((s.clone(),e.clone())).or_insert(0) += (depth*depth) as i32;
                    break;
                }
            }
        }

        let bound = if best <= alpha_orig { Bound::Upper } else if best >= beta { Bound::Lower } else { Bound::Exact };
        self.tt.put(hash, TTEntry { depth, value: best, bound, best: best_move });
        best
    }

    pub fn best_move(&mut self, game: &mut Game) -> Option<(String, String)> {
        let moves = game.board.all_legal_moves(game.current_turn);
        let base_engine = self.clone();

        let results: Vec<((String, String), i32, Engine)> = moves
            .into_par_iter()
            .filter_map(|(s, e)| {
                let mut board = game.board.clone();
                if board.make_move_state(&s, &e).is_none() {
                    return None;
                }
                let hash = board.hash(opposite(game.current_turn));
                if game.repetition_count(hash) >= 2 {
                    return None;
                }
                let mut eng = base_engine.clone();
                let score = -eng.negamax(&mut board, opposite(game.current_turn), eng.depth-1, -100000, 100000, 1);
                Some(((s, e), score, eng))
            })
            .collect();

        let best = results
            .into_iter()
            .max_by_key(|(_, score, _)| *score);

        if let Some(((s, e), _score, eng)) = best {
            *self = eng;
            Some((s, e))
        } else {
            None
        }
    }
}

fn opposite(c: Color) -> Color {
    match c { Color::White => Color::Black, Color::Black => Color::White }
}
