use crate::attacks::{between_mask, bishop_attacks, rook_attacks};
use crate::board::{Board, color_idx, piece_index};
use crate::eval_cache::{EVAL_CACHE, PAWN_CACHE, pawn_hash};
use crate::movegen::{KING_TABLE, KNIGHT_TABLE};
use crate::pieces::{Color, Piece, PieceType};
use crate::types::{Phase, Square};

#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
pub struct Score(i32);

impl Score {
    pub const ZERO: Score = Score(0);

    #[inline(always)]
    pub const fn new(mg: i16, eg: i16) -> Self {
        Score(((mg as i32) << 16) + (eg as i32))
    }

    #[inline(always)]
    pub const fn make(v: i16) -> Self {
        Self::new(v, v)
    }

    #[inline(always)]
    pub const fn mg(self) -> i32 {
        (self.0 + 0x8000) >> 16
    }

    #[inline(always)]
    pub const fn eg(self) -> i32 {
        (self.0 as i16) as i32
    }

    #[inline(always)]
    pub fn taper(self, phase: i32) -> i32 {
        let mg = self.mg();
        let eg = self.eg();
        ((mg * phase) + (eg * (Phase::TOTAL_PHASE - phase))) / Phase::TOTAL_PHASE
    }
}

impl std::ops::Add for Score {
    type Output = Score;
    #[inline(always)]
    fn add(self, rhs: Score) -> Score {
        Score(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Score {
    type Output = Score;
    #[inline(always)]
    fn sub(self, rhs: Score) -> Score {
        Score(self.0 - rhs.0)
    }
}

impl std::ops::Neg for Score {
    type Output = Score;
    #[inline(always)]
    fn neg(self) -> Score {
        Score(-self.0)
    }
}

impl std::ops::AddAssign for Score {
    #[inline(always)]
    fn add_assign(&mut self, rhs: Score) {
        self.0 += rhs.0;
    }
}

impl std::ops::SubAssign for Score {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: Score) {
        self.0 -= rhs.0;
    }
}

impl std::ops::Mul<i32> for Score {
    type Output = Score;
    #[inline(always)]
    fn mul(self, rhs: i32) -> Score {
        Score(self.0 * rhs)
    }
}

const PAWN_PST_MG: [i16; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, // Rank 1 (never occupied)
    -10, 5, -5, -10, -10, -5, 5, -10, // Rank 2 (starting)
    -10, 0, 5, 10, 10, 5, 0, -10, // Rank 3
    -5, 0, 10, 25, 25, 10, 0, -5, // Rank 4
    5, 5, 15, 30, 30, 15, 5, 5, // Rank 5
    15, 15, 25, 35, 35, 25, 15, 15, // Rank 6
    50, 50, 50, 50, 50, 50, 50, 50, // Rank 7 (about to promote)
    0, 0, 0, 0, 0, 0, 0, 0, // Rank 8 (never occupied)
];

const PAWN_PST_EG: [i16; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 5, 5, 5, 5, 5, 5, 5, 5, 10, 10, 10, 10, 10, 10, 10, 10, 15, 15, 15, 20,
    20, 15, 15, 15, 25, 25, 25, 30, 30, 25, 25, 25, 40, 40, 40, 45, 45, 40, 40, 40, 70, 70, 70, 70,
    70, 70, 70, 70, // Very high value near promotion
    0, 0, 0, 0, 0, 0, 0, 0,
];

const KNIGHT_PST_MG: [i16; 64] = [
    -50, -40, -30, -30, -30, -30, -40, -50, -40, -20, 0, 5, 5, 0, -20, -40, -30, 5, 15, 20, 20, 15,
    5, -30, -30, 5, 20, 25, 25, 20, 5, -30, -30, 5, 20, 25, 25, 20, 5, -30, -30, 5, 15, 20, 20, 15,
    5, -30, -40, -20, 0, 5, 5, 0, -20, -40, -50, -40, -30, -30, -30, -30, -40, -50,
];

const KNIGHT_PST_EG: [i16; 64] = [
    -50, -40, -30, -30, -30, -30, -40, -50, -40, -20, 0, 0, 0, 0, -20, -40, -30, 0, 15, 15, 15, 15,
    0, -30, -30, 5, 15, 20, 20, 15, 5, -30, -30, 5, 15, 20, 20, 15, 5, -30, -30, 0, 15, 15, 15, 15,
    0, -30, -40, -20, 0, 0, 0, 0, -20, -40, -50, -40, -30, -30, -30, -30, -40, -50,
];

const BISHOP_PST_MG: [i16; 64] = [
    -20, -10, -10, -10, -10, -10, -10, -20, -10, 5, 0, 0, 0, 0, 5, -10, -10, 10, 10, 10, 10, 10,
    10, -10, -10, 0, 15, 15, 15, 15, 0, -10, -10, 5, 10, 15, 15, 10, 5, -10, -10, 0, 10, 15, 15,
    10, 0, -10, -10, 5, 0, 0, 0, 0, 5, -10, -20, -10, -10, -10, -10, -10, -10, -20,
];

const BISHOP_PST_EG: [i16; 64] = [
    -20, -10, -10, -10, -10, -10, -10, -20, -10, 0, 0, 0, 0, 0, 0, -10, -10, 0, 5, 10, 10, 5, 0,
    -10, -10, 5, 10, 15, 15, 10, 5, -10, -10, 5, 10, 15, 15, 10, 5, -10, -10, 0, 5, 10, 10, 5, 0,
    -10, -10, 0, 0, 0, 0, 0, 0, -10, -20, -10, -10, -10, -10, -10, -10, -20,
];

const ROOK_PST_MG: [i16; 64] = [
    0, 0, 5, 10, 10, 5, 0, 0, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0,
    0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, 10, 15, 15, 15, 15, 15, 15,
    10, // 7th rank bonus
    0, 0, 0, 5, 5, 0, 0, 0,
];

const ROOK_PST_EG: [i16; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 15, 15, 15, 15, 15, 15,
    15, // 7th rank strong in endgame
    0, 0, 0, 0, 0, 0, 0, 0,
];

const QUEEN_PST_MG: [i16; 64] = [
    -20, -10, -10, -5, -5, -10, -10, -20, -10, 0, 5, 0, 0, 0, 0, -10, -10, 5, 5, 5, 5, 5, 0, -10,
    0, 0, 5, 5, 5, 5, 0, -5, -5, 0, 5, 5, 5, 5, 0, -5, -10, 0, 5, 5, 5, 5, 0, -10, -10, 0, 0, 0, 0,
    0, 0, -10, -20, -10, -10, -5, -5, -10, -10, -20,
];

const QUEEN_PST_EG: [i16; 64] = [
    -20, -10, -10, -5, -5, -10, -10, -20, -10, 0, 0, 0, 0, 0, 0, -10, -10, 0, 5, 5, 5, 5, 0, -10,
    -5, 0, 5, 10, 10, 5, 0, -5, -5, 0, 5, 10, 10, 5, 0, -5, -10, 0, 5, 5, 5, 5, 0, -10, -10, 0, 0,
    0, 0, 0, 0, -10, -20, -10, -10, -5, -5, -10, -10, -20,
];

const KING_PST_MG: [i16; 64] = [
    20, 30, 10, 0, 0, 10, 30, 20, // Castle and stay safe
    20, 20, 0, 0, 0, 0, 20, 20, -10, -20, -20, -20, -20, -20, -20, -10, -20, -30, -30, -40, -40,
    -30, -30, -20, -30, -40, -40, -50, -50, -40, -40, -30, -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30, -30, -40, -40, -50, -50, -40, -40, -30,
];

const KING_PST_EG: [i16; 64] = [
    -50, -30, -30, -30, -30, -30, -30, -50, // In endgame, center is good
    -30, -20, -10, -10, -10, -10, -20, -30, -30, -10, 20, 30, 30, 20, -10, -30, -30, -10, 30, 40,
    40, 30, -10, -30, -30, -10, 30, 40, 40, 30, -10, -30, -30, -10, 20, 30, 30, 20, -10, -30, -30,
    -20, -10, -10, -10, -10, -20, -30, -50, -30, -30, -30, -30, -30, -30, -50,
];

const PST_MG: [[i16; 64]; 6] = [
    PAWN_PST_MG,
    KNIGHT_PST_MG,
    BISHOP_PST_MG,
    ROOK_PST_MG,
    QUEEN_PST_MG,
    KING_PST_MG,
];

const PST_EG: [[i16; 64]; 6] = [
    PAWN_PST_EG,
    KNIGHT_PST_EG,
    BISHOP_PST_EG,
    ROOK_PST_EG,
    QUEEN_PST_EG,
    KING_PST_EG,
];

const MATERIAL_MG: [i16; 6] = [100, 320, 330, 500, 900, 0];
const MATERIAL_EG: [i16; 6] = [120, 300, 320, 550, 1000, 0];

const PASSED_PAWN_BONUS_MG: [i16; 8] = [0, 5, 10, 20, 40, 70, 120, 0];
const PASSED_PAWN_BONUS_EG: [i16; 8] = [0, 10, 20, 40, 70, 120, 200, 0];

const CONNECTED_PASSED_BONUS: Score = Score::new(10, 20);

const DOUBLED_PAWN_PENALTY: Score = Score::new(10, 15);

const ISOLATED_PAWN_PENALTY: Score = Score::new(15, 10);

const BACKWARD_PAWN_PENALTY: Score = Score::new(10, 8);

const MOBILITY_BONUS: [Score; 6] = [
    Score::ZERO,
    Score::new(3, 4),
    Score::new(4, 4),
    Score::new(2, 3),
    Score::new(1, 2),
    Score::ZERO,
];

const KING_ZONE_ATTACK_BONUS: [Score; 6] = [
    Score::ZERO,
    Score::new(5, 1),
    Score::new(5, 1),
    Score::new(3, 1),
    Score::new(2, 0),
    Score::ZERO,
];

const PAWN_SHELTER_PENALTY: Score = Score::new(20, 5);

const KING_OPEN_FILE_PENALTY: Score = Score::new(30, 0);

const KING_SEMI_OPEN_FILE_PENALTY: Score = Score::new(15, 0);

const BISHOP_PAIR_BONUS: Score = Score::new(30, 50);

const ROOK_OPEN_FILE_BONUS: Score = Score::new(20, 10);

const ROOK_SEMI_OPEN_FILE_BONUS: Score = Score::new(10, 5);

const ROOK_ON_7TH_BONUS: Score = Score::new(20, 30);

const KNIGHT_OUTPOST_BONUS: Score = Score::new(25, 15);

const PROTECTED_PASSED_PAWN_BONUS_MG: [i16; 8] = [0, 0, 6, 10, 18, 28, 40, 0];
const PROTECTED_PASSED_PAWN_BONUS_EG: [i16; 8] = [0, 0, 10, 16, 26, 40, 60, 0];

const BLOCKED_PASSED_PAWN_PENALTY_MG: [i16; 8] = [0, 0, 4, 8, 12, 18, 28, 0];
const BLOCKED_PASSED_PAWN_PENALTY_EG: [i16; 8] = [0, 0, 6, 12, 18, 28, 40, 0];

const ROOK_BEHIND_PASSED_PAWN_BONUS: Score = Score::new(14, 24);

const ENEMY_ROOK_BEHIND_PASSED_PAWN_PENALTY: Score = Score::new(10, 18);

const OPPOSITE_BISHOPS_SCALE_NUM: i32 = 3;
const OPPOSITE_BISHOPS_SCALE_DEN: i32 = 4;

const TEMPO_BONUS: i32 = 15;

const FILE_A: u64 = 0x0101010101010101;
const FILE_H: u64 = 0x8080808080808080;

const fn file_mask(file: usize) -> u64 {
    FILE_A << file
}

/// Squares on the files directly left and right of `file`.
const ADJACENT_FILES: [u64; 8] = {
    let mut table = [0u64; 8];
    let mut file = 0;
    while file < 8 {
        if file > 0 {
            table[file] |= file_mask(file - 1);
        }
        if file < 7 {
            table[file] |= file_mask(file + 1);
        }
        file += 1;
    }
    table
};

/// Squares of ranks `lo..hi` (exclusive upper bound).
const fn ranks_mask(lo: usize, hi: usize) -> u64 {
    let mut mask = 0u64;
    let mut r = lo;
    while r < hi {
        mask |= 0xFFu64 << (r * 8);
        r += 1;
    }
    mask
}

/// Squares at (rank, file - 1) and (rank, file + 1), if on the board.
const fn side_squares(rank: usize, file: usize) -> u64 {
    let mut mask = 0u64;
    if file > 0 {
        mask |= 1u64 << (rank * 8 + file - 1);
    }
    if file < 7 {
        mask |= 1u64 << (rank * 8 + file + 1);
    }
    mask
}

/// Precomputed pawn-structure masks indexed by `[color][square]`
/// (color 0 = white, 1 = black) or by `[square]`.
struct PawnMasks {
    /// Own pawns on adjacent files within one rank (connected-passer check).
    neighbor: [u64; 64],
    /// Own pawns on adjacent files at or behind the pawn's rank support it.
    backward_support: [[u64; 64]; 2],
    /// Enemy pawns here attack the pawn's advance square.
    backward_attack: [[u64; 64]; 2],
    /// Own pawns here defend a knight on the square.
    outpost_support: [[u64; 64]; 2],
    /// Enemy pawns here could eventually attack a knight on the square.
    outpost_attack: [[u64; 64]; 2],
}

static PAWN_MASKS: PawnMasks = {
    let mut m = PawnMasks {
        neighbor: [0; 64],
        backward_support: [[0; 64]; 2],
        backward_attack: [[0; 64]; 2],
        outpost_support: [[0; 64]; 2],
        outpost_attack: [[0; 64]; 2],
    };
    let mut sq = 0;
    while sq < 64 {
        let file = sq % 8;
        let rank = sq / 8;
        let adj = ADJACENT_FILES[file];
        let white_front = ranks_mask(rank + 1, 8);
        let black_front = ranks_mask(0, rank);

        let lo = if rank > 0 { rank - 1 } else { 0 };
        let hi = if rank < 7 { rank + 2 } else { 8 };
        m.neighbor[sq] = adj & ranks_mask(lo, hi);

        m.backward_support[0][sq] = adj & ranks_mask(0, rank + 1);
        m.backward_support[1][sq] = adj & ranks_mask(rank, 8);

        if rank < 6 {
            m.backward_attack[0][sq] = side_squares(rank + 2, file);
        }
        if rank > 1 {
            m.backward_attack[1][sq] = side_squares(rank - 2, file);
        }

        if rank > 0 {
            m.outpost_support[0][sq] = side_squares(rank - 1, file);
        }
        if rank < 7 {
            m.outpost_support[1][sq] = side_squares(rank + 1, file);
        }

        m.outpost_attack[0][sq] = adj & white_front;
        m.outpost_attack[1][sq] = adj & black_front;

        sq += 1;
    }
    m
};

#[inline(always)]
pub(crate) fn piece_eval_delta(piece: Piece, sq: usize) -> (i32, i32, i32) {
    let idx = piece_index(piece.piece_type);
    let eval_sq = if piece.color == Color::White {
        sq
    } else {
        Square::flip(sq as u8) as usize
    };
    let sign = if piece.color == Color::White { 1 } else { -1 };
    let phase = match piece.piece_type {
        PieceType::Knight => Phase::KNIGHT_PHASE,
        PieceType::Bishop => Phase::BISHOP_PHASE,
        PieceType::Rook => Phase::ROOK_PHASE,
        PieceType::Queen => Phase::QUEEN_PHASE,
        _ => 0,
    };

    (
        sign * (MATERIAL_MG[idx] as i32 + PST_MG[idx][eval_sq] as i32),
        sign * (MATERIAL_EG[idx] as i32 + PST_EG[idx][eval_sq] as i32),
        phase,
    )
}

#[allow(dead_code)]
pub struct Evaluator<'a> {
    board: &'a Board,
    white_pieces: u64,
    black_pieces: u64,
    occupied: u64,
    phase: i32,
}

impl<'a> Evaluator<'a> {
    pub fn new(board: &'a Board) -> Self {
        let white_pieces = board.white_occ;
        let black_pieces = board.black_occ;

        let phase = board.eval_phase.clamp(0, Phase::TOTAL_PHASE);

        Self {
            board,
            white_pieces,
            black_pieces,
            occupied: white_pieces | black_pieces,
            phase,
        }
    }

