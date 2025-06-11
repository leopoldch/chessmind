use crate::board::Board;
use crate::pieces::{Color, PieceType};
use crate::game::Game;

pub struct Engine {
    pub depth: u32,
}

impl Engine {
    pub fn new(depth: u32) -> Self {
        Self { depth }
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
        let mut score = 0;
        for y in 0..8 {
            for x in 0..8 {
                if let Some(p) = board.get_index(x, y) {
                    let val = Self::piece_value(p.piece_type);
                    if p.color == color { score += val; } else { score -= val; }
                }
            }
        }
        score
    }

    fn negamax(board: &mut Board, color: Color, depth: u32, mut alpha: i32, beta: i32) -> i32 {
        if depth == 0 {
            return Self::evaluate(board, color);
        }
        let moves = board.all_legal_moves(color);
        if moves.is_empty() {
            if board.in_check(color) { return -10000 + depth as i32; }
            return 0;
        }
        let mut best = -100000;
        for (s, e) in moves {
            if let Some(state) = board.make_move_state(&s, &e) {
                let score = -Self::negamax(board, opposite(color), depth - 1, -beta, -alpha);
                board.unmake_move(state);
                if score > best { best = score; }
                if best > alpha { alpha = best; }
                if alpha >= beta { break; }
            }
        }
        best
    }

    pub fn best_move(&self, game: &mut Game) -> Option<(String, String)> {
        let moves = game.board.all_legal_moves(game.current_turn);
        let mut best_score = -100000;
        let mut best_move = None;
        for (s, e) in moves {
            if let Some(state) = game.board.make_move_state(&s, &e) {
                let score = -Self::negamax(&mut game.board, opposite(game.current_turn), self.depth - 1, -100000, 100000);
                game.board.unmake_move(state);
                if score > best_score {
                    best_score = score;
                    best_move = Some((s, e));
                }
            }
        }
        best_move
    }
}

fn opposite(c: Color) -> Color {
    match c { Color::White => Color::Black, Color::Black => Color::White }
}
