use crate::attacks::{BISHOP_PSEUDO, ROOK_PSEUDO, between_mask, bishop_attacks, rook_attacks};
use crate::board::{Board, color_idx, piece_index};
use crate::pieces::{Color, PieceType};
use crate::types::{Move, MoveList};

const DIRS_KNIGHT: [(i32, i32); 8] = [
    (-2, -1),
    (-2, 1),
    (-1, -2),
    (-1, 2),
    (1, -2),
    (1, 2),
    (2, -1),
    (2, 1),
];
const DIRS_KING: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

const fn build_step_table(dirs: &[(i32, i32)]) -> [u64; 64] {
    let mut arr = [0u64; 64];
    let mut sq = 0;
    while sq < 64 {
        let x = (sq % 8) as i32;
        let y = (sq / 8) as i32;
        let mut bb = 0u64;
        let mut i = 0;
        while i < dirs.len() {
            let nx = x + dirs[i].0;
            let ny = y + dirs[i].1;
            if nx >= 0 && nx < 8 && ny >= 0 && ny < 8 {
                bb |= 1u64 << (ny * 8 + nx);
            }
            i += 1;
        }
        arr[sq] = bb;
        sq += 1;
    }
    arr
}

pub static KNIGHT_TABLE: [u64; 64] = build_step_table(&DIRS_KNIGHT);
pub static KING_TABLE: [u64; 64] = build_step_table(&DIRS_KING);
/// Squares attacked by a white pawn standing on each square.
pub static WHITE_PAWN_ATTACKS: [u64; 64] = build_step_table(&[(-1, 1), (1, 1)]);
/// Squares attacked by a black pawn standing on each square.
pub static BLACK_PAWN_ATTACKS: [u64; 64] = build_step_table(&[(-1, -1), (1, -1)]);

#[inline(always)]
fn ep_mask(en_passant: Option<(usize, usize)>) -> u64 {
    match en_passant {
        Some((x, y)) => 1u64 << ((y * 8 + x) & 63),
        None => 0,
    }
}

#[inline(always)]
pub(crate) fn pawn_moves(
    sq: usize,
    color: Color,
    occ: u64,
    opp_occ: u64,
    en_passant: Option<(usize, usize)>,
) -> u64 {
    let sq = sq & 63;
    let y = sq / 8;
    let ep_bb = ep_mask(en_passant);
    match color {
        Color::White => {
            let mut moves = WHITE_PAWN_ATTACKS[sq] & (opp_occ | ep_bb);
            if y < 7 && (occ & (1u64 << (sq + 8))) == 0 {
                moves |= 1u64 << (sq + 8);
                if y == 1 && (occ & (1u64 << (sq + 16))) == 0 {
                    moves |= 1u64 << (sq + 16);
                }
            }
            moves
        }
        Color::Black => {
            let mut moves = BLACK_PAWN_ATTACKS[sq] & (opp_occ | ep_bb);
            if y > 0 && (occ & (1u64 << (sq - 8))) == 0 {
                moves |= 1u64 << (sq - 8);
                if y == 6 && (occ & (1u64 << (sq - 16))) == 0 {
                    moves |= 1u64 << (sq - 16);
                }
            }
            moves
        }
    }
}

/// Castling destination squares available to the king on `sq` (occupancy and
/// rights only; attacked squares are checked by the legality test).
#[inline(always)]
fn castling_targets(board: &Board, sq: usize, color: Color, occ_all: u64) -> u64 {
    let cidx = color_idx(color);
    let rank = if color == Color::White { 0 } else { 7 };
    let base = rank * 8;
    if sq != base + 4 {
        return 0;
    }
    let rooks = board.bitboards[cidx][piece_index(PieceType::Rook)];
    let mut targets = 0u64;
    if board.castling[cidx][0]
        && (rooks & (1u64 << (base + 7))) != 0
        && (occ_all & (0b0110_0000u64 << base)) == 0
    {
        targets |= 1u64 << (base + 6);
    }
    if board.castling[cidx][1]
        && (rooks & (1u64 << base)) != 0
        && (occ_all & (0b0000_1110u64 << base)) == 0
    {
        targets |= 1u64 << (base + 2);
    }
    targets
}

