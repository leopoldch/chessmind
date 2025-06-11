use crate::board::Board;
use crate::pieces::Color;

pub struct Game {
    pub board: Board,
    pub current_turn: Color,
    pub history: Vec<(String, String)>,
    pub result: Option<Color>,
}

impl Game {
    pub fn new() -> Self {
        let mut board = Board::new();
        board.setup_standard();
        Self { board, current_turn: Color::White, history: Vec::new(), result: None }
    }

    pub fn make_move(&mut self, start: &str, end: &str) -> bool {
        if !self.board.is_legal(start, end, self.current_turn) {
            return false;
        }
        if self.board.make_move_state(start, end).is_some() {
            self.history.push((start.to_string(), end.to_string()));
            self.current_turn = if self.current_turn == Color::White { Color::Black } else { Color::White };
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
}