    fn calculate_phase(board: &Board) -> i32 {
        let mut phase = 0;
        for color in 0..2 {
            phase += board.bitboards[color][1].count_ones() as i32 * Phase::KNIGHT_PHASE;
            phase += board.bitboards[color][2].count_ones() as i32 * Phase::BISHOP_PHASE;
            phase += board.bitboards[color][3].count_ones() as i32 * Phase::ROOK_PHASE;
            phase += board.bitboards[color][4].count_ones() as i32 * Phase::QUEEN_PHASE;
        }
        phase.min(Phase::TOTAL_PHASE)
    }

    pub fn evaluate(&self) -> i32 {
        if is_drawn_endgame(self.board) {
            return 0;
        }

        self.evaluate_non_drawn()
    }

    /// Full evaluation (using the pawn cache), assuming the caller has already
    /// ruled out `is_drawn_endgame`.
    #[inline(always)]
    fn evaluate_non_drawn(&self) -> i32 {
        let mut score = Score::ZERO;

        score += self.eval_material_and_pst();

        score += self.eval_pawn_structure();

        score += self.eval_pieces();

        score += self.eval_king_safety();

        self.scale_sparse_endgame(score.taper(self.phase))
    }

    pub fn evaluate_uncached(&self) -> i32 {
        if is_drawn_endgame(self.board) {
            return 0;
        }

        let mut score = Score::ZERO;

        score += self.eval_material_and_pst();
        score += self.eval_pawn_structure_uncached();
        score += self.eval_pieces();
        score += self.eval_king_safety();

        self.scale_sparse_endgame(score.taper(self.phase))
    }

