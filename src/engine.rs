use crate::board::{Board, color_idx};
use crate::pieces::{Color, PieceType};
use crate::game::Game;
use crate::transposition::{Table, TTEntry, Bound};
use std::collections::HashMap;

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
            tt: HashMap::new(),
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
        const VALUES: [i32;6] = [100,320,320,500,900,0];
        let mut score = 0;
        for c in 0..2 {
            let sign = if c == color_idx(color) { 1 } else { -1 };
            for p in 0..6 {
                let mut bb = board.bitboards[c][p];
                let val = VALUES[p];
                while bb != 0 {
                    score += sign * val;
                    bb &= bb - 1;
                }
            }
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
        self.tt.insert(hash, TTEntry { depth, value: best, bound, best: best_move });
        best
    }

    pub fn best_move(&mut self, game: &mut Game) -> Option<(String, String)> {
        let moves = game.board.all_legal_moves(game.current_turn);
        let mut best = None;
        let mut best_score = -100000;
        for (s,e) in moves {
            if let Some(state) = game.board.make_move_state(&s,&e) {
                let score = -self.negamax(&mut game.board, opposite(game.current_turn), self.depth-1, -100000, 100000, 1);
                game.board.unmake_move(state);
                if score > best_score {
                    best_score = score;
                    best = Some((s,e));
                }
            }
        }
        best
    }
}

fn opposite(c: Color) -> Color {
    match c { Color::White => Color::Black, Color::Black => Color::White }
}
