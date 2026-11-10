use crate::board::Board;
use crate::pieces::Color;

/// Why a game ended in a draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawReason {
    Stalemate,
    FiftyMoveRule,
    ThreefoldRepetition,
    InsufficientMaterial,
}

/// Final result of a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Win(Color),
    Draw(DrawReason),
}

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

    /// Result of the game in the current position, draws included (`result`
    /// only records checkmates). `None` while the game is still going.
    pub fn outcome(&mut self) -> Option<Outcome> {
        let side = self.current_turn;
        if !self.board.has_legal_move(side) {
            return Some(if self.board.in_check_fast(side) {
                Outcome::Win(match side {
                    Color::White => Color::Black,
                    Color::Black => Color::White,
                })
            } else {
                Outcome::Draw(DrawReason::Stalemate)
            });
        }
        if self.board.halfmove >= 100 {
            return Some(Outcome::Draw(DrawReason::FiftyMoveRule));
        }
        if self.repetition_count(self.board.hash(side)) >= 3 {
            return Some(Outcome::Draw(DrawReason::ThreefoldRepetition));
        }
        if self.insufficient_material() {
            return Some(Outcome::Draw(DrawReason::InsufficientMaterial));
        }
        None
    }

    /// Neither side can ever mate: bare kings, a single minor piece, or only
    /// bishops that all stand on squares of the same colour.
    fn insufficient_material(&self) -> bool {
        use crate::pieces::PieceType;
        let count = |pt| self.board.piece_count(pt);
        if count(PieceType::Pawn) + count(PieceType::Rook) + count(PieceType::Queen) > 0 {
            return false;
        }
        let knights = count(PieceType::Knight);
        let bishops = count(PieceType::Bishop);
        if knights + bishops <= 1 {
            return true;
        }
        if knights > 0 {
            return false;
        }
        let mut colours = [false; 2];
        for y in 0..8 {
            for x in 0..8 {
                if matches!(self.board.get_index(x, y), Some(p) if p.piece_type == PieceType::Bishop)
                {
                    colours[(x + y) % 2] = true;
                }
            }
        }
        !(colours[0] && colours[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pieces::{Piece, PieceType};

    fn game_from_board(board: Board, current_turn: Color) -> Game {
        let hash = board.hash(current_turn);
        Game {
            board,
            current_turn,
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

    fn game_with(pieces: &[(&str, PieceType, Color)], turn: Color) -> Game {
        let mut board = Board::new();
        for &(sq, piece_type, color) in pieces {
            board.set(sq, Some(Piece { piece_type, color }));
        }
        board.castling = [[false, false], [false, false]];
        game_from_board(board, turn)
    }

    #[test]
    fn outcome_reports_mate_and_every_kind_of_draw() {
        use PieceType::*;
        let mut fresh = Game::new();
        assert_eq!(fresh.outcome(), None);

        let mut mate = game_with(
            &[("h8", King, Color::Black), ("g7", Queen, Color::White), ("g6", King, Color::White)],
            Color::Black,
        );
        assert_eq!(mate.outcome(), Some(Outcome::Win(Color::White)));

        let mut stalemate = game_with(
            &[("h8", King, Color::Black), ("g6", Queen, Color::White), ("f7", King, Color::White)],
            Color::Black,
        );
        assert_eq!(stalemate.outcome(), Some(Outcome::Draw(DrawReason::Stalemate)));

        let mut bare = game_with(
            &[("h8", King, Color::Black), ("a1", King, Color::White), ("c3", Bishop, Color::White)],
            Color::White,
        );
        assert_eq!(bare.outcome(), Some(Outcome::Draw(DrawReason::InsufficientMaterial)));

        let mut fifty = game_with(
            &[("h8", King, Color::Black), ("a1", King, Color::White), ("c3", Rook, Color::White)],
            Color::White,
        );
        assert_eq!(fifty.outcome(), None);
        fifty.board.halfmove = 100;
        assert_eq!(fifty.outcome(), Some(Outcome::Draw(DrawReason::FiftyMoveRule)));

        let mut rep = Game::new();
        for _ in 0..2 {
            for (from, to) in [("g1", "f3"), ("g8", "f6"), ("f3", "g1"), ("f6", "g8")] {
                assert!(rep.make_move(from, to));
            }
        }
        assert_eq!(rep.outcome(), Some(Outcome::Draw(DrawReason::ThreefoldRepetition)));
    }

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

    #[test]
    fn repetition_count_distinguishes_castling_rights() {
        let mut board = Board::new();
        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "h1",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::White,
            }),
        );
        board.set(
            "a8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.castling = [[true, false], [false, false]];

        let initial_hash = board.hash(Color::White);
        let mut game = game_from_board(board, Color::White);

        assert!(game.make_move("h1", "h2"));
        assert!(game.make_move("a8", "a7"));
        assert!(game.make_move("h2", "h1"));
        assert!(game.make_move("a7", "a8"));

        let current_hash = game.board.hash(game.current_turn);
        assert_eq!(game.current_turn, Color::White);
        assert_ne!(current_hash, initial_hash);
        assert_eq!(game.repetition_count(current_hash), 1);
    }

    #[test]
    fn repetition_count_distinguishes_en_passant_rights() {
        let mut board = Board::new();
        board.set(
            "b1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "g8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "e5",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "d7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.castling = [[false, false], [false, false]];

        let mut game = game_from_board(board, Color::Black);
        assert!(game.make_move("d7", "d5"));

        let with_ep_hash = game.board.hash(game.current_turn);
        assert!(game.make_move("b1", "c1"));
        assert!(game.make_move("g8", "f8"));
        assert!(game.make_move("c1", "b1"));
        assert!(game.make_move("f8", "g8"));

        let current_hash = game.board.hash(game.current_turn);
        assert_eq!(game.current_turn, Color::White);
        assert_ne!(current_hash, with_ep_hash);
        assert_eq!(game.repetition_count(current_hash), 1);
    }
}