    fn eval_material_and_pst(&self) -> Score {
        Score::new(self.board.eval_mg as i16, self.board.eval_eg as i16)
    }

    fn eval_pawn_structure(&self) -> Score {
        let key = pawn_hash(self.board);
        let static_score =
            Score(PAWN_CACHE.get_or_insert_with(key, || self.eval_pawn_structure_static().0));
        static_score + self.eval_pawn_structure_dynamic()
    }

    fn eval_pawn_structure_uncached(&self) -> Score {
        self.eval_pawn_structure_static() + self.eval_pawn_structure_dynamic()
    }

    fn eval_pawn_structure_static(&self) -> Score {
        let mut score = Score::ZERO;
        let white_pawns = self.board.bitboards[0][0];
        let black_pawns = self.board.bitboards[1][0];

        score += self.eval_pawns_for_color_static(Color::White, white_pawns, black_pawns);
        score -= self.eval_pawns_for_color_static(Color::Black, black_pawns, white_pawns);

        score
    }

    fn eval_pawn_structure_dynamic(&self) -> Score {
        let mut score = Score::ZERO;
        let white_pawns = self.board.bitboards[0][0];
        let black_pawns = self.board.bitboards[1][0];

        score += self.eval_pawns_for_color_dynamic(Color::White, white_pawns, black_pawns);
        score -= self.eval_pawns_for_color_dynamic(Color::Black, black_pawns, white_pawns);

        score
    }