#[inline(always)]
pub(crate) fn pseudo_targets_for_piece(
    board: &Board,
    sq: usize,
    piece_type: PieceType,
    color: Color,
) -> u64 {
    let sq = sq & 63;
    let occ_self = board.all_pieces(color);
    let occ_opp = board.all_pieces(opposite_color(color));
    let occ_all = occ_self | occ_opp;

    let targets = match piece_type {
        PieceType::Pawn => pawn_moves(sq, color, occ_all, occ_opp, board.en_passant),
        PieceType::Knight => KNIGHT_TABLE[sq],
        PieceType::Bishop => bishop_attacks(sq, occ_all),
        PieceType::Rook => rook_attacks(sq, occ_all),
        PieceType::Queen => bishop_attacks(sq, occ_all) | rook_attacks(sq, occ_all),
        PieceType::King => KING_TABLE[sq] | castling_targets(board, sq, color, occ_all),
    };

    targets & !occ_self
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

/// Squares from which a pawn of `by_color` attacks `sq`.
#[inline(always)]
pub(crate) fn pawn_attackers_to_square(sq: usize, by_color: Color) -> u64 {
    match by_color {
        Color::White => BLACK_PAWN_ATTACKS[sq & 63],
        Color::Black => WHITE_PAWN_ATTACKS[sq & 63],
    }
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

/// A pawn move to the empty en-passant square that changes file.
#[inline(always)]
fn is_en_passant_target(ep_bb: u64, from_sq: usize, to_sq: usize, occ_opp: u64) -> bool {
    (ep_bb & !occ_opp & (1u64 << to_sq)) != 0 && ((from_sq ^ to_sq) & 7) != 0
}

/// Emits the moves of one pawn in ascending target order. With
/// `queen_promotions_only`, under-promotions are skipped.
#[inline(always)]
fn push_pawn_moves(
    list: &mut MoveList,
    ep_bb: u64,
    sq: usize,
    mut targets: u64,
    occ_opp: u64,
    queen_promotions_only: bool,
) {
    let from = sq as u8;
    while targets != 0 {
        let to_sq = targets.trailing_zeros() as usize;
        targets &= targets - 1;
        let to = to_sq as u8;
        let is_ep = is_en_passant_target(ep_bb, sq, to_sq, occ_opp);
        let is_capture = (occ_opp & (1u64 << to_sq)) != 0 || is_ep;
        let rank_to = to_sq / 8;
        if rank_to == 0 || rank_to == 7 {
            if queen_promotions_only {
                list.push(Move::promotion(from, to, PieceType::Queen, is_capture));
            } else {
                push_promotion_moves(list, from, to, is_capture);
            }
        } else if is_ep {
            list.push(Move::new(from, to, Move::FLAG_EP_CAPTURE));
        } else if (to_sq as isize - sq as isize).abs() == 16 {
            list.push(Move::new(from, to, Move::FLAG_DOUBLE_PUSH));
        } else if is_capture {
            list.push(Move::capture(from, to));
        } else {
            list.push(Move::normal(from, to));
        }
    }
}

/// Emits the moves of one non-pawn piece in ascending target order.
#[inline(always)]
fn push_piece_moves(list: &mut MoveList, sq: usize, mut targets: u64, occ_opp: u64) {
    let from = sq as u8;
    while targets != 0 {
        let to_sq = targets.trailing_zeros() as usize;
        targets &= targets - 1;
        let to = to_sq as u8;
        if (occ_opp & (1u64 << to_sq)) != 0 {
            list.push(Move::capture(from, to));
        } else {
            list.push(Move::normal(from, to));
        }
    }
}

const PROMOTION_RANKS: u64 = 0xFF00_0000_0000_00FF;

/// Generates pseudo-legal moves grouped by piece type (pawn..king), then by
/// ascending from-square, then by ascending to-square. Mode masks are applied
/// to target sets up front, so filtered moves are never produced.
fn generate_pseudo_legal_moves_with_mode(
    board: &Board,
    color: Color,
    list: &mut MoveList,
    mode: MoveGenMode,
) {
    list.clear();
    let cidx = color_idx(color);
    let own = &board.bitboards[cidx];
    let occ_self = board.all_pieces(color);
    let occ_opp = board.all_pieces(opposite_color(color));
    let occ_all = occ_self | occ_opp;
    let ep_bb = ep_mask(board.en_passant);
    let pawn_attacks: &[u64; 64] = if color == Color::White {
        &WHITE_PAWN_ATTACKS
    } else {
        &BLACK_PAWN_ATTACKS
    };

    // Target mask for non-king pieces; `None` means only king moves remain.
    let (piece_mask, king_mask, with_castling) = match mode {
        MoveGenMode::All => (Some(!0u64), !0u64, true),
        MoveGenMode::CapturesOnly => (Some(occ_opp), occ_opp, false),
        MoveGenMode::Evasions(info) => (
            if info.checker_count > 1 {
                None
            } else {
                Some(info.evasion_mask)
            },
            !0u64,
            false,
        ),
    };

    if let Some(piece_mask) = piece_mask {
        let mut bb = own[piece_index(PieceType::Pawn)];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            let mut targets = pawn_moves(sq, color, occ_all, occ_opp, board.en_passant) & !occ_self;
            match mode {
                MoveGenMode::All => {}
                MoveGenMode::CapturesOnly => {
                    targets &= occ_opp | PROMOTION_RANKS | (pawn_attacks[sq] & ep_bb);
                }
                MoveGenMode::Evasions(_) => {
                    targets &= piece_mask | (pawn_attacks[sq] & ep_bb & !occ_opp);
                }
            }
            let queen_only = matches!(mode, MoveGenMode::CapturesOnly);
            push_pawn_moves(list, ep_bb, sq, targets, occ_opp, queen_only);
        }

        let mut bb = own[piece_index(PieceType::Knight)];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            let targets = KNIGHT_TABLE[sq] & !occ_self & piece_mask;
            push_piece_moves(list, sq, targets, occ_opp);
        }

        let mut bb = own[piece_index(PieceType::Bishop)];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            let targets = bishop_attacks(sq, occ_all) & !occ_self & piece_mask;
            push_piece_moves(list, sq, targets, occ_opp);
        }

        let mut bb = own[piece_index(PieceType::Rook)];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            let targets = rook_attacks(sq, occ_all) & !occ_self & piece_mask;
            push_piece_moves(list, sq, targets, occ_opp);
        }

        let mut bb = own[piece_index(PieceType::Queen)];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            bb &= bb - 1;
            let targets =
                (bishop_attacks(sq, occ_all) | rook_attacks(sq, occ_all)) & !occ_self & piece_mask;
            push_piece_moves(list, sq, targets, occ_opp);
        }
    }

    let mut bb = own[piece_index(PieceType::King)];
    while bb != 0 {
        let sq = bb.trailing_zeros() as usize;
        bb &= bb - 1;
        let from = sq as u8;
        let mut targets = KING_TABLE[sq] & !occ_self & king_mask;
        if with_castling {
            targets |= castling_targets(board, sq, color, occ_all) & !occ_self;
        }
        while targets != 0 {
            let to_sq = targets.trailing_zeros() as usize;
            targets &= targets - 1;
            let to = to_sq as u8;
            if to_sq == sq + 2 {
                list.push(Move::new(from, to, Move::FLAG_KING_CASTLE));
            } else if to_sq + 2 == sq {
                list.push(Move::new(from, to, Move::FLAG_QUEEN_CASTLE));
            } else if (occ_opp & (1u64 << to_sq)) != 0 {
                list.push(Move::capture(from, to));
            } else {
                list.push(Move::normal(from, to));
            }
        }
    }
}

