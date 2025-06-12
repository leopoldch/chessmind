use crate::board::Board;
use crate::pieces::Color;

pub struct Game {
    pub board: Board,
    pub current_turn: Color,
    pub history: Vec<(String, String)>,
    pub hash_history: Vec<u64>,
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
            result: None,
        }
    }

    pub fn make_move(&mut self, start: &str, end: &str) -> bool {
        if !self.board.is_legal(start, end, self.current_turn) {
            return false;
        }
        if self.board.make_move_state(start, end).is_some() {
            self.history.push((start.to_string(), end.to_string()));
            self.current_turn = if self.current_turn == Color::White { Color::Black } else { Color::White };
            self.hash_history.push(self.board.hash(self.current_turn));
            if self.board.all_legal_moves(self.current_turn).is_empty() {
                if self.board.in_check(self.current_turn) {
                    self.result = Some(if self.current_turn == Color::White { Color::Black } else { Color::White });
                }
            }
            true
        } else {
            false
        }
    }

    pub fn legal_moves(&mut self) -> Vec<(String, String)> {
        self.board.all_legal_moves(self.current_turn)
    }

    pub fn repetition_count(&self, hash: u64) -> usize {
        self.hash_history.iter().filter(|&&h| h == hash).count()
    }
}
