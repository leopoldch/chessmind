use crate::board::{Board, color_idx, piece_index};
use crate::pieces::{Color, PieceType};
use once_cell::sync::Lazy;

const DIRS_KNIGHT: &[(isize, isize)] = &[
    (-2, -1),
    (-2, 1),
    (-1, -2),
    (-1, 2),
    (1, -2),
    (1, 2),
    (2, -1),
    (2, 1),
];
const DIRS_KING: &[(isize, isize)] = &[
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

pub static KNIGHT_TABLE: Lazy<[u64; 64]> = Lazy::new(|| {
    let mut arr = [0u64; 64];
    for y in 0..8 {
        for x in 0..8 {
            let mut bb = 0u64;
            for (dx, dy) in DIRS_KNIGHT {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && nx < 8 && ny >= 0 && ny < 8 {
                    bb |= 1u64 << (ny * 8 + nx);
                }
            }
            arr[y * 8 + x] = bb;
        }
    }
    arr
});

pub static KING_TABLE: Lazy<[u64; 64]> = Lazy::new(|| {
    let mut arr = [0u64; 64];
    for y in 0..8 {
        for x in 0..8 {
            let mut bb = 0u64;
            for (dx, dy) in DIRS_KING {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && nx < 8 && ny >= 0 && ny < 8 {
                    bb |= 1u64 << (ny * 8 + nx);
                }
            }
            arr[y * 8 + x] = bb;
        }
    }
    arr
});

pub static WHITE_PAWN_ATTACKS: Lazy<[u64; 64]> = Lazy::new(|| {
    let mut arr = [0u64; 64];
    for y in 0..8 {
        for x in 0..8 {
            let mut bb = 0u64;
            if x > 0 && y < 7 {
                bb |= 1u64 << ((y + 1) * 8 + (x - 1));
            }
            if x < 7 && y < 7 {
                bb |= 1u64 << ((y + 1) * 8 + (x + 1));
            }
            arr[y * 8 + x] = bb;
        }
    }
    arr
});

pub static BLACK_PAWN_ATTACKS: Lazy<[u64; 64]> = Lazy::new(|| {
    let mut arr = [0u64; 64];
    for y in 0..8 {
        for x in 0..8 {
            let mut bb = 0u64;
            if x > 0 && y > 0 {
                bb |= 1u64 << ((y - 1) * 8 + (x - 1));
            }
            if x < 7 && y > 0 {
                bb |= 1u64 << ((y - 1) * 8 + (x + 1));
            }
            arr[y * 8 + x] = bb;
        }
    }
    arr
});

fn rook_attacks(sq: usize, occ: u64) -> u64 {
    let x = (sq % 8) as isize;
    let y = (sq / 8) as isize;
    let mut attacks = 0u64;
    let mut ny = y + 1;
    while ny < 8 {
        let idx = (ny * 8 + x) as usize;
        attacks |= 1u64 << idx;
        if (occ & (1u64 << idx)) != 0 {
            break;
        }
        ny += 1;
    }
    ny = y - 1;
    while ny >= 0 {
        let idx = (ny * 8 + x) as usize;
        attacks |= 1u64 << idx;
        if (occ & (1u64 << idx)) != 0 {
            break;
        }
        if ny == 0 {
            break;
        }
        ny -= 1;
    }
    let mut nx = x + 1;
    while nx < 8 {
        let idx = (y * 8 + nx) as usize;
        attacks |= 1u64 << idx;
        if (occ & (1u64 << idx)) != 0 {
            break;
        }
        nx += 1;
    }
    nx = x - 1;
    while nx >= 0 {
        let idx = (y * 8 + nx) as usize;
        attacks |= 1u64 << idx;
        if (occ & (1u64 << idx)) != 0 {
            break;
        }
        if nx == 0 {
            break;
        }
        nx -= 1;
    }
    attacks
}

fn bishop_attacks(sq: usize, occ: u64) -> u64 {
    let x = (sq % 8) as isize;
    let y = (sq / 8) as isize;
    let mut attacks = 0u64;
    let mut nx = x + 1;
    let mut ny = y + 1;
    while nx < 8 && ny < 8 {
        let idx = (ny * 8 + nx) as usize;
        attacks |= 1u64 << idx;
        if (occ & (1u64 << idx)) != 0 {
            break;
        }
        nx += 1;
        ny += 1;
    }
    nx = x - 1;
    ny = y + 1;
    while nx >= 0 && ny < 8 {
        let idx = (ny * 8 + nx) as usize;
        attacks |= 1u64 << idx;
        if (occ & (1u64 << idx)) != 0 {
            break;
        }
        if nx == 0 {
            break;
        }
        nx -= 1;
        ny += 1;
    }
    nx = x + 1;
    ny = y - 1;
    while nx < 8 && ny >= 0 {
        let idx = (ny * 8 + nx) as usize;
        attacks |= 1u64 << idx;
        if (occ & (1u64 << idx)) != 0 {
            break;
        }
        if ny == 0 {
            break;
        }
        nx += 1;
        ny -= 1;
    }
    nx = x - 1;
    ny = y - 1;
    while nx >= 0 && ny >= 0 {
        let idx = (ny * 8 + nx) as usize;
        attacks |= 1u64 << idx;
        if (occ & (1u64 << idx)) != 0 {
            break;
        }
        if nx == 0 || ny == 0 {
            attacks |= 0;
        };
        nx -= 1;
        ny -= 1;
        if nx < 0 || ny < 0 {
            break;
        }
    }
    attacks
}

fn pawn_moves(
    sq: usize,
    color: Color,
    occ: u64,
    opp_occ: u64,
    en_passant: Option<(usize, usize)>,
) -> u64 {
    let x = sq % 8;
    let y = sq / 8;
    let mut moves = 0u64;
    match color {
        Color::White => {
            if y < 7 && (occ & (1u64 << ((y + 1) * 8 + x))) == 0 {
                moves |= 1u64 << ((y + 1) * 8 + x);
                if y == 1 && (occ & (1u64 << ((y + 2) * 8 + x))) == 0 {
                    moves |= 1u64 << ((y + 2) * 8 + x);
                }
            }
            if x > 0 && y < 7 && (opp_occ & (1u64 << ((y + 1) * 8 + x - 1))) != 0 {
                moves |= 1u64 << ((y + 1) * 8 + x - 1);
            }
            if x < 7 && y < 7 && (opp_occ & (1u64 << ((y + 1) * 8 + x + 1))) != 0 {
                moves |= 1u64 << ((y + 1) * 8 + x + 1);
            }
            if let Some((ex, ey)) = en_passant {
                if ey == y + 1 && ((ex == x + 1) || (ex + 1 == x)) {
                    moves |= 1u64 << (ey * 8 + ex);
                }
            }
        }
        Color::Black => {
            if y > 0 && (occ & (1u64 << ((y - 1) * 8 + x))) == 0 {
                moves |= 1u64 << ((y - 1) * 8 + x);
                if y == 6 && (occ & (1u64 << ((y - 2) * 8 + x))) == 0 {
                    moves |= 1u64 << ((y - 2) * 8 + x);
                }
            }
            if x > 0 && y > 0 && (opp_occ & (1u64 << ((y - 1) * 8 + x - 1))) != 0 {
                moves |= 1u64 << ((y - 1) * 8 + x - 1);
            }
            if x < 7 && y > 0 && (opp_occ & (1u64 << ((y - 1) * 8 + x + 1))) != 0 {
                moves |= 1u64 << ((y - 1) * 8 + x + 1);
            }
            if let Some((ex, ey)) = en_passant {
                if ey + 1 == y && ((ex == x + 1) || (ex + 1 == x)) {
                    moves |= 1u64 << (ey * 8 + ex);
                }
            }
        }
    }
    moves
}

pub fn generate_moves(board: &mut Board, color: Color) -> Vec<(String, String)> {
    let mut list = crate::types::MoveList::new();
    generate_moves_fast(board, color, &mut list);

    let mut res = Vec::new();
    for i in 0..list.len() {
        let m = list.get(i).unwrap();
        let f_str =
            Board::index_to_algebraic((m.from_sq() % 8) as usize, (m.from_sq() / 8) as usize)
                .unwrap();
        let mut t_str =
            Board::index_to_algebraic((m.to_sq() % 8) as usize, (m.to_sq() / 8) as usize).unwrap();

        if m.is_promotion() {
            if let Some(pt) = m.promotion_piece() {
                let c = match pt {
                    PieceType::Queen => 'q',
                    PieceType::Rook => 'r',
                    PieceType::Bishop => 'b',
                    PieceType::Knight => 'n',
                    _ => 'q',
                };
                t_str.push(c);
            }
        }
        res.push((f_str, t_str));
    }
    res
}

pub fn generate_moves_fast(board: &mut Board, color: Color, list: &mut crate::types::MoveList) {
    let cidx = color_idx(color);
    let opp_color = if color == Color::White {
        Color::Black
    } else {
        Color::White
    };
    let occ_self: u64 = board.bitboards[cidx].iter().fold(0u64, |a, &b| a | b);
    let occ_opp: u64 = board.bitboards[1 - cidx].iter().fold(0u64, |a, &b| a | b);
    let occ_all = occ_self | occ_opp;

    for pt in [
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
        PieceType::King,
    ] {
        let mut bb = board.bitboards[cidx][piece_index(pt)];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            let from = sq as u8;
            let mut targets;

            match pt {
                PieceType::Pawn => {
                    targets = pawn_moves(sq, color, occ_all, occ_opp, board.en_passant)
                }
                PieceType::Knight => targets = KNIGHT_TABLE[sq],
                PieceType::Bishop => targets = bishop_attacks(sq, occ_all),
                PieceType::Rook => targets = rook_attacks(sq, occ_all),
                PieceType::Queen => {
                    targets = bishop_attacks(sq, occ_all) | rook_attacks(sq, occ_all)
                }
                PieceType::King => {
                    targets = KING_TABLE[sq];
                    let rank = if color == Color::White { 0 } else { 7 };
                    if sq == rank * 8 + 4 {
                        if board.castling[cidx][0]
                            && board.get_index(5, rank).is_none()
                            && board.get_index(6, rank).is_none()
                            && !board.is_square_attacked_by(sq as u8, opp_color) // King not in check
                            && !board.is_square_attacked_by((rank*8+5) as u8, opp_color)
                        {
                            targets |= 1u64 << (rank * 8 + 6);
                        }
                        if board.castling[cidx][1]
                            && board.get_index(1, rank).is_none()
                            && board.get_index(2, rank).is_none()
                            && board.get_index(3, rank).is_none()
                            && !board.is_square_attacked_by(sq as u8, opp_color)
                            && !board.is_square_attacked_by((rank * 8 + 3) as u8, opp_color)
                        {
                            targets |= 1u64 << (rank * 8 + 2);
                        }
                    }
                }
            }
            targets &= !occ_self;

            while targets != 0 {
                let to_sq = targets.trailing_zeros() as usize;
                let to = to_sq as u8;

                let is_capture = (occ_opp & (1u64 << to_sq)) != 0
                    || (pt == PieceType::Pawn && (to_sq as isize - sq as isize).abs() % 8 != 0); // Diag pawn move

                let mut flags = crate::types::Move::FLAG_NORMAL;
                if is_capture {
                    flags = crate::types::Move::FLAG_CAPTURE;
                }

                if pt == PieceType::Pawn {
                    let dy = (to_sq as isize - sq as isize).abs();
                    if dy == 16 {
                        flags = crate::types::Move::FLAG_DOUBLE_PUSH;
                    }
                    if dy % 8 != 0 && (occ_opp & (1u64 << to_sq)) == 0 {
                        flags = crate::types::Move::FLAG_EP_CAPTURE;
                    }

                    let rank_to = to_sq / 8;
                    if rank_to == 0 || rank_to == 7 {
                        let next_is_capture = (occ_opp & (1u64 << to_sq)) != 0;

                        let f_s = Board::index_to_algebraic(sq % 8, sq / 8).unwrap();
                        let t_s = Board::index_to_algebraic(to_sq % 8, to_sq / 8).unwrap();

                        if board.is_legal(&f_s, &t_s, color) {
                            list.push(crate::types::Move::promotion(
                                from,
                                to,
                                PieceType::Queen,
                                next_is_capture,
                            ));
                            list.push(crate::types::Move::promotion(
                                from,
                                to,
                                PieceType::Rook,
                                next_is_capture,
                            ));
                            list.push(crate::types::Move::promotion(
                                from,
                                to,
                                PieceType::Bishop,
                                next_is_capture,
                            ));
                            list.push(crate::types::Move::promotion(
                                from,
                                to,
                                PieceType::Knight,
                                next_is_capture,
                            ));
                        }

                        targets &= targets - 1;
                        continue; // Skip normal push (already handled all 4 promos)
                    }
                }

                if pt == PieceType::King && (to_sq as isize - sq as isize).abs() == 2 {
                    if to_sq > sq {
                        flags = crate::types::Move::FLAG_KING_CASTLE;
                    } else {
                        flags = crate::types::Move::FLAG_QUEEN_CASTLE;
                    }
                }

                let mv = crate::types::Move::new(from, to, flags);

                let f_s = Board::index_to_algebraic(sq % 8, sq / 8).unwrap();
                let t_s = Board::index_to_algebraic(to_sq % 8, to_sq / 8).unwrap();

                if board.is_legal(&f_s, &t_s, color) {
                    list.push(mv);
                }

                targets &= targets - 1;
            }
            bb &= bb - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MoveList;

    fn setup_board() -> Board {
        let mut board = Board::new();
        board.setup_standard();
        board
    }

    #[test]
    fn test_starting_position_white_moves() {
        let mut board = setup_board();
        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut list);

        assert_eq!(
            list.len(),
            20,
            "White should have 20 legal moves in starting position"
        );
    }

    #[test]
    fn test_starting_position_black_moves() {
        let mut board = setup_board();
        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::Black, &mut list);

        assert_eq!(
            list.len(),
            20,
            "Black should have 20 legal moves in starting position"
        );
    }

    #[test]
    fn test_kiwipete_position() {
        let mut board = Board::new();

        board.set(
            "a1",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Rook,
                color: Color::White,
            }),
        );
        board.set(
            "e1",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "h1",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Rook,
                color: Color::White,
            }),
        );

        for file in ['a', 'b', 'c', 'f', 'g', 'h'] {
            board.set(
                &format!("{}2", file),
                Some(crate::pieces::Piece {
                    piece_type: PieceType::Pawn,
                    color: Color::White,
                }),
            );
        }
        board.set(
            "d2",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Bishop,
                color: Color::White,
            }),
        );
        board.set(
            "e2",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Bishop,
                color: Color::White,
            }),
        );

        board.set(
            "c3",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Knight,
                color: Color::White,
            }),
        );
        board.set(
            "f3",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Queen,
                color: Color::White,
            }),
        );
        board.set(
            "h3",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        board.set(
            "b4",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.set(
            "e4",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );

        board.set(
            "d5",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "e5",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Knight,
                color: Color::White,
            }),
        );

        board.set(
            "a6",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Bishop,
                color: Color::Black,
            }),
        );
        board.set(
            "b6",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Knight,
                color: Color::Black,
            }),
        );
        board.set(
            "e6",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.set(
            "f6",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Knight,
                color: Color::Black,
            }),
        );
        board.set(
            "g6",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        board.set(
            "a7",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.set(
            "c7",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.set(
            "d7",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.set(
            "e7",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Queen,
                color: Color::Black,
            }),
        );
        board.set(
            "f7",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.set(
            "g7",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Bishop,
                color: Color::Black,
            }),
        );

        board.set(
            "a8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "e8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "h8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );

        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut list);

        assert_eq!(
            list.len(),
            48,
            "Kiwipete should have 48 legal moves for white, got {}",
            list.len()
        );
    }

    #[test]
    fn test_no_moves_leave_king_in_check() {
        let mut board = setup_board();
        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut list);

        for i in 0..list.len() {
            let mv = list.get(i).unwrap();
            let from_x = (mv.from_sq() % 8) as usize;
            let from_y = (mv.from_sq() / 8) as usize;
            let to_x = (mv.to_sq() % 8) as usize;
            let to_y = (mv.to_sq() / 8) as usize;

            let from_str = Board::index_to_algebraic(from_x, from_y).unwrap();
            let to_str = Board::index_to_algebraic(to_x, to_y).unwrap();

            let state = board.make_move_state(&from_str, &to_str);
            assert!(
                state.is_some(),
                "Move generation returned invalid move: {} -> {}",
                from_str,
                to_str
            );

            let in_check = board.in_check(Color::White);
            assert!(
                !in_check,
                "Legal move {} -> {} leaves king in check!",
                from_str, to_str
            );

            board.unmake_move(state.unwrap());
        }
    }

    #[test]
    fn test_promotion_moves_generated() {
        let mut board = Board::new();

        board.set(
            "e7",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "e1",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "h8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );

        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut list);

        let promo_count = (0..list.len())
            .filter(|&i| list.get(i).unwrap().is_promotion())
            .count();

        assert_eq!(
            promo_count, 4,
            "Should generate 4 promotion moves for e7-e8, got {}",
            promo_count
        );
    }

    #[test]
    fn test_promotion_capture_moves() {
        let mut board = Board::new();

        board.set(
            "e7",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "d8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "f8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "e1",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "h8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );

        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut list);

        let promo_count = (0..list.len())
            .filter(|&i| list.get(i).unwrap().is_promotion())
            .count();

        assert_eq!(
            promo_count, 12,
            "Should generate 12 promotion moves (3 squares × 4 pieces), got {}",
            promo_count
        );
    }

    #[test]
    fn test_castling_moves_available() {
        let mut board = setup_board();

        board.set("f1", None);
        board.set("g1", None);
        board.set("b1", None);
        board.set("c1", None);
        board.set("d1", None);

        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut list);

        let castle_count = (0..list.len())
            .filter(|&i| {
                let mv = list.get(i).unwrap();
                mv.from_sq() == 4 && (mv.to_sq() == 6 || mv.to_sq() == 2) // e1=4, g1=6, c1=2
            })
            .count();

        assert_eq!(
            castle_count, 2,
            "Should have 2 castling moves (kingside and queenside)"
        );
    }

    #[test]
    fn test_en_passant_moves() {
        let mut board = Board::new();

        board.set(
            "e5",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "d5",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.set(
            "e1",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "e8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.en_passant = Some((3, 5)); // d6

        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut list);

        let ep_count = (0..list.len())
            .filter(|&i| list.get(i).unwrap().is_ep())
            .count();

        assert!(ep_count >= 1, "Should have at least 1 en passant move");
    }

    #[test]
    fn test_knight_table() {
        let attacks = KNIGHT_TABLE[28];

        let expected_squares = [11, 13, 18, 22, 34, 38, 43, 45]; // d2, f2, c3, g3, c5, g5, d6, f6
        for sq in expected_squares {
            assert!(
                attacks & (1u64 << sq) != 0,
                "Knight on e4 should attack square {}",
                sq
            );
        }
    }

    #[test]
    fn test_king_table() {
        let attacks = KING_TABLE[28];

        let expected_squares = [19, 20, 21, 27, 29, 35, 36, 37]; // d3, e3, f3, d4, f4, d5, e5, f5
        for sq in expected_squares {
            assert!(
                attacks & (1u64 << sq) != 0,
                "King on e4 should attack square {}",
                sq
            );
        }

        assert_eq!(attacks.count_ones(), 8);
    }
}