    fn eval_pawns_for_color_static(&self, color: Color, own_pawns: u64, enemy_pawns: u64) -> Score {
        let mut score = Score::ZERO;
        let mut pawns = own_pawns;
        let own_pawn_attacks = Self::pawn_attack_map(color, own_pawns);
        let passers = Self::passed_pawns(color, own_pawns, enemy_pawns);

        while pawns != 0 {
            let sq = pawns.trailing_zeros() as u8;
            let file = Square::file(sq) as usize;
            let rank = if color == Color::White {
                Square::rank(sq) as usize
            } else {
                7 - Square::rank(sq) as usize
            };

            let pawns_on_file = (own_pawns & file_mask(file)).count_ones();
            if pawns_on_file > 1 {
                score -= DOUBLED_PAWN_PENALTY;
            }

            if (own_pawns & ADJACENT_FILES[file]) == 0 {
                score -= ISOLATED_PAWN_PENALTY;
            }

            if (passers & (1u64 << sq)) != 0 {
                let bonus = Score::new(PASSED_PAWN_BONUS_MG[rank], PASSED_PAWN_BONUS_EG[rank]);
                score += bonus;

                if self.has_adjacent_pawn(sq, color, own_pawns) {
                    score += CONNECTED_PASSED_BONUS;
                }

                if (own_pawn_attacks & (1u64 << sq)) != 0 {
                    score += Score::new(
                        PROTECTED_PASSED_PAWN_BONUS_MG[rank],
                        PROTECTED_PASSED_PAWN_BONUS_EG[rank],
                    );
                }

                // Occupancy-dependent passed-pawn bonuses are evaluated separately.
            }

            if self.is_backward_pawn(sq, color, own_pawns, enemy_pawns) {
                score -= BACKWARD_PAWN_PENALTY;
            }

            pawns &= pawns - 1;
        }

        score
    }

    fn eval_pawns_for_color_dynamic(
        &self,
        color: Color,
        own_pawns: u64,
        enemy_pawns: u64,
    ) -> Score {
        let mut score = Score::ZERO;
        let mut passers = Self::passed_pawns(color, own_pawns, enemy_pawns);

        while passers != 0 {
            let sq = passers.trailing_zeros() as u8;
            let rank = if color == Color::White {
                Square::rank(sq) as usize
            } else {
                7 - Square::rank(sq) as usize
            };

            if let Some(stop_sq) = Self::advance_square(sq, color) {
                if (self.occupied & (1u64 << stop_sq)) != 0 {
                    score -= Score::new(
                        BLOCKED_PASSED_PAWN_PENALTY_MG[rank],
                        BLOCKED_PASSED_PAWN_PENALTY_EG[rank],
                    );
                }
            }

            score += self.eval_passed_pawn_rook_support(color, sq);

            passers &= passers - 1;
        }

        score
    }

    #[inline(always)]
    fn pawn_attack_map(color: Color, pawns: u64) -> u64 {
        match color {
            Color::White => {
                ((pawns & !0x0101010101010101u64) << 7) | ((pawns & !0x8080808080808080u64) << 9)
            }
            Color::Black => {
                ((pawns & !0x8080808080808080u64) >> 7) | ((pawns & !0x0101010101010101u64) >> 9)
            }
        }
    }

    #[inline(always)]
    fn advance_square(sq: u8, color: Color) -> Option<u8> {
        match color {
            Color::White if Square::rank(sq) < 7 => Some(sq + 8),
            Color::Black if Square::rank(sq) > 0 => Some(sq - 8),
            _ => None,
        }
    }

    /// Own pawns with no enemy pawn in front of them on the same or an
    /// adjacent file.
    #[inline(always)]
    fn passed_pawns(color: Color, own_pawns: u64, enemy_pawns: u64) -> u64 {
        // Squares strictly behind each enemy pawn (from the enemy's point of
        // view) on its own file, then widened to the adjacent files.
        let mut span = match color {
            Color::White => {
                let mut s = enemy_pawns >> 8;
                s |= s >> 8;
                s |= s >> 16;
                s |= s >> 32;
                s
            }
            Color::Black => {
                let mut s = enemy_pawns << 8;
                s |= s << 8;
                s |= s << 16;
                s |= s << 32;
                s
            }
        };
        span |= ((span & !FILE_A) >> 1) | ((span & !FILE_H) << 1);
        own_pawns & !span
    }

    #[inline(always)]
    fn has_adjacent_pawn(&self, sq: u8, _color: Color, own_pawns: u64) -> bool {
        (own_pawns & PAWN_MASKS.neighbor[sq as usize]) != 0
    }

    #[inline(always)]
    fn is_backward_pawn(&self, sq: u8, color: Color, own_pawns: u64, enemy_pawns: u64) -> bool {
        let cidx = color_idx(color);
        let sq = sq as usize;
        (own_pawns & PAWN_MASKS.backward_support[cidx][sq]) == 0
            && (enemy_pawns & PAWN_MASKS.backward_attack[cidx][sq]) != 0
    }

    fn eval_pieces(&self) -> Score {
        let mut score = Score::ZERO;

        if self.board.bitboards[0][2].count_ones() >= 2 {
            score += BISHOP_PAIR_BONUS;
        }
        if self.board.bitboards[1][2].count_ones() >= 2 {
            score -= BISHOP_PAIR_BONUS;
        }

        score += self.eval_pieces_for_color::<true>(Color::White);
        score -= self.eval_pieces_for_color::<true>(Color::Black);

        score
    }

