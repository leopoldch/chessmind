//! FEN / EPD parsing and formatting.
//!
//! `parse_fen` accepts a full FEN (`pieces side castling ep halfmove fullmove`)
//! or an EPD record (the first four fields, optionally followed by operations
//! such as `bm e4; id "x"; hmvc 3; fmvn 12;`). Missing counters default to
//! halfmove 0 and fullmove 1.
//!
//! The resulting `Game` has its repetition history seeded with the start
//! position only, so threefold repetition counts from the FEN onwards.
//! `Game` has no halfmove clock: it is returned next to it in `FenPosition`.

use crate::board::Board;
use crate::game::Game;
use crate::pieces::{Color, Piece, PieceType};
use std::collections::HashMap;

pub const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// A position parsed from FEN/EPD.
pub struct FenPosition {
    pub game: Game,
    /// Plies since the last capture or pawn move (50-move rule counter).
    pub halfmove: u32,
    /// Full move number (starts at 1, incremented after Black's move).
    pub fullmove: u32,
}

/// Parse a FEN or EPD string into a `Game` plus move counters.
pub fn parse_fen(text: &str) -> Result<FenPosition, String> {
    let fields: Vec<&str> = text.split_whitespace().collect();
    if fields.len() < 2 {
        return Err("expected at least piece placement and side to move".into());
    }

    let mut board = Board::new();
    let ranks: Vec<&str> = fields[0].split('/').collect();
    if ranks.len() != 8 {
        return Err(format!("expected 8 ranks, found {}", ranks.len()));
    }
    for (i, rank) in ranks.iter().enumerate() {
        let y = 7 - i;
        let mut x = 0usize;
        for c in rank.chars() {
            if let Some(d) = c.to_digit(10) {
                if !(1..=8).contains(&d) {
                    return Err(format!("bad empty-square count '{c}'"));
                }
                x += d as usize;
                if x > 8 {
                    return Err(format!("rank {} is too long", 8 - i));
                }
                continue;
            }
            let color = if c.is_ascii_uppercase() {
                Color::White
            } else {
                Color::Black
            };
            let piece_type = match c.to_ascii_lowercase() {
                'p' => PieceType::Pawn,
                'n' => PieceType::Knight,
                'b' => PieceType::Bishop,
                'r' => PieceType::Rook,
                'q' => PieceType::Queen,
                'k' => PieceType::King,
                _ => return Err(format!("bad piece character '{c}'")),
            };
            if x > 7 {
                return Err(format!("rank {} is too long", 8 - i));
            }
            if piece_type == PieceType::Pawn && (y == 0 || y == 7) {
                return Err("pawn on the first or last rank".into());
            }
            board.set_index(x, y, Some(Piece { piece_type, color }));
            x += 1;
        }
        if x != 8 {
            return Err(format!("rank {} has {x} squares", 8 - i));
        }
    }
    for color in [Color::White, Color::Black] {
        if board.piece_count_color(PieceType::King, color) != 1 {
            return Err(format!("{color:?} must have exactly one king"));
        }
    }

    let turn = match fields[1] {
        "w" | "W" => Color::White,
        "b" | "B" => Color::Black,
        other => return Err(format!("bad side to move '{other}'")),
    };

    // Castling: keep only rights consistent with king/rook placement, so the
    // move generator never sees a right it cannot honour.
    let castling = fields.get(2).copied().unwrap_or("-");
    if castling != "-" && !castling.chars().all(|c| "KQkq".contains(c)) {
        return Err(format!("bad castling field '{castling}'"));
    }
    let has = |x: usize, y: usize, piece_type: PieceType, color: Color| {
        is_piece(&board, x, y, piece_type, color)
    };
    let rights = |color: Color, rank: usize, king_side: char, queen_side: char| {
        let king_home = has(4, rank, PieceType::King, color);
        [
            castling.contains(king_side) && king_home && has(7, rank, PieceType::Rook, color),
            castling.contains(queen_side) && king_home && has(0, rank, PieceType::Rook, color),
        ]
    };
    let castling_rights = [
        rights(Color::White, 0, 'K', 'Q'),
        rights(Color::Black, 7, 'k', 'q'),
    ];

    // En passant: keep the square only if it is plausible (right rank for the
    // side to move, empty, with the double-pushed pawn in front of it).
    let ep = fields.get(3).copied().unwrap_or("-");
    let mut ep_square = None;
    if ep != "-" {
        let (x, y) =
            Board::algebraic_to_index(ep).ok_or_else(|| format!("bad en passant field '{ep}'"))?;
        let (ep_rank, pawn_rank, pusher) = if turn == Color::White {
            (5, 4, Color::Black)
        } else {
            (2, 3, Color::White)
        };
        if y != ep_rank {
            return Err(format!("en passant square '{ep}' on the wrong rank"));
        }
        if board.get_index(x, y).is_none() && has(x, pawn_rank, PieceType::Pawn, pusher) {
            ep_square = Some((x, y));
        }
    }
    board.castling = castling_rights;
    board.en_passant = ep_square;

    // Counters: FEN has two integers; EPD may carry hmvc/fmvn operations.
    let int = |s: Option<&&str>| s.and_then(|v| v.trim_end_matches(';').parse::<u32>().ok());
    let (mut halfmove, mut fullmove) = (0u32, 1u32);
    if let (Some(h), Some(f)) = (int(fields.get(4)), int(fields.get(5))) {
        halfmove = h;
        fullmove = f.max(1);
    } else if fields.len() > 4 {
        let ops = fields[4..].join(" ");
        for op in ops.split(';') {
            let mut it = op.split_whitespace();
            match (it.next(), it.next().and_then(|v| v.parse::<u32>().ok())) {
                (Some("hmvc"), Some(v)) => halfmove = v,
                (Some("fmvn"), Some(v)) => fullmove = v.max(1),
                _ => {}
            }
        }
    }

    if board.in_check(opposite(turn)) {
        return Err("side not to move is in check".into());
    }

    let hash = board.hash(turn);
    let mut hash_counts = HashMap::new();
    hash_counts.insert(hash, 1);
    Ok(FenPosition {
        game: Game {
            board,
            current_turn: turn,
            history: Vec::new(),
            hash_history: vec![hash],
            hash_counts,
            result: None,
        },
        halfmove,
        fullmove,
    })
}

