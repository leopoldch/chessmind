use crate::attacks::{between_mask, bishop_attacks, rook_attacks};
use crate::board::{Board, color_idx, piece_index};
use crate::pieces::{Color, PieceType};
use crate::types::{Move, MoveList};
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

#[inline(always)]
pub(crate) fn pawn_moves(
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

#[inline(always)]
fn has_castle_rook(board: &Board, color: Color, rook_x: usize, rank: usize) -> bool {
    matches!(
        board.get_index(rook_x, rank),
        Some(piece) if piece.color == color && piece.piece_type == PieceType::Rook
    )
}

#[inline(always)]
pub(crate) fn pseudo_targets_for_piece(
    board: &Board,
    sq: usize,
    piece_type: PieceType,
    color: Color,
) -> u64 {
    let cidx = color_idx(color);
    let occ_self = board.all_pieces(color);
    let occ_opp = board.all_pieces(if color == Color::White {
        Color::Black
    } else {
        Color::White
    });
    let occ_all = occ_self | occ_opp;

    let mut targets = match piece_type {
        PieceType::Pawn => pawn_moves(sq, color, occ_all, occ_opp, board.en_passant),
        PieceType::Knight => KNIGHT_TABLE[sq],
        PieceType::Bishop => bishop_attacks(sq, occ_all),
        PieceType::Rook => rook_attacks(sq, occ_all),
        PieceType::Queen => bishop_attacks(sq, occ_all) | rook_attacks(sq, occ_all),
        PieceType::King => {
            let mut king_targets = KING_TABLE[sq];
            let rank = if color == Color::White { 0 } else { 7 };
            if sq == rank * 8 + 4 {
                if board.castling[cidx][0]
                    && has_castle_rook(board, color, 7, rank)
                    && board.get_index(5, rank).is_none()
                    && board.get_index(6, rank).is_none()
                {
                    king_targets |= 1u64 << (rank * 8 + 6);
                }
                if board.castling[cidx][1]
                    && has_castle_rook(board, color, 0, rank)
                    && board.get_index(1, rank).is_none()
                    && board.get_index(2, rank).is_none()
                    && board.get_index(3, rank).is_none()
                {
                    king_targets |= 1u64 << (rank * 8 + 2);
                }
            }
            king_targets
        }
    };

    targets &= !occ_self;
    targets
}

#[inline(always)]
pub(crate) fn is_move_pseudo_legal(board: &Board, mv: Move, color: Color) -> bool {
    let from_sq = mv.from_sq() as usize;
    let to_sq = mv.to_sq() as usize;
    let from_x = from_sq % 8;
    let from_y = from_sq / 8;
    let to_x = to_sq % 8;
    let to_y = to_sq / 8;

    let piece = match board.get_index(from_x, from_y) {
        Some(piece) if piece.color == color => piece,
        _ => return false,
    };

    if matches!(board.get_index(to_x, to_y), Some(piece) if piece.color == color) {
        return false;
    }

    let targets = pseudo_targets_for_piece(board, from_sq, piece.piece_type, color);
    if (targets & (1u64 << to_sq)) == 0 {
        return false;
    }

    let is_ep_target = piece.piece_type == PieceType::Pawn
        && board.en_passant == Some((to_x, to_y))
        && from_x != to_x
        && board.get_index(to_x, to_y).is_none();
    let is_capture = board.get_index(to_x, to_y).is_some() || is_ep_target;

    match piece.piece_type {
        PieceType::Pawn => {
            let promotion_rank = if color == Color::White { 7 } else { 0 };
            if (to_y == promotion_rank) != mv.is_promotion() {
                return false;
            }
            if mv.is_ep() != is_ep_target {
                return false;
            }
            if mv.is_double_push() != (from_x == to_x && from_y.abs_diff(to_y) == 2) {
                return false;
            }
            if mv.is_capture() != is_capture {
                return false;
            }
        }
        PieceType::King => {
            let is_castle = from_y == to_y && from_x.abs_diff(to_x) == 2;
            if mv.is_castle() != is_castle {
                return false;
            }
            if mv.is_double_push() || mv.is_ep() || mv.is_promotion() {
                return false;
            }
            if !mv.is_castle() && mv.is_capture() != is_capture {
                return false;
            }
        }
        _ => {
            if mv.is_double_push() || mv.is_ep() || mv.is_promotion() || mv.is_castle() {
                return false;
            }
            if mv.is_capture() != is_capture {
                return false;
            }
        }
    }

    true
}

pub fn generate_moves(board: &mut Board, color: Color) -> Vec<(String, String)> {
    let mut list = MoveList::new();
    generate_moves_fast(board, color, &mut list);

    let mut res = Vec::with_capacity(list.len());
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

#[inline(always)]
fn push_promotion_moves(list: &mut MoveList, from: u8, to: u8, is_capture: bool) {
    list.push(Move::promotion(from, to, PieceType::Queen, is_capture));
    list.push(Move::promotion(from, to, PieceType::Rook, is_capture));
    list.push(Move::promotion(from, to, PieceType::Bishop, is_capture));
    list.push(Move::promotion(from, to, PieceType::Knight, is_capture));
}

#[derive(Copy, Clone)]
struct EvasionInfo {
    checker_count: u32,
    evasion_mask: u64,
}

#[derive(Copy, Clone)]
enum MoveGenMode {
    All,
    CapturesOnly,
    Evasions(EvasionInfo),
}

#[inline(always)]
fn opposite_color(color: Color) -> Color {
    if color == Color::White {
        Color::Black
    } else {
        Color::White
    }
}

#[inline(always)]
fn pawn_attackers_to_square(sq: usize, by_color: Color) -> u64 {
    let x = sq % 8;
    let y = sq / 8;
    let mut attackers = 0u64;

    match by_color {
        Color::White => {
            if y > 0 {
                if x > 0 {
                    attackers |= 1u64 << ((y - 1) * 8 + (x - 1));
                }
                if x < 7 {
                    attackers |= 1u64 << ((y - 1) * 8 + (x + 1));
                }
            }
        }
        Color::Black => {
            if y < 7 {
                if x > 0 {
                    attackers |= 1u64 << ((y + 1) * 8 + (x - 1));
                }
                if x < 7 {
                    attackers |= 1u64 << ((y + 1) * 8 + (x + 1));
                }
            }
        }
    }

    attackers
}

#[inline(always)]
fn attackers_to_square(board: &Board, sq: usize, by_color: Color) -> u64 {
    let cidx = color_idx(by_color);
    let occ = board.occupied();
    let bishops_queens = board.bitboards[cidx][piece_index(PieceType::Bishop)]
        | board.bitboards[cidx][piece_index(PieceType::Queen)];
    let rooks_queens = board.bitboards[cidx][piece_index(PieceType::Rook)]
        | board.bitboards[cidx][piece_index(PieceType::Queen)];

    (pawn_attackers_to_square(sq, by_color) & board.bitboards[cidx][piece_index(PieceType::Pawn)])
        | (KNIGHT_TABLE[sq] & board.bitboards[cidx][piece_index(PieceType::Knight)])
        | (KING_TABLE[sq] & board.bitboards[cidx][piece_index(PieceType::King)])
        | (bishop_attacks(sq, occ) & bishops_queens)
        | (rook_attacks(sq, occ) & rooks_queens)
}

#[inline(always)]
fn evasion_info(board: &Board, color: Color) -> Option<EvasionInfo> {
    let cidx = color_idx(color);
    let king_bb = board.bitboards[cidx][piece_index(PieceType::King)];
    if king_bb == 0 {
        return None;
    }

    let king_sq = king_bb.trailing_zeros() as usize;
    let checkers = attackers_to_square(board, king_sq, opposite_color(color));
    let checker_count = checkers.count_ones();
    if checker_count == 0 {
        return None;
    }

    let evasion_mask = if checker_count == 1 {
        let checker_sq = checkers.trailing_zeros() as usize;
        (1u64 << checker_sq) | between_mask(king_sq, checker_sq)
    } else {
        0
    };

    Some(EvasionInfo {
        checker_count,
        evasion_mask,
    })
}

#[inline(always)]
fn is_en_passant_target(board: &Board, from_sq: usize, to_sq: usize, occ_opp: u64) -> bool {
    board.en_passant == Some((to_sq % 8, to_sq / 8))
        && (occ_opp & (1u64 << to_sq)) == 0
        && (to_sq as isize - from_sq as isize).abs() % 8 != 0
}

fn generate_pseudo_legal_moves_with_mode(
    board: &Board,
    color: Color,
    list: &mut MoveList,
    mode: MoveGenMode,
) {
    list.clear();
    let cidx = color_idx(color);
    let occ_opp: u64 = board.bitboards[color_idx(opposite_color(color))]
        .iter()
        .fold(0u64, |a, &b| a | b);

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
            let mut targets = pseudo_targets_for_piece(board, sq, pt, color);

            while targets != 0 {
                let to_sq = targets.trailing_zeros() as usize;
                let to = to_sq as u8;
                let to_bb = 1u64 << to_sq;
                let is_castle = pt == PieceType::King && (to_sq as isize - sq as isize).abs() == 2;

                if pt == PieceType::Pawn {
                    let is_ep = is_en_passant_target(board, sq, to_sq, occ_opp);
                    let is_capture = (occ_opp & to_bb) != 0 || is_ep;
                    let rank_to = to_sq / 8;
                    let is_promotion = rank_to == 0 || rank_to == 7;
                    let emit = match mode {
                        MoveGenMode::All => true,
                        MoveGenMode::CapturesOnly => is_capture || is_promotion,
                        MoveGenMode::Evasions(info) => {
                            if info.checker_count > 1 {
                                false
                            } else {
                                is_ep || (info.evasion_mask & to_bb) != 0
                            }
                        }
                    };

                    if !emit {
                        targets &= targets - 1;
                        continue;
                    }

                    if is_promotion {
                        push_promotion_moves(list, from, to, is_capture);
                        targets &= targets - 1;
                        continue;
                    }
                    if is_ep {
                        list.push(Move::new(from, to, Move::FLAG_EP_CAPTURE));
                    } else if (to_sq as isize - sq as isize).abs() == 16 {
                        list.push(Move::new(from, to, Move::FLAG_DOUBLE_PUSH));
                    } else if is_capture {
                        list.push(Move::capture(from, to));
                    } else {
                        list.push(Move::normal(from, to));
                    }
                    targets &= targets - 1;
                    continue;
                }

                let emit = match mode {
                    MoveGenMode::All => true,
                    MoveGenMode::CapturesOnly => (occ_opp & to_bb) != 0,
                    MoveGenMode::Evasions(info) => {
                        if pt == PieceType::King {
                            !is_castle
                        } else if info.checker_count > 1 {
                            false
                        } else {
                            (info.evasion_mask & to_bb) != 0
                        }
                    }
                };

                if !emit {
                    targets &= targets - 1;
                    continue;
                }

                if is_castle {
                    let flag = if to_sq > sq {
                        Move::FLAG_KING_CASTLE
                    } else {
                        Move::FLAG_QUEEN_CASTLE
                    };
                    list.push(Move::new(from, to, flag));
                } else if (occ_opp & to_bb) != 0 {
                    list.push(Move::capture(from, to));
                } else {
                    list.push(Move::normal(from, to));
                }

                targets &= targets - 1;
            }
            bb &= bb - 1;
        }
    }
}