    #[cfg(test)]
    fn eval_piece_activity_for_color(&self, color: Color) -> Score {
        self.eval_pieces_for_color::<false>(color)
    }

    /// Mobility and king-zone pressure for knights, bishops, rooks and queens.
    /// With `FULL`, also knight outposts and rook file / 7th-rank terms, so
    /// each piece set is walked only once.
    #[inline(always)]
    fn eval_pieces_for_color<const FULL: bool>(&self, color: Color) -> Score {
        let mut score = Score::ZERO;
        let cidx = color_idx(color);
        let own_occ = if color == Color::White {
            self.white_pieces
        } else {
            self.black_pieces
        };
        let own_pawns = self.board.bitboards[cidx][0];
        let enemy_pawns = self.board.bitboards[1 - cidx][0];
        let enemy_king = self.board.bitboards[1 - cidx][5];
        let enemy_king_zone = if enemy_king != 0 {
            let enemy_king_sq = enemy_king.trailing_zeros() as usize;
            enemy_king | KING_TABLE[enemy_king_sq]
        } else {
            0
        };

        let activity = |pt: usize, attacks: u64| -> Score {
            let attacks = attacks & !own_occ;
            let mut s = MOBILITY_BONUS[pt] * attacks.count_ones() as i32;
            if enemy_king_zone != 0 {
                s += KING_ZONE_ATTACK_BONUS[pt] * (attacks & enemy_king_zone).count_ones() as i32;
            }
            s
        };

        let mut bb = self.board.bitboards[cidx][1];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            score += activity(1, KNIGHT_TABLE[sq]);

            if FULL {
                let rank = sq / 8;
                let in_enemy_territory = match color {
                    Color::White => rank >= 4,
                    Color::Black => rank <= 3,
                };
                if in_enemy_territory
                    && (own_pawns & PAWN_MASKS.outpost_support[cidx][sq]) != 0
                    && (enemy_pawns & PAWN_MASKS.outpost_attack[cidx][sq]) == 0
                {
                    score += KNIGHT_OUTPOST_BONUS;
                }
            }

            bb &= bb - 1;
        }

        let mut bb = self.board.bitboards[cidx][2];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            score += activity(2, bishop_attacks(sq, self.occupied));
            bb &= bb - 1;
        }

        let seventh = if color == Color::White { 6 } else { 1 };
        let mut bb = self.board.bitboards[cidx][3];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            score += activity(3, rook_attacks(sq, self.occupied));

            if FULL {
                let file_bb = file_mask(sq % 8);
                if (own_pawns & file_bb) == 0 && (enemy_pawns & file_bb) == 0 {
                    score += ROOK_OPEN_FILE_BONUS;
                } else if (own_pawns & file_bb) == 0 {
                    score += ROOK_SEMI_OPEN_FILE_BONUS;
                }

                if sq / 8 == seventh {
                    score += ROOK_ON_7TH_BONUS;
                }
            }

            bb &= bb - 1;
        }

        let mut bb = self.board.bitboards[cidx][4];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            score += activity(
                4,
                bishop_attacks(sq, self.occupied) | rook_attacks(sq, self.occupied),
            );
            bb &= bb - 1;
        }

        score
    }

    fn eval_passed_pawn_rook_support(&self, color: Color, pawn_sq: u8) -> Score {
        let cidx = color_idx(color);
        let file_bb = file_mask(Square::file(pawn_sq) as usize);
        let mut score = Score::ZERO;

        let mut own_rooks = self.board.bitboards[cidx][3] & file_bb;
        while own_rooks != 0 {
            let rook_sq = own_rooks.trailing_zeros() as u8;
            if Self::is_rook_behind_passed_pawn(color, rook_sq, pawn_sq)
                && self.clear_file_between(rook_sq, pawn_sq)
            {
                score += ROOK_BEHIND_PASSED_PAWN_BONUS;
                break;
            }
            own_rooks &= own_rooks - 1;
        }

        let mut enemy_rooks = self.board.bitboards[1 - cidx][3] & file_bb;
        while enemy_rooks != 0 {
            let rook_sq = enemy_rooks.trailing_zeros() as u8;
            if Self::is_rook_in_front_of_passed_pawn(color, rook_sq, pawn_sq)
                && self.clear_file_between(rook_sq, pawn_sq)
            {
                score -= ENEMY_ROOK_BEHIND_PASSED_PAWN_PENALTY;
                break;
            }
            enemy_rooks &= enemy_rooks - 1;
        }

        score
    }

    #[inline(always)]
    fn is_rook_behind_passed_pawn(color: Color, rook_sq: u8, pawn_sq: u8) -> bool {
        if Square::file(rook_sq) != Square::file(pawn_sq) {
            return false;
        }

        match color {
            Color::White => Square::rank(rook_sq) < Square::rank(pawn_sq),
            Color::Black => Square::rank(rook_sq) > Square::rank(pawn_sq),
        }
    }

    #[inline(always)]
    fn is_rook_in_front_of_passed_pawn(color: Color, rook_sq: u8, pawn_sq: u8) -> bool {
        if Square::file(rook_sq) != Square::file(pawn_sq) {
            return false;
        }

        match color {
            Color::White => Square::rank(rook_sq) > Square::rank(pawn_sq),
            Color::Black => Square::rank(rook_sq) < Square::rank(pawn_sq),
        }
    }

    #[inline(always)]
    fn clear_file_between(&self, a: u8, b: u8) -> bool {
        Square::file(a) == Square::file(b)
            && (between_mask(a as usize, b as usize) & self.occupied) == 0
    }

    fn eval_king_safety(&self) -> Score {
        let mut score = Score::ZERO;

        score += self.eval_king_safety_for_color(Color::White);
        score -= self.eval_king_safety_for_color(Color::Black);

        score
    }

    fn eval_king_safety_for_color(&self, color: Color) -> Score {
        let mut score = Score::ZERO;
        let cidx = color_idx(color);

        let king_bb = self.board.bitboards[cidx][5];
        if king_bb == 0 {
            return score;
        }
        let king_sq = king_bb.trailing_zeros() as usize;
        let king_file = king_sq % 8;
        let king_rank = king_sq / 8;

        let own_pawns = self.board.bitboards[cidx][0];
        let enemy_pawns = self.board.bitboards[1 - cidx][0];

        if self.phase > Phase::TOTAL_PHASE / 2 {
            let shelter_rank = if color == Color::White {
                king_rank + 1
            } else {
                king_rank.saturating_sub(1)
            };

            if shelter_rank < 8 {
                for f in king_file.saturating_sub(1)..=(king_file + 1).min(7) {
                    let shelter_sq = shelter_rank * 8 + f;
                    if (own_pawns & (1u64 << shelter_sq)) == 0 {
                        score -= PAWN_SHELTER_PENALTY;
                    }
                }
            }

            let file_mask = 0x0101010101010101u64 << king_file;
            if (own_pawns & file_mask) == 0 && (enemy_pawns & file_mask) == 0 {
                score -= KING_OPEN_FILE_PENALTY;
            } else if (own_pawns & file_mask) == 0 {
                score -= KING_SEMI_OPEN_FILE_PENALTY;
            }
        }

        score
    }

    fn scale_sparse_endgame(&self, score: i32) -> i32 {
        if self.is_opposite_colored_bishops_endgame() {
            return score * OPPOSITE_BISHOPS_SCALE_NUM / OPPOSITE_BISHOPS_SCALE_DEN;
        }

        score
    }

    fn is_opposite_colored_bishops_endgame(&self) -> bool {
        if self.board.bitboards[0][4] != 0
            || self.board.bitboards[1][4] != 0
            || self.board.bitboards[0][3] != 0
            || self.board.bitboards[1][3] != 0
            || self.board.bitboards[0][1] != 0
            || self.board.bitboards[1][1] != 0
        {
            return false;
        }

        if self.board.bitboards[0][2].count_ones() != 1
            || self.board.bitboards[1][2].count_ones() != 1
        {
            return false;
        }

        if (self.board.bitboards[0][0] | self.board.bitboards[1][0]) == 0 {
            return false;
        }

        let white_sq = self.board.bitboards[0][2].trailing_zeros() as u8;
        let black_sq = self.board.bitboards[1][2].trailing_zeros() as u8;

        Self::is_light_square(white_sq) != Self::is_light_square(black_sq)
    }

    #[inline(always)]
    fn is_light_square(sq: u8) -> bool {
        ((Square::file(sq) + Square::rank(sq)) & 1) == 0
    }
}