/// Convenience wrapper: just the `Game` (counters dropped).
pub fn game_from_fen(text: &str) -> Option<Game> {
    parse_fen(text).ok().map(|p| p.game)
}

/// Format a full FEN. The en passant square is written only when a pawn of
/// the side to move could actually capture there (the usual convention).
pub fn to_fen(game: &Game, halfmove: u32, fullmove: u32) -> String {
    let mut board = game.board.clone();
    if let Some((x, y)) = board.en_passant {
        let turn = game.current_turn;
        let from_y = if turn == Color::White {
            y.wrapping_sub(1)
        } else {
            y + 1
        };
        let capturer =
            |fx: usize| fx < 8 && from_y < 8 && is_piece(&board, fx, from_y, PieceType::Pawn, turn);
        if !(capturer(x.wrapping_sub(1)) || capturer(x + 1)) {
            board.en_passant = None;
        }
    }
    let fen = board.to_fen(game.current_turn);
    // Board::to_fen always ends with " 0 1": replace the counters.
    let base = fen.rsplitn(3, ' ').nth(2).unwrap_or(&fen);
    format!("{base} {halfmove} {}", fullmove.max(1))
}

/// Four-field EPD (placement, side, castling, en passant).
pub fn to_epd(game: &Game) -> String {
    let fen = to_fen(game, 0, 1);
    fen.rsplitn(3, ' ').nth(2).unwrap_or(&fen).to_string()
}

fn is_piece(board: &Board, x: usize, y: usize, piece_type: PieceType, color: Color) -> bool {
    matches!(board.get_index(x, y), Some(p) if p.piece_type == piece_type && p.color == color)
}

