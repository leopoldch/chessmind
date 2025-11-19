use crate::{
    game::Game,
    pieces::{Color, PieceType},
};
use regex::Regex;

pub fn parse_san(game: &mut Game, san: &str, color: Color) -> Option<(String, String)> {
    let mut san = san.replace("0", "O");
    san = san.trim_end_matches(|c| c == '+' || c == '#').to_string();
    let upper = san.to_uppercase();
    if upper == "O-O" {
        let start = if color == Color::White { "e1" } else { "e8" };
        let end = if color == Color::White { "g1" } else { "g8" };
        return Some((start.into(), end.into()));
    }
    if upper == "O-O-O" {
        let start = if color == Color::White { "e1" } else { "e8" };
        let end = if color == Color::White { "c1" } else { "c8" };
        return Some((start.into(), end.into()));
    }
    let re = Regex::new(r"^([NBRQK])?([a-h])?([1-8])?[x-]?([a-h][1-8])(=?[NBRQK])?$").ok()?;
    let caps = re.captures(&san)?;
    let mut piece_letter = caps.get(1).map(|m| m.as_str());
    let mut dfile = caps.get(2).map(|m| m.as_str());
    let drank = caps.get(3).map(|m| m.as_str());
    let dest = caps.get(4)?.as_str();
    if let Some(pl) = piece_letter {
        if pl.chars().all(|c| c.is_ascii_lowercase()) && "abcdefgh".contains(pl) && dfile.is_none()
        {
            dfile = Some(pl);
            piece_letter = None;
        }
    }
    let ptype = match piece_letter {
        Some("N") => PieceType::Knight,
        Some("B") => PieceType::Bishop,
        Some("R") => PieceType::Rook,
        Some("Q") => PieceType::Queen,
        Some("K") => PieceType::King,
        None => PieceType::Pawn,
        _ => return None,
    };
    let moves = game.board.all_legal_moves(color);
    let mut candidates = Vec::new();
    for (start, end) in moves {
        if end != dest {
            continue;
        }
        if let Some(piece) = game.board.get(&start) {
            if piece.piece_type != ptype {
                continue;
            }
            if let Some(df) = dfile {
                if &start[0..1] != df {
                    continue;
                }
            }
            if let Some(dr) = drank {
                if &start[1..2] != dr {
                    continue;
                }
            }
            candidates.push(start);
        }
    }
    if candidates.len() == 1 {
        Some((candidates.remove(0), dest.to_string()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Game;

    #[test]
    fn castle_kingside() {
        let mut game = Game::new();
        let mv = parse_san(&mut game, "O-O", Color::White).unwrap();
        assert_eq!(mv, ("e1".to_string(), "g1".to_string()));
    }

    #[test]
    fn queen_move() {
        let mut game = Game::new();
        // open path for queen
        game.make_move("d2", "d4");
        game.make_move("d7", "d5");
        let mv = parse_san(&mut game, "Qd3", Color::White).unwrap();
        assert_eq!(mv, ("d1".to_string(), "d3".to_string()));
    }
}
