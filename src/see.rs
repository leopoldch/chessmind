use crate::attacks::{bishop_attacks, rook_attacks};
use crate::board::{Board, color_idx, piece_index};
use crate::pieces::{Color, PieceType};
use crate::types::{Move, PieceValues};

#[inline(always)]
fn opposite(color: Color) -> Color {
    match color {
        Color::White => Color::Black,
        Color::Black => Color::White,
    }
}

#[inline(always)]
fn sq_bb(sq: u8) -> u64 {
    1u64 << sq
}

#[inline(always)]
fn is_promotion_square(color: Color, sq: u8) -> bool {
    match color {
        Color::White => sq / 8 == 7,
        Color::Black => sq / 8 == 0,
    }
}

fn pawn_attackers_to_square(sq: u8, by_color: Color) -> u64 {
    let file = sq % 8;
    let rank = sq / 8;
    let mut attackers = 0u64;

    match by_color {
        Color::White => {
            if rank > 0 {
                if file > 0 {
                    attackers |= sq_bb(sq - 9);
                }
                if file < 7 {
                    attackers |= sq_bb(sq - 7);
                }
            }
        }
        Color::Black => {
            if rank < 7 {
                if file > 0 {
                    attackers |= sq_bb(sq + 7);
                }
                if file < 7 {
                    attackers |= sq_bb(sq + 9);
                }
            }
        }
    }

    attackers
}

fn attackers_to_square(pieces: &[[u64; 6]; 2], occ: u64, sq: u8, by_color: Color) -> u64 {
    let cidx = color_idx(by_color);
    let pawns = pieces[cidx][piece_index(PieceType::Pawn)];
    let knights = pieces[cidx][piece_index(PieceType::Knight)];
    let bishops = pieces[cidx][piece_index(PieceType::Bishop)];
    let rooks = pieces[cidx][piece_index(PieceType::Rook)];
    let queens = pieces[cidx][piece_index(PieceType::Queen)];
    let kings = pieces[cidx][piece_index(PieceType::King)];

    (pawn_attackers_to_square(sq, by_color) & pawns)
        | (crate::movegen::KNIGHT_TABLE[sq as usize] & knights)
        | (crate::movegen::KING_TABLE[sq as usize] & kings)
        | (bishop_attacks(sq as usize, occ) & (bishops | queens))
        | (rook_attacks(sq as usize, occ) & (rooks | queens))
}

fn least_valuable_attacker(
    pieces: &[[u64; 6]; 2],
    attackers: u64,
    by_color: Color,
) -> Option<(u8, usize)> {
    let cidx = color_idx(by_color);
    for pt in 0..6 {
        let candidates = pieces[cidx][pt] & attackers;
        if candidates != 0 {
            return Some((candidates.trailing_zeros() as u8, pt));
        }
    }
    None
}