fn opposite(c: Color) -> Color {
    if c == Color::White {
        Color::Black
    } else {
        Color::White
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_round_trip() {
        let pos = parse_fen(STARTPOS).unwrap();
        assert_eq!(pos.halfmove, 0);
        assert_eq!(pos.fullmove, 1);
        assert_eq!(pos.game.current_turn, Color::White);
        assert_eq!(pos.game.board.castling, [[true, true], [true, true]]);
        assert_eq!(to_fen(&pos.game, 0, 1), STARTPOS);
        let reference = Game::new();
        assert_eq!(
            pos.game.board.hash(Color::White),
            reference.board.hash(Color::White)
        );
        let mut g = pos.game;
        assert_eq!(g.legal_moves().len(), 20);
    }

    #[test]
    fn parses_counters_side_castling_and_ep() {
        let fen = "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w Kq f6 0 3";
        let pos = parse_fen(fen).unwrap();
        assert_eq!(pos.halfmove, 0);
        assert_eq!(pos.fullmove, 3);
        assert_eq!(pos.game.board.castling, [[true, false], [false, true]]);
        assert_eq!(pos.game.board.en_passant, Some((5, 5)));
        let mut g = pos.game;
        assert!(g.legal_moves().iter().any(|(f, t)| f == "e5" && t == "f6"));
        assert!(g.make_move("e5", "f6"));
        assert!(
            g.board.get("f5").is_none(),
            "en passant capture removes pawn"
        );
        assert_eq!(to_fen(&parse_fen(fen).unwrap().game, 0, 3), fen);
    }

    #[test]
    fn parses_epd_with_operations() {
        let epd = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - bm Bb5; hmvc 2; fmvn 3; id \"x\";";
        let pos = parse_fen(epd).unwrap();
        assert_eq!(pos.halfmove, 2);
        assert_eq!(pos.fullmove, 3);
        let plain =
            parse_fen("r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq -").unwrap();
        assert_eq!((plain.halfmove, plain.fullmove), (0, 1));
        assert_eq!(plain.game.current_turn, Color::Black);
        assert_eq!(
            to_epd(&plain.game),
            "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq -"
        );
    }

    #[test]
    fn drops_inconsistent_rights_and_ep() {
        // No rook on h1 / a8 and an implausible en passant square.
        let pos = parse_fen("r3k3/8/8/8/8/8/8/R3K3 w KQkq e6 5 40").unwrap();
        assert_eq!(pos.game.board.castling, [[false, true], [false, true]]);
        assert_eq!(pos.game.board.en_passant, None);
        assert_eq!((pos.halfmove, pos.fullmove), (5, 40));
    }

    #[test]
    fn rejects_invalid() {
        for bad in [
            "",
            "8/8/8/8/8/8/8/8 w - -",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq -",
            "rnbqkbnr/pppppppp/9/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR x KQkq -",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNX w KQkq -",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq z9",
            "P3k3/8/8/8/8/8/8/4K3 w - -",
            // Black to move while White is in check.
            "4k3/8/8/8/8/8/4r3/4K3 b - -",
        ] {
            assert!(parse_fen(bad).is_err(), "accepted: {bad}");
        }
    }

    #[test]
    fn repetition_history_starts_at_fen() {
        let mut g = game_from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let start = g.board.hash(Color::White);
        assert_eq!(g.repetition_count(start), 1);
        for (f, t) in [("e1", "d1"), ("e8", "d8"), ("d1", "e1"), ("d8", "e8")] {
            assert!(g.make_move(f, t));
        }
        // Castling right lost: not the same position as the start.
        assert_eq!(g.repetition_count(g.board.hash(Color::White)), 1);
        for (f, t) in [("e1", "d1"), ("e8", "d8"), ("d1", "e1"), ("d8", "e8")] {
            assert!(g.make_move(f, t));
        }
        assert_eq!(g.repetition_count(g.board.hash(Color::White)), 2);
    }
}
