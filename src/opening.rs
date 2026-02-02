use crate::board::Board;
use crate::pieces::Color;

const BOOK_LINES: &[&[&str]] = &[
    &[
        "e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "f8c5", "c2c3", "g8f6", "d2d3",
    ],
    &[
        "d2d4", "d7d5", "c2c4", "e7e6", "b1c3", "g8f6", "c1g5", "f8e7",
    ],
    &[
        "e2e4", "c7c5", "g1f3", "d7d6", "d2d4", "c5d4", "f3d4", "g8f6", "b1c3", "a7a6",
    ],
    &[
        "c2c4", "e7e5", "b1c3", "g8f6", "g2g3", "d7d5", "c4d5", "f6d5", "f1g2", "b8b6", "g1f3",
    ],
    &[
        "d2d4", "g8f6", "c2c4", "g7g6", "b1c3", "f8g7", "e2e4", "d7d6", "g1f3", "e8g8",
    ],
    &[
        "e2e4", "e7e6", "d2d4", "d7d5", "b1c3", "g8f6", "c1g5", "f8e7",
    ],
    &[
        "e2e4", "c7c6", "d2d4", "d7d5", "b1c3", "d5e4", "c3e4", "c8f5", "g1g3", "f5g6",
    ],
];

pub fn book_move(
    history: &[(String, String)],
    board: &Board,
    color: Color,
) -> Option<(String, String)> {
    let played: Vec<String> = history.iter().map(|(s, e)| format!("{}{}", s, e)).collect();

    'outer: for line in BOOK_LINES {
        if played.len() >= line.len() {
            continue;
        }

        for (idx, mv) in played.iter().enumerate() {
            if mv != line[idx] {
                continue 'outer;
            }
        }

        let next = line[played.len()];
        if next.len() != 4 {
            continue;
        }
        let (s, e) = next.split_at(2);
        let mut board_copy = board.clone();
        if board_copy.is_legal(s, e, color) {
            return Some((s.to_string(), e.to_string()));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::book_move;
    use crate::game::Game;

    #[test]
    fn suggests_first_white_move() {
        let game = Game::new();
        let mv = book_move(&game.history, &game.board, game.current_turn);
        assert_eq!(mv, Some(("e2".into(), "e4".into())));
    }

    #[test]
    fn suggests_reply_for_black() {
        let mut game = Game::new();
        assert!(game.make_move("e2", "e4"));
        let mv = book_move(&game.history, &game.board, game.current_turn);
        assert_eq!(mv, Some(("e7".into(), "e5".into())));
    }
}