fn generate_legal_moves_with_mode(
    board: &mut Board,
    color: Color,
    list: &mut MoveList,
    mode: MoveGenMode,
    ctx: &LegalityContext,
) {
    generate_pseudo_legal_moves_with_mode(board, color, list, mode);

    // Filter in place, preserving order.
    let mut kept = 0;
    for i in 0..list.len() {
        let mv = list[i];
        if ctx.is_legal(board, mv, color) {
            list[kept] = mv;
            kept += 1;
        }
    }
    list.truncate(kept);
}

/// Per-position data that lets most pseudo-legal moves be validated without
/// making them: checkers, pinned pieces and the ray each pinned piece may use.
struct LegalityContext {
    king_sq: usize,
    king_bb: u64,
    checkers: u64,
    evasion_mask: u64,
    pinned: u64,
    pin_count: usize,
    pin_sq: [u8; 8],
    pin_ray: [u64; 8],
}

impl LegalityContext {
    #[inline(always)]
    fn new(board: &Board, color: Color) -> Self {
        let cidx = color_idx(color);
        let opp = opposite_color(color);
        let king_bb = board.bitboards[cidx][piece_index(PieceType::King)];
        let mut ctx = Self {
            king_sq: 64,
            king_bb,
            checkers: 0,
            evasion_mask: 0,
            pinned: 0,
            pin_count: 0,
            pin_sq: [0; 8],
            pin_ray: [0; 8],
        };
        if king_bb == 0 {
            return ctx;
        }
        let king_sq = king_bb.trailing_zeros() as usize;
        ctx.king_sq = king_sq;
        ctx.checkers = attackers_to_square(board, king_sq, opp);
        if ctx.checkers.count_ones() == 1 {
            let checker_sq = ctx.checkers.trailing_zeros() as usize;
            ctx.evasion_mask = ctx.checkers | between_mask(king_sq, checker_sq);
        }

        let ocidx = color_idx(opp);
        let opp_queens = board.bitboards[ocidx][piece_index(PieceType::Queen)];
        let opp_rq = board.bitboards[ocidx][piece_index(PieceType::Rook)] | opp_queens;
        let opp_bq = board.bitboards[ocidx][piece_index(PieceType::Bishop)] | opp_queens;
        let mut snipers = (ROOK_PSEUDO[king_sq] & opp_rq) | (BISHOP_PSEUDO[king_sq] & opp_bq);
        if snipers == 0 {
            return ctx;
        }
        let occ = board.occupied();
        let own = board.all_pieces(color);
        while snipers != 0 {
            let sniper_sq = snipers.trailing_zeros() as usize;
            snipers &= snipers - 1;
            let between = between_mask(king_sq, sniper_sq);
            let blockers = between & occ;
            if blockers != 0 && (blockers & (blockers - 1)) == 0 && (blockers & own) != 0 {
                ctx.pinned |= blockers;
                ctx.pin_sq[ctx.pin_count] = blockers.trailing_zeros() as u8;
                ctx.pin_ray[ctx.pin_count] = between | (1u64 << sniper_sq);
                ctx.pin_count += 1;
            }
        }
        ctx
    }