pub fn static_exchange_eval(board: &Board, mv: Move) -> i32 {
    let from_sq = mv.from_sq();
    let to_sq = mv.to_sq();
    let Some((moving_piece, color)) = board.piece_at_sq(from_sq) else {
        return 0;
    };
    let opp = opposite(color);

    let captured_piece = if mv.is_ep() {
        Some(PieceType::Pawn)
    } else {
        board.piece_at_sq(to_sq).map(|(piece_type, _)| piece_type)
    };

    if captured_piece.is_none() && !mv.is_promotion() {
        return 0;
    }

    let mut pieces = board.bitboards;
    let mut occ = board.occupied();
    let from_bb = sq_bb(from_sq);
    let to_bb = sq_bb(to_sq);

    let moving_piece_idx = piece_index(moving_piece);
    pieces[color_idx(color)][moving_piece_idx] &= !from_bb;
    occ &= !from_bb;

    let captured_value = if mv.is_ep() {
        let cap_sq = if color == Color::White {
            to_sq - 8
        } else {
            to_sq + 8
        };
        let cap_bb = sq_bb(cap_sq);
        pieces[color_idx(opp)][piece_index(PieceType::Pawn)] &= !cap_bb;
        occ &= !cap_bb;
        PieceValues::PAWN
    } else if let Some(piece_type) = captured_piece {
        pieces[color_idx(opp)][piece_index(piece_type)] &= !to_bb;
        PieceValues::value(piece_type)
    } else {
        0
    };

    let mut occupant_piece_idx = if let Some(promo) = mv.promotion_piece() {
        piece_index(promo)
    } else {
        moving_piece_idx
    };
    let promotion_bonus = if let Some(promo) = mv.promotion_piece() {
        PieceValues::value(promo) - PieceValues::value(PieceType::Pawn)
    } else {
        0
    };

    pieces[color_idx(color)][occupant_piece_idx] |= to_bb;
    occ |= to_bb;

    let mut gain = [0i32; 32];
    let mut depth = 0usize;
    gain[0] = captured_value + promotion_bonus;

    let mut side = opp;
    let mut occupant_color = color;

    loop {
        let attackers = attackers_to_square(&pieces, occ, to_sq, side);
        let Some((attacker_sq, attacker_piece_idx)) =
            least_valuable_attacker(&pieces, attackers, side)
        else {
            break;
        };

        depth += 1;
        gain[depth] = PieceValues::value_by_idx(occupant_piece_idx) - gain[depth - 1];

        if gain[depth].max(-gain[depth - 1]) < 0 {
            break;
        }

        pieces[color_idx(occupant_color)][occupant_piece_idx] &= !to_bb;

        let attacker_bb = sq_bb(attacker_sq);
        pieces[color_idx(side)][attacker_piece_idx] &= !attacker_bb;
        occ &= !attacker_bb;

        occupant_piece_idx = if attacker_piece_idx == piece_index(PieceType::Pawn)
            && is_promotion_square(side, to_sq)
        {
            piece_index(PieceType::Queen)
        } else {
            attacker_piece_idx
        };

        pieces[color_idx(side)][occupant_piece_idx] |= to_bb;
        occ |= to_bb;
        occupant_color = side;
        side = opposite(side);
    }

    while depth > 0 {
        depth -= 1;
        gain[depth] = -(-gain[depth]).max(gain[depth + 1]);
    }

    gain[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::pieces::{Color, Piece, PieceType};

    fn put(board: &mut Board, sq: &str, piece_type: PieceType, color: Color) {
        board.set(sq, Some(Piece { piece_type, color }));
    }

    #[test]
    fn see_detects_losing_capture() {
        let mut board = Board::new();
        put(&mut board, "e1", PieceType::King, Color::White);
        put(&mut board, "e8", PieceType::King, Color::Black);
        put(&mut board, "d4", PieceType::Knight, Color::White);
        put(&mut board, "e6", PieceType::Pawn, Color::Black);
        put(&mut board, "f7", PieceType::Pawn, Color::Black);

        let mv = Move::capture(27, 44);
        assert!(static_exchange_eval(&board, mv) < 0);
    }

    #[test]
    fn see_handles_en_passant() {
        let mut board = Board::new();
        put(&mut board, "e1", PieceType::King, Color::White);
        put(&mut board, "e8", PieceType::King, Color::Black);
        put(&mut board, "e5", PieceType::Pawn, Color::White);
        put(&mut board, "d5", PieceType::Pawn, Color::Black);
        board.en_passant = Some((3, 5));

        let mv = Move::new(36, 43, Move::FLAG_EP_CAPTURE);
        assert_eq!(static_exchange_eval(&board, mv), PieceValues::PAWN);
    }

    #[test]
    fn see_counts_promotion_gain() {
        let mut board = Board::new();
        put(&mut board, "e1", PieceType::King, Color::White);
        put(&mut board, "h8", PieceType::King, Color::Black);
        put(&mut board, "e7", PieceType::Pawn, Color::White);
        put(&mut board, "f8", PieceType::Rook, Color::Black);

        let mv = Move::promotion(52, 61, PieceType::Queen, true);
        assert!(static_exchange_eval(&board, mv) >= PieceValues::ROOK);
    }
}