fn generate_legal_moves_with_mode(
    board: &mut Board,
    color: Color,
    list: &mut MoveList,
    mode: MoveGenMode,
) {
    let mut pseudo = MoveList::new();
    generate_pseudo_legal_moves_with_mode(board, color, &mut pseudo, mode);

    list.clear();
    for mv in pseudo.iter().copied() {
        if board.is_generated_move_legal(mv, color) {
            list.push(mv);
        }
    }
}

pub fn generate_moves_fast(board: &mut Board, color: Color, list: &mut MoveList) {
    generate_legal_moves_with_mode(board, color, list, MoveGenMode::All);
}

pub fn generate_captures_fast(board: &mut Board, color: Color, list: &mut MoveList) {
    generate_legal_moves_with_mode(board, color, list, MoveGenMode::CapturesOnly);
}

pub fn generate_evasions_fast(board: &mut Board, color: Color, list: &mut MoveList) {
    if let Some(info) = evasion_info(board, color) {
        generate_legal_moves_with_mode(board, color, list, MoveGenMode::Evasions(info));
    } else {
        generate_moves_fast(board, color, list);
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

    fn move_strings(list: &MoveList) -> Vec<String> {
        list.iter().map(|mv| mv.to_algebraic()).collect()
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
    fn test_castling_through_check_not_generated() {
        let mut board = Board::new();

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
        board.set(
            "a8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
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

        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut list);

        assert!(
            !(0..list.len())
                .any(|i| list.get(i).unwrap() == Move::new(4, 6, Move::FLAG_KING_CASTLE)),
            "Kingside castling must not be generated when f1 is attacked"
        );
    }

    #[test]
    fn test_en_passant_discovered_check_not_generated() {
        let mut board = Board::new();

        board.set(
            "e1",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "e5",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "a8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "e8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "d5",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.en_passant = Some((3, 5)); // d6

        let mut list = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut list);

        assert!(
            !(0..list.len())
                .any(|i| list.get(i).unwrap() == Move::new(36, 43, Move::FLAG_EP_CAPTURE)),
            "En passant must be filtered out when it opens the e-file onto the king"
        );
    }

    #[test]
    fn test_generate_captures_only_keeps_promotions_and_ep() {
        let mut board = Board::new();
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
        board.en_passant = Some((3, 5)); // d6

        let mut list = MoveList::new();
        generate_captures_fast(&mut board, Color::White, &mut list);
        let moves = move_strings(&list);

        for mv in ["e7e8q", "e7e8r", "e7e8b", "e7e8n"] {
            assert!(
                moves.contains(&mv.to_string()),
                "captures-only should keep quiet promotion {}",
                mv
            );
        }

        for mv in ["e7d8q", "e7d8r", "e7d8b", "e7d8n"] {
            assert!(
                moves.contains(&mv.to_string()),
                "captures-only should keep capture promotion {}",
                mv
            );
        }

        assert!(
            moves.contains(&"e5d6".to_string()),
            "captures-only should keep en passant captures"
        );
        assert!(
            !moves.contains(&"e1d1".to_string()),
            "captures-only must not include unrelated quiet king moves"
        );
    }

    #[test]
    fn test_generate_evasions_matches_full_legal_moves() {
        let mut board = Board::new();
        board.set(
            "e1",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "d2",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Bishop,
                color: Color::White,
            }),
        );
        board.set(
            "g2",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Bishop,
                color: Color::White,
            }),
        );
        board.set(
            "e8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "h8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );

        assert!(board.in_check_fast(Color::White));

        let mut all_moves = MoveList::new();
        generate_moves_fast(&mut board, Color::White, &mut all_moves);
        let expected = move_strings(&all_moves);

        let mut evasions = MoveList::new();
        generate_evasions_fast(&mut board, Color::White, &mut evasions);
        let actual = move_strings(&evasions);

        assert_eq!(
            actual.len(),
            expected.len(),
            "evasions-only should produce the full legal move set when in check"
        );
        for mv in expected {
            assert!(
                actual.contains(&mv),
                "evasions-only is missing legal evasion {}",
                mv
            );
        }
    }

    #[test]
    fn test_generate_evasions_double_check_only_moves_king() {
        let mut board = Board::new();
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
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "b4",
            Some(crate::pieces::Piece {
                piece_type: PieceType::Bishop,
                color: Color::Black,
            }),
        );
        board.set(
            "h8",
            Some(crate::pieces::Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );

        assert!(board.in_check_fast(Color::White));

        let mut evasions = MoveList::new();
        generate_evasions_fast(&mut board, Color::White, &mut evasions);

        assert!(
            !evasions.is_empty(),
            "double-check position should still have king evasions"
        );
        assert!(
            evasions.iter().all(|mv| mv.from_sq() == 4),
            "double-check evasions must be king moves only"
        );
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