    #[inline(always)]
    fn pin_ray_of(&self, sq: u8) -> u64 {
        for i in 0..self.pin_count {
            if self.pin_sq[i] == sq {
                return self.pin_ray[i];
            }
        }
        !0
    }

    #[inline(always)]
    fn is_legal(&self, board: &mut Board, mv: Move, color: Color) -> bool {
        if self.king_bb == 0 || mv.is_castle() || mv.is_ep() {
            return board.is_generated_move_legal(mv, color);
        }
        let from = mv.from_sq();
        let to_bb = 1u64 << mv.to_sq();
        if from as usize == self.king_sq {
            let occ = board.occupied() & !self.king_bb;
            return !board.is_square_attacked_by_occ(mv.to_sq(), opposite_color(color), occ);
        }
        if self.checkers != 0 && (self.evasion_mask & to_bb) == 0 {
            // Double check (empty mask) or a move that neither captures nor blocks.
            return false;
        }
        if (self.pinned & (1u64 << from)) != 0 {
            return (self.pin_ray_of(from) & to_bb) != 0;
        }
        true
    }
}

pub fn generate_moves_fast(board: &mut Board, color: Color, list: &mut MoveList) {
    let ctx = LegalityContext::new(board, color);
    generate_legal_moves_with_mode(board, color, list, MoveGenMode::All, &ctx);
}