#[cfg(test)]
pub(crate) fn evaluate_uncached(board: &Board) -> i32 {
    let evaluator = Evaluator::new(board);
    evaluator.evaluate_uncached()
}

#[inline]
pub fn evaluate(board: &Board, color: Color) -> i32 {
    if is_drawn_endgame(board) {
        return 0;
    }

    // The pawn-structure cache returns exactly what the uncached computation
    // would, so the search path uses it on eval-cache misses.
    let base =
        EVAL_CACHE.get_or_insert_with(board.hash, || Evaluator::new(board).evaluate_non_drawn());

    if color == Color::White {
        base + TEMPO_BONUS
    } else {
        -base + TEMPO_BONUS
    }
}

#[inline]
pub fn game_phase(board: &Board) -> i32 {
    Evaluator::calculate_phase(board)
}

pub fn is_drawn_endgame(board: &Board) -> bool {
    let white_pieces = board.white_occ;
    let black_pieces = board.black_occ;
    let total = (white_pieces | black_pieces).count_ones();

    if total <= 2 {
        return true;
    }

    if total == 3 {
        let white_minors = board.bitboards[0][1].count_ones() + board.bitboards[0][2].count_ones();
        let black_minors = board.bitboards[1][1].count_ones() + board.bitboards[1][2].count_ones();
        if white_minors + black_minors == 1 {
            return true;
        }
    }

    if total == 4 {
        if board.bitboards[0][0] == 0
            && board.bitboards[1][0] == 0
            && board.bitboards[0][3] == 0
            && board.bitboards[1][3] == 0
            && board.bitboards[0][4] == 0
            && board.bitboards[1][4] == 0
        {
            let white_minors =
                board.bitboards[0][1].count_ones() + board.bitboards[0][2].count_ones();
            let black_minors =
                board.bitboards[1][1].count_ones() + board.bitboards[1][2].count_ones();
            if white_minors == 1 && black_minors == 1 {
                return true;
            }
        }

        let white_knights = board.bitboards[0][1].count_ones();
        let black_knights = board.bitboards[1][1].count_ones();
        if (white_knights == 2 && black_knights == 0) || (white_knights == 0 && black_knights == 2)
        {
            if board.bitboards[0][0] == 0 && board.bitboards[1][0] == 0 {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Game;
    use crate::pieces::{Piece, PieceType};

    #[test]
    fn test_score_pack_unpack() {
        let s = Score::new(100, -50);
        assert_eq!(s.mg(), 100);
        assert_eq!(s.eg(), -50);
    }

    #[test]
    fn test_score_operations() {
        let a = Score::new(100, 50);
        let b = Score::new(30, 20);
        let c = a + b;
        assert_eq!(c.mg(), 130);
        assert_eq!(c.eg(), 70);
    }

    #[test]
    fn test_taper() {
        let s = Score::new(100, 0); // 100 in MG, 0 in EG
        assert_eq!(s.taper(Phase::TOTAL_PHASE), 100); // Full MG
        assert_eq!(s.taper(0), 0); // Full EG
        assert_eq!(s.taper(Phase::TOTAL_PHASE / 2), 50); // Half
    }

    #[test]
    fn test_starting_position_eval() {
        let game = Game::new();
        let score = evaluate(&game.board, Color::White);
        assert!(score.abs() < 50, "Starting eval: {}", score);
    }

    #[test]
    fn test_material_advantage() {
        let mut game = Game::new();
        game.board.set_index(3, 7, None);

        let score = evaluate(&game.board, Color::White);
        assert!(score > 800, "Score with queen up: {}", score);
    }

    fn bare_board() -> Board {
        let mut board = Board::new();
        board.castling = [[false, false], [false, false]];
        board
    }

    #[test]
    fn test_piece_activity_prefers_active_development() {
        let mut active = bare_board();
        active.set(
            "g1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        active.set(
            "g8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        active.set(
            "h5",
            Some(Piece {
                piece_type: PieceType::Queen,
                color: Color::White,
            }),
        );
        active.set(
            "c4",
            Some(Piece {
                piece_type: PieceType::Bishop,
                color: Color::White,
            }),
        );
        active.set(
            "f3",
            Some(Piece {
                piece_type: PieceType::Knight,
                color: Color::White,
            }),
        );
        active.set(
            "g7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        active.set(
            "h7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        let mut passive = bare_board();
        passive.set(
            "g1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        passive.set(
            "g8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        passive.set(
            "d1",
            Some(Piece {
                piece_type: PieceType::Queen,
                color: Color::White,
            }),
        );
        passive.set(
            "c1",
            Some(Piece {
                piece_type: PieceType::Bishop,
                color: Color::White,
            }),
        );
        passive.set(
            "b1",
            Some(Piece {
                piece_type: PieceType::Knight,
                color: Color::White,
            }),
        );
        passive.set(
            "g7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        passive.set(
            "h7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        let active_score = Evaluator::new(&active).eval_piece_activity_for_color(Color::White);
        let passive_score = Evaluator::new(&passive).eval_piece_activity_for_color(Color::White);

        assert!(
            active_score.taper(Phase::TOTAL_PHASE) > passive_score.taper(Phase::TOTAL_PHASE),
            "Active piece setup should outscore passive setup: active={}, passive={}",
            active_score.taper(Phase::TOTAL_PHASE),
            passive_score.taper(Phase::TOTAL_PHASE),
        );
    }

    #[test]
    fn test_protected_passed_pawn_scores_higher() {
        let mut protected = bare_board();
        protected.set(
            "g1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        protected.set(
            "g8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        protected.set(
            "d5",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        protected.set(
            "e4",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        protected.set(
            "a2",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        protected.set(
            "a7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        let mut unprotected = bare_board();
        unprotected.set(
            "g1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        unprotected.set(
            "g8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        unprotected.set(
            "d5",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        unprotected.set(
            "e3",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        unprotected.set(
            "a2",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        unprotected.set(
            "a7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        let protected_eval = Evaluator::new(&protected).eval_pawns_for_color_static(
            Color::White,
            protected.bitboards[0][0],
            protected.bitboards[1][0],
        );
        let unprotected_eval = Evaluator::new(&unprotected).eval_pawns_for_color_static(
            Color::White,
            unprotected.bitboards[0][0],
            unprotected.bitboards[1][0],
        );

        assert!(
            protected_eval.taper(0) > unprotected_eval.taper(0),
            "Protected passer should score higher: protected={}, unprotected={}",
            protected_eval.taper(0),
            unprotected_eval.taper(0),
        );
    }

    #[test]
    fn test_rook_behind_passed_pawn_scores_higher() {
        let mut rook_behind = bare_board();
        rook_behind.set(
            "g1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        rook_behind.set(
            "g8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        rook_behind.set(
            "d1",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::White,
            }),
        );
        rook_behind.set(
            "d5",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        rook_behind.set(
            "a2",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        rook_behind.set(
            "a7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        rook_behind.set(
            "h2",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        rook_behind.set(
            "h7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        let mut rook_sideways = bare_board();
        rook_sideways.set(
            "g1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        rook_sideways.set(
            "g8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        rook_sideways.set(
            "h1",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::White,
            }),
        );
        rook_sideways.set(
            "d5",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        rook_sideways.set(
            "a2",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        rook_sideways.set(
            "a7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        rook_sideways.set(
            "h2",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        rook_sideways.set(
            "h7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );

        let behind_eval = Evaluator::new(&rook_behind).eval_pawns_for_color_dynamic(
            Color::White,
            rook_behind.bitboards[0][0],
            rook_behind.bitboards[1][0],
        );
        let sideways_eval = Evaluator::new(&rook_sideways).eval_pawns_for_color_dynamic(
            Color::White,
            rook_sideways.bitboards[0][0],
            rook_sideways.bitboards[1][0],
        );

        assert!(
            behind_eval.taper(0) > sideways_eval.taper(0),
            "Rook behind passer should score higher: behind={}, sideways={}",
            behind_eval.taper(0),
            sideways_eval.taper(0),
        );
    }

    #[test]
    fn test_drawn_minor_endgame_scores_zero() {
        let mut board = bare_board();
        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "e8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "c1",
            Some(Piece {
                piece_type: PieceType::Bishop,
                color: Color::White,
            }),
        );
        board.set(
            "f6",
            Some(Piece {
                piece_type: PieceType::Knight,
                color: Color::Black,
            }),
        );

        assert_eq!(evaluate(&board, Color::White), 0);
        assert_eq!(evaluate(&board, Color::Black), 0);
    }

    #[test]
    fn test_eval_cache_matches_uncached_and_survives_unmake() {
        let mut game = Game::new();
        let start_hash = game.board.hash;

        let cached_start = evaluate(&game.board, Color::White);
        let uncached_start = evaluate_uncached(&game.board) + TEMPO_BONUS;
        assert_eq!(cached_start, uncached_start);
        assert_eq!(
            evaluate(&game.board, Color::Black),
            -evaluate_uncached(&game.board) + TEMPO_BONUS
        );

        let first = game.board.make_move_state("e2", "e4").unwrap();
        let second = game.board.make_move_state("e7", "e5").unwrap();
        let third = game.board.make_move_state("g1", "f3").unwrap();

        let cached_mid = evaluate(&game.board, Color::White);
        let uncached_mid = evaluate_uncached(&game.board) + TEMPO_BONUS;
        assert_eq!(cached_mid, uncached_mid);
        assert_eq!(
            evaluate(&game.board, Color::Black),
            -evaluate_uncached(&game.board) + TEMPO_BONUS
        );

        game.board.unmake_move(third);
        game.board.unmake_move(second);
        game.board.unmake_move(first);

        assert_eq!(game.board.hash, start_hash);

        let cached_restored = evaluate(&game.board, Color::White);
        let uncached_restored = evaluate_uncached(&game.board) + TEMPO_BONUS;
        assert_eq!(cached_restored, uncached_restored);
        assert_eq!(cached_restored, cached_start);
    }

    #[test]
    fn test_pawn_cache_matches_uncached_and_survives_unmake() {
        let mut game = Game::new();
        let start_hash = pawn_hash(&game.board);

        let cached_start = Evaluator::new(&game.board).eval_pawn_structure();
        let uncached_start = Evaluator::new(&game.board).eval_pawn_structure_uncached();
        assert_eq!(cached_start, uncached_start);

        let first = game.board.make_move_state("e2", "e4").unwrap();
        let second = game.board.make_move_state("e7", "e5").unwrap();
        let third = game.board.make_move_state("g1", "f3").unwrap();

        let mid_hash = pawn_hash(&game.board);
        let cached_mid = Evaluator::new(&game.board).eval_pawn_structure();
        let uncached_mid = Evaluator::new(&game.board).eval_pawn_structure_uncached();
        assert_eq!(cached_mid, uncached_mid);

        game.board.unmake_move(third);
        game.board.unmake_move(second);
        game.board.unmake_move(first);

        assert_eq!(pawn_hash(&game.board), start_hash);

        let cached_restored = Evaluator::new(&game.board).eval_pawn_structure();
        let uncached_restored = Evaluator::new(&game.board).eval_pawn_structure_uncached();
        assert_eq!(cached_restored, uncached_restored);
        assert_eq!(cached_restored, cached_start);
        assert_ne!(mid_hash, start_hash);
    }

    // Straightforward loop-based versions of the pawn masks, kept as a
    // reference for the precomputed tables and bitboard tricks.
    fn ref_passed(sq: usize, color: Color, enemy: u64) -> bool {
        let (file, rank) = (sq % 8, sq / 8);
        let ranks = match color {
            Color::White => (rank + 1)..8,
            Color::Black => 0..rank,
        };
        let mut mask = 0u64;
        for r in ranks {
            for f in file.saturating_sub(1)..=(file + 1).min(7) {
                mask |= 1u64 << (r * 8 + f);
            }
        }
        enemy & mask == 0
    }

    fn ref_adjacent(sq: usize, own: u64) -> bool {
        let (file, rank) = (sq % 8, sq / 8);
        for f in file.saturating_sub(1)..=(file + 1).min(7) {
            if f == file {
                continue;
            }
            for r in rank.saturating_sub(1)..=(rank + 1).min(7) {
                if own & (1u64 << (r * 8 + f)) != 0 {
                    return true;
                }
            }
        }
        false
    }

    fn ref_backward(sq: usize, color: Color, own: u64, enemy: u64) -> bool {
        let (file, rank) = (sq % 8, sq / 8);
        let ranks = match color {
            Color::White => 0..(rank + 1),
            Color::Black => rank..8,
        };
        let mut support = 0u64;
        for r in ranks {
            for f in file.saturating_sub(1)..=(file + 1).min(7) {
                if f != file {
                    support |= 1u64 << (r * 8 + f);
                }
            }
        }
        if own & support != 0 {
            return false;
        }
        let mut attacks = 0u64;
        match color {
            Color::White if rank < 6 => {
                if file > 0 {
                    attacks |= 1u64 << ((rank + 2) * 8 + file - 1);
                }
                if file < 7 {
                    attacks |= 1u64 << ((rank + 2) * 8 + file + 1);
                }
            }
            Color::Black if rank > 1 => {
                if file > 0 {
                    attacks |= 1u64 << ((rank - 2) * 8 + file - 1);
                }
                if file < 7 {
                    attacks |= 1u64 << ((rank - 2) * 8 + file + 1);
                }
            }
            _ => {}
        }
        enemy & attacks != 0
    }

    fn ref_outpost(sq: usize, color: Color, own: u64, enemy: u64) -> bool {
        let (file, rank) = (sq % 8, sq / 8);
        let support_rank = match color {
            Color::White if rank > 0 => Some(rank - 1),
            Color::Black if rank < 7 => Some(rank + 1),
            _ => None,
        };
        let mut support = 0u64;
        if let Some(r) = support_rank {
            if file > 0 {
                support |= 1u64 << (r * 8 + file - 1);
            }
            if file < 7 {
                support |= 1u64 << (r * 8 + file + 1);
            }
        }
        let ranks = match color {
            Color::White => (rank + 1)..8,
            Color::Black => 0..rank,
        };
        let mut attack = 0u64;
        for r in ranks {
            for f in [file.wrapping_sub(1), file + 1] {
                if f < 8 {
                    attack |= 1u64 << (r * 8 + f);
                }
            }
        }
        own & support != 0 && enemy & attack == 0
    }

    #[test]
    fn test_pawn_masks_match_reference() {
        let board = bare_board();
        let mut eval = Evaluator::new(&board);
        let mut seed = 0x9E3779B97F4A7C15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };

        for _ in 0..4000 {
            let density = next() % 3;
            let mut own = next() & 0x00FF_FFFF_FFFF_FF00;
            let mut enemy = next() & 0x00FF_FFFF_FFFF_FF00;
            for _ in 0..density {
                own &= next();
                enemy &= next();
            }
            enemy &= !own;

            for color in [Color::White, Color::Black] {
                let cidx = color_idx(color);
                let passers = Evaluator::passed_pawns(color, own, enemy);
                for sq in 0..64usize {
                    let bit = 1u64 << sq;
                    if own & bit != 0 {
                        assert_eq!(passers & bit != 0, ref_passed(sq, color, enemy));
                    }
                    assert_eq!(
                        eval.has_adjacent_pawn(sq as u8, color, own),
                        ref_adjacent(sq, own)
                    );
                    assert_eq!(
                        eval.is_backward_pawn(sq as u8, color, own, enemy),
                        ref_backward(sq, color, own, enemy),
                        "backward sq={sq} color={color:?}"
                    );
                    let outpost = (own & PAWN_MASKS.outpost_support[cidx][sq]) != 0
                        && (enemy & PAWN_MASKS.outpost_attack[cidx][sq]) == 0;
                    assert_eq!(outpost, ref_outpost(sq, color, own, enemy));
                }
            }

            eval.occupied = own | enemy;
            for a in 0..64u8 {
                for b in 0..64u8 {
                    let expected = if Square::file(a) != Square::file(b) {
                        false
                    } else {
                        let lo = Square::rank(a).min(Square::rank(b)) + 1;
                        let hi = Square::rank(a).max(Square::rank(b));
                        (lo..hi).all(|r| {
                            eval.occupied & (1u64 << Square::make(Square::file(a), r)) == 0
                        })
                    };
                    assert_eq!(eval.clear_file_between(a, b), expected);
                }
            }
        }
    }
}
