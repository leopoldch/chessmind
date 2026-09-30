use crate::attacks::{bishop_attacks, rook_attacks};
use crate::board::{Board, color_idx, piece_index};
use crate::movegen::{BLACK_PAWN_ATTACKS, KING_TABLE, KNIGHT_TABLE, WHITE_PAWN_ATTACKS};
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

/// Static exchange evaluation of `mv` on its destination square.
///
/// The attacker set is computed once; after each exchange only the used
/// attacker is removed and sliders are re-scanned for x-ray attackers.
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

    let bbs = &board.bitboards;
    let sq = (to_sq & 63) as usize;
    let mut occ = board.occupied() & !sq_bb(from_sq);

    let captured_value = if mv.is_ep() {
        let cap_sq = if color == Color::White {
            to_sq - 8
        } else {
            to_sq + 8
        };
        occ &= !sq_bb(cap_sq);
        PieceValues::PAWN
    } else if let Some(piece_type) = captured_piece {
        PieceValues::value(piece_type)
    } else {
        0
    };

    let mut occupant_piece_idx = match mv.promotion_piece() {
        Some(promo) => piece_index(promo),
        None => piece_index(moving_piece),
    };
    let promotion_bonus = match mv.promotion_piece() {
        Some(promo) => PieceValues::value(promo) - PieceValues::value(PieceType::Pawn),
        None => 0,
    };
    occ |= sq_bb(to_sq);

    let (w, b) = (&bbs[0], &bbs[1]);
    let queens = w[4] | b[4];
    let diagonal_sliders = w[2] | b[2] | queens;
    let straight_sliders = w[3] | b[3] | queens;

    // Pieces removed from `occ` (mover, e.p. victim, used attackers) are
    // masked out by `& occ`; the destination square never attacks itself.
    let mut attackers = ((BLACK_PAWN_ATTACKS[sq] & w[0])
        | (WHITE_PAWN_ATTACKS[sq] & b[0])
        | (KNIGHT_TABLE[sq] & (w[1] | b[1]))
        | (KING_TABLE[sq] & (w[5] | b[5]))
        | (bishop_attacks(sq, occ) & diagonal_sliders)
        | (rook_attacks(sq, occ) & straight_sliders))
        & occ;

    let mut gain = [0i32; 32];
    let mut depth = 0usize;
    gain[0] = captured_value + promotion_bonus;

    let mut side = opp;

    loop {
        let side_attackers = attackers & board.all_pieces(side);
        if side_attackers == 0 {
            break;
        }
        let side_bbs = &bbs[color_idx(side)];
        let mut attacker_piece_idx = 0;
        let mut candidates = side_bbs[0] & side_attackers;
        while candidates == 0 && attacker_piece_idx < 5 {
            attacker_piece_idx += 1;
            candidates = side_bbs[attacker_piece_idx] & side_attackers;
        }
        if candidates == 0 {
            break;
        }
        let attacker_bb = candidates & candidates.wrapping_neg();

        depth += 1;
        gain[depth] = PieceValues::value_by_idx(occupant_piece_idx) - gain[depth - 1];

        if gain[depth].max(-gain[depth - 1]) < 0 {
            break;
        }

        occ &= !attacker_bb;
        attackers &= !attacker_bb;
        // Removing a piece can only uncover sliders on the line it stood on.
        match attacker_piece_idx {
            0 | 2 => attackers |= bishop_attacks(sq, occ) & diagonal_sliders & occ,
            3 => attackers |= rook_attacks(sq, occ) & straight_sliders & occ,
            4 | 5 => {
                attackers |= ((bishop_attacks(sq, occ) & diagonal_sliders)
                    | (rook_attacks(sq, occ) & straight_sliders))
                    & occ
            }
            _ => {}
        }

        occupant_piece_idx = if attacker_piece_idx == piece_index(PieceType::Pawn)
            && is_promotion_square(side, to_sq)
        {
            piece_index(PieceType::Queen)
        } else {
            attacker_piece_idx
        };

        side = opposite(side);
    }

    while depth > 0 {
        depth -= 1;
        gain[depth] = -(-gain[depth]).max(gain[depth + 1]);
    }

    gain[0]
}

/// The original from-scratch implementation, kept to validate the incremental one.
#[cfg(test)]
mod reference {
    use super::*;

    fn sq_bb(sq: u8) -> u64 {
        1u64 << sq
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

    pub(super) fn static_exchange_eval_reference(board: &Board, mv: Move) -> i32 {
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

    fn compare_tree(board: &mut Board, color: Color, depth: u32, checked: &mut u64) {
        let mut list = crate::types::MoveList::new();
        crate::movegen::generate_moves_fast(board, color, &mut list);
        for mv in list.iter().copied() {
            assert_eq!(
                static_exchange_eval(board, mv),
                reference::static_exchange_eval_reference(board, mv),
                "SEE mismatch for {} in {}",
                mv.to_algebraic(),
                board.to_fen(color)
            );
            *checked += 1;
        }
        if depth == 0 {
            return;
        }
        for mv in list.iter().copied() {
            let undo = board.make_move_fast(mv, color);
            compare_tree(board, opposite(color), depth - 1, checked);
            board.unmake_move_fast(undo, color);
        }
    }

    #[test]
    fn incremental_see_matches_reference() {
        let mut checked = 0u64;
        for &(fen, depth, _) in crate::movegen::perft_tests::CASES {
            let (mut board, color) = crate::movegen::perft_tests::board_from_fen(fen);
            compare_tree(&mut board, color, depth.min(3), &mut checked);
        }
        assert!(checked > 100_000);
    }
}