pub fn generate_captures_fast(board: &mut Board, color: Color, list: &mut MoveList) {
    let ctx = LegalityContext::new(board, color);
    generate_legal_moves_with_mode(board, color, list, MoveGenMode::CapturesOnly, &ctx);
}

pub fn generate_evasions_fast(board: &mut Board, color: Color, list: &mut MoveList) {
    let ctx = LegalityContext::new(board, color);
    let mode = if ctx.checkers != 0 {
        MoveGenMode::Evasions(EvasionInfo {
            checker_count: ctx.checkers.count_ones(),
            evasion_mask: ctx.evasion_mask,
        })
    } else {
        MoveGenMode::All
    };
    generate_legal_moves_with_mode(board, color, list, mode, &ctx);
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

        for mv in ["e7e8q", "e7d8q"] {
            assert!(
                moves.contains(&mv.to_string()),
                "captures-only should keep queen promotion {}",
                mv
            );
        }

        for mv in ["e7e8r", "e7e8b", "e7e8n", "e7d8r", "e7d8b", "e7d8n"] {
            assert!(
                !moves.contains(&mv.to_string()),
                "captures-only should skip under-promotion {}",
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

#[cfg(test)]
pub(crate) mod perft_tests {
    use super::*;
    use crate::pieces::Piece;

    pub(crate) fn board_from_fen(fen: &str) -> (Board, Color) {
        let mut board = Board::new();
        let parts: Vec<&str> = fen.split_whitespace().collect();
        let mut rank = 7usize;
        let mut file = 0usize;
        for ch in parts[0].chars() {
            match ch {
                '/' => {
                    rank -= 1;
                    file = 0;
                }
                '1'..='8' => file += ch.to_digit(10).unwrap() as usize,
                _ => {
                    let color = if ch.is_ascii_uppercase() {
                        Color::White
                    } else {
                        Color::Black
                    };
                    let piece_type = match ch.to_ascii_lowercase() {
                        'p' => PieceType::Pawn,
                        'n' => PieceType::Knight,
                        'b' => PieceType::Bishop,
                        'r' => PieceType::Rook,
                        'q' => PieceType::Queen,
                        _ => PieceType::King,
                    };
                    board.set_index(file, rank, Some(Piece { piece_type, color }));
                    file += 1;
                }
            }
        }
        let color = if parts[1] == "w" {
            Color::White
        } else {
            Color::Black
        };
        let c = parts[2];
        board.castling = [
            [c.contains('K'), c.contains('Q')],
            [c.contains('k'), c.contains('q')],
        ];
        board.en_passant = if parts[3] == "-" {
            None
        } else {
            Board::algebraic_to_index(parts[3])
        };
        (board, color)
    }

    #[inline]
    fn mix(sig: &mut u64, v: u64) {
        *sig = (*sig ^ v).wrapping_mul(0x100000001b3).rotate_left(17);
    }

    fn other(c: Color) -> Color {
        if c == Color::White {
            Color::Black
        } else {
            Color::White
        }
    }

    /// Perft that also folds every generated list (in order), SEE values and the
    /// incremental board state after each make into a signature.
    fn perft(board: &mut Board, color: Color, depth: u32, sig: &mut u64) -> u64 {
        let mut list = MoveList::new();
        generate_moves_fast(board, color, &mut list);
        for mv in list.iter() {
            mix(sig, mv.0 as u64);
        }

        let mut caps = MoveList::new();
        generate_captures_fast(board, color, &mut caps);
        mix(sig, 0xC0FFEE);
        for mv in caps.iter() {
            mix(sig, mv.0 as u64);
            mix(
                sig,
                crate::see::static_exchange_eval(board, *mv) as i64 as u64,
            );
        }

        let in_check = board.in_check_fast(color);
        mix(sig, in_check as u64);
        let mut ev = MoveList::new();
        generate_evasions_fast(board, color, &mut ev);
        mix(sig, 0xE5);
        for mv in ev.iter() {
            mix(sig, mv.0 as u64);
        }

        if depth == 1 {
            return list.len() as u64;
        }
        let mut nodes = 0;
        for mv in list.iter().copied() {
            let before = board.clone();
            let undo = board.make_move_fast(mv, color);
            mix(sig, board.hash);
            mix(sig, board.eval_mg as u64);
            mix(sig, board.eval_eg as u64);
            mix(sig, board.eval_phase as u64);
            nodes += perft(board, other(color), depth - 1, sig);
            board.unmake_move_fast(undo, color);
            assert_eq!(board.hash, before.hash);
            assert_eq!(board.eval_mg, before.eval_mg);
            assert_eq!(board.eval_eg, before.eval_eg);
            assert_eq!(board.eval_phase, before.eval_phase);
            assert_eq!(board.bitboards, before.bitboards);
            assert_eq!(board.white_occ, before.white_occ);
            assert_eq!(board.black_occ, before.black_occ);
            assert_eq!(board.en_passant, before.en_passant);
            assert_eq!(board.castling, before.castling);
        }
        nodes
    }

    pub(crate) const CASES: &[(&str, u32, u64)] = &[
        (
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -",
            4,
            197281,
        ),
        (
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -",
            4,
            4085603,
        ),
        ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - -", 5, 674624),
        (
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq -",
            4,
            422333,
        ),
        (
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ -",
            3,
            62379,
        ),
        (
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - -",
            3,
            89890,
        ),
    ];

    #[test]
    fn perft_counts_and_generation_signature() {
        let mut sig = 0xcbf29ce484222325u64;
        for &(fen, depth, expected) in CASES {
            let (mut board, color) = board_from_fen(fen);
            let nodes = perft(&mut board, color, depth, &mut sig);
            assert_eq!(nodes, expected, "perft mismatch for {fen}");
        }
        println!("generation signature {sig:#x}");
        assert_eq!(sig, EXPECTED_SIGNATURE);
    }

    /// Checks `Board::gives_check` against make + `in_check_fast` for every
    /// legal move of every node down to `depth`.
    fn check_gives_check(board: &mut Board, color: Color, depth: u32, checks: &mut u64) -> u64 {
        let mut list = MoveList::new();
        generate_moves_fast(board, color, &mut list);
        let mut nodes = 0;
        for mv in list.iter().copied() {
            let predicted = board.gives_check(mv, color);
            let undo = board.make_move_fast(mv, color);
            let actual = board.in_check_fast(other(color));
            assert_eq!(
                predicted,
                actual,
                "gives_check mismatch for {} in {}",
                mv.to_algebraic(),
                board.to_fen(other(color))
            );
            *checks += actual as u64;
            nodes += 1;
            if depth > 1 {
                nodes += check_gives_check(board, other(color), depth - 1, checks);
            }
            board.unmake_move_fast(undo, color);
        }
        nodes
    }

    #[test]
    fn gives_check_matches_make_and_in_check() {
        let extra: &[&str] = &[
            // En passant that discovers a rook check along the rank.
            "8/8/8/1k1pP2R/8/8/8/4K3 w - d6",
            // Castling gives check with the rook (both sides).
            "5k2/8/8/8/8/8/8/4K2R w K -",
            "3k4/8/8/8/8/8/8/R3K3 w Q -",
            // Promotions (with and without capture) giving direct/discovered checks.
            "1r5k/P1P5/8/8/8/8/6K1/B7 w - -",
            "k7/6P1/8/8/8/8/8/K6R w - -",
            // Discovered checks by bishops, rooks and queens behind many pieces.
            "4k3/8/2N1P3/3B4/4R3/8/8/Q3K3 w - -",
            "r3k2r/8/8/3pP3/8/8/8/R3K2R w KQkq d6",
        ];
        let mut total = 0;
        let mut checks = 0;
        for fen in CASES.iter().map(|c| c.0).chain(extra.iter().copied()) {
            let (mut board, color) = board_from_fen(fen);
            total += check_gives_check(&mut board, color, 3, &mut checks);
        }
        println!("gives_check: {total} moves, {checks} checks");
        assert!(total > 100_000, "only {total} moves checked");
        assert!(checks > 1_000, "only {checks} checking moves seen");
    }

    // Captures-only lists emit queen promotions only (under-promotions are
    // skipped), which is folded into this signature.
    const EXPECTED_SIGNATURE: u64 = 0x1ace0a0692f04604;
}
