use crate::board::Board;
use crate::pieces::Color;

pub struct Game {
    pub board: Board,
    pub current_turn: Color,
    pub history: Vec<(String, String)>,
    pub hash_history: Vec<u64>,
    pub hash_counts: std::collections::HashMap<u64, usize>,
    pub result: Option<Color>,
}

impl Game {
    pub fn new() -> Self {
        let mut board = Board::new();
        board.setup_standard();
        let hash = board.hash(Color::White);
        Self {
            board,
            current_turn: Color::White,
            history: Vec::new(),
            hash_history: vec![hash],
            hash_counts: {
                let mut m = std::collections::HashMap::new();
                m.insert(hash, 1);
                m
            },
            result: None,
        }
    }

    pub fn make_move(&mut self, start: &str, end: &str) -> bool {
        if start == end {
            return false;
        }
        let Some(mv) = self.board.encode_move(start, end, self.current_turn) else {
            return false;
        };
        if !self.board.is_move_legal_fast(mv, self.current_turn) {
            return false;
        }
        self.board.make_move_fast(mv, self.current_turn);
        {
            self.history.push((start.to_string(), end.to_string()));
            self.current_turn = if self.current_turn == Color::White {
                Color::Black
            } else {
                Color::White
            };
            let h = self.board.hash(self.current_turn);
            self.hash_history.push(h);
            *self.hash_counts.entry(h).or_insert(0) += 1;
            if self
                .board
                .all_legal_moves_fast(self.current_turn)
                .is_empty()
            {
                if self.board.in_check(self.current_turn) {
                    self.result = Some(if self.current_turn == Color::White {
                        Color::Black
                    } else {
                        Color::White
                    });
                }
            }
            true
        }
    }

    pub fn legal_moves(&mut self) -> Vec<(String, String)> {
        self.board.all_legal_moves_fast(self.current_turn)
    }

    pub fn repetition_count(&self, hash: u64) -> usize {
        *self.hash_counts.get(&hash).unwrap_or(&0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pieces::{Piece, PieceType};

    #[test]
    fn make_move_accepts_promotion_suffix() {
        let mut game = Game {
            board: Board::new(),
            current_turn: Color::White,
            history: Vec::new(),
            hash_history: Vec::new(),
            hash_counts: std::collections::HashMap::new(),
            result: None,
        };

        game.board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
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
            "e7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        let hash = game.board.hash(Color::White);
        game.hash_history.push(hash);
        game.hash_counts.insert(hash, 1);

        assert!(game.make_move("e7", "e8q"));
        assert_eq!(game.board.get("e8").unwrap().piece_type, PieceType::Queen);
    }
}
