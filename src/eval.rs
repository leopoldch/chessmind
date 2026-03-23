use crate::board::{Board, color_idx};
use crate::eval_cache::EVAL_CACHE;
use crate::movegen::{KING_TABLE, KNIGHT_TABLE};
use crate::pieces::Color;
use crate::types::{Phase, Square};

#[derive(Copy, Clone, Default, Eq, PartialEq)]
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
        let white_pieces: u64 = board.bitboards[0].iter().fold(0, |a, b| a | b);
        let black_pieces: u64 = board.bitboards[1].iter().fold(0, |a, b| a | b);

        let phase = Self::calculate_phase(board);

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

        let mut score = Score::ZERO;

        score += self.eval_material_and_pst();

        score += self.eval_pawn_structure();

        score += self.eval_pieces();

        score += self.eval_king_safety();

        self.scale_sparse_endgame(score.taper(self.phase))
    }

    fn eval_material_and_pst(&self) -> Score {
        let mut score = Score::ZERO;

        for pt in 0..6 {
            let mut bb = self.board.bitboards[0][pt];
            while bb != 0 {
                let sq = bb.trailing_zeros() as usize;
                score += Score::new(
                    MATERIAL_MG[pt] + PST_MG[pt][sq],
                    MATERIAL_EG[pt] + PST_EG[pt][sq],
                );
                bb &= bb - 1;
            }

            let mut bb = self.board.bitboards[1][pt];
            while bb != 0 {
                let sq = bb.trailing_zeros() as usize;
                let flipped = Square::flip(sq as u8) as usize;
                score -= Score::new(
                    MATERIAL_MG[pt] + PST_MG[pt][flipped],
                    MATERIAL_EG[pt] + PST_EG[pt][flipped],
                );
                bb &= bb - 1;
            }
        }

        score
    }

    fn eval_pawn_structure(&self) -> Score {
        let mut score = Score::ZERO;
        let white_pawns = self.board.bitboards[0][0];
        let black_pawns = self.board.bitboards[1][0];

        score += self.eval_pawns_for_color(Color::White, white_pawns, black_pawns);

        score -= self.eval_pawns_for_color(Color::Black, black_pawns, white_pawns);

        score
    }

    fn eval_pawns_for_color(&self, color: Color, own_pawns: u64, enemy_pawns: u64) -> Score {
        let mut score = Score::ZERO;
        let mut pawns = own_pawns;
        let own_pawn_attacks = Self::pawn_attack_map(color, own_pawns);

        while pawns != 0 {
            let sq = pawns.trailing_zeros() as u8;
            let file = Square::file(sq) as usize;
            let rank = if color == Color::White {
                Square::rank(sq) as usize
            } else {
                7 - Square::rank(sq) as usize
            };

            let file_mask = 0x0101010101010101u64 << file;
            let pawns_on_file = (own_pawns & file_mask).count_ones();
            if pawns_on_file > 1 {
                score -= DOUBLED_PAWN_PENALTY;
            }

            let adjacent_files = match file {
                0 => 0x0202020202020202u64,
                7 => 0x4040404040404040u64,
                _ => (0x0101010101010101u64 << (file - 1)) | (0x0101010101010101u64 << (file + 1)),
            };
            if (own_pawns & adjacent_files) == 0 {
                score -= ISOLATED_PAWN_PENALTY;
            }

            if self.is_passed_pawn(sq, color, enemy_pawns) {
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

                if let Some(stop_sq) = Self::advance_square(sq, color) {
                    if (self.occupied & (1u64 << stop_sq)) != 0 {
                        score -= Score::new(
                            BLOCKED_PASSED_PAWN_PENALTY_MG[rank],
                            BLOCKED_PASSED_PAWN_PENALTY_EG[rank],
                        );
                    }
                }

                score += self.eval_passed_pawn_rook_support(color, sq);
            }

            if self.is_backward_pawn(sq, color, own_pawns, enemy_pawns) {
                score -= BACKWARD_PAWN_PENALTY;
            }

            pawns &= pawns - 1;
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

    fn is_passed_pawn(&self, sq: u8, color: Color, enemy_pawns: u64) -> bool {
        let file = Square::file(sq) as usize;
        let rank = Square::rank(sq) as usize;

        let mut mask = 0u64;

        match color {
            Color::White => {
                for r in (rank + 1)..8 {
                    for f in file.saturating_sub(1)..=(file + 1).min(7) {
                        mask |= 1u64 << (r * 8 + f);
                    }
                }
            }
            Color::Black => {
                for r in 0..rank {
                    for f in file.saturating_sub(1)..=(file + 1).min(7) {
                        mask |= 1u64 << (r * 8 + f);
                    }
                }
            }
        }

        (enemy_pawns & mask) == 0
    }

    fn has_adjacent_pawn(&self, sq: u8, _color: Color, own_pawns: u64) -> bool {
        let file = Square::file(sq) as usize;
        let rank = Square::rank(sq) as usize;

        for f in file.saturating_sub(1)..=(file + 1).min(7) {
            if f == file {
                continue;
            }
            for r in rank.saturating_sub(1)..=(rank + 1).min(7) {
                let check_sq = r * 8 + f;
                if (own_pawns & (1u64 << check_sq)) != 0 {
                    return true;
                }
            }
        }
        false
    }

    fn is_backward_pawn(&self, sq: u8, color: Color, own_pawns: u64, enemy_pawns: u64) -> bool {
        let file = Square::file(sq) as usize;
        let rank = Square::rank(sq) as usize;

        let _support_files = match file {
            0 => 0x0202020202020202u64,
            7 => 0x4040404040404040u64,
            _ => (0x0101010101010101u64 << (file - 1)) | (0x0101010101010101u64 << (file + 1)),
        };

        let support_mask = match color {
            Color::White => {
                let mut m = 0u64;
                for r in 0..=rank {
                    for f in file.saturating_sub(1)..=(file + 1).min(7) {
                        if f != file {
                            m |= 1u64 << (r * 8 + f);
                        }
                    }
                }
                m
            }
            Color::Black => {
                let mut m = 0u64;
                for r in rank..8 {
                    for f in file.saturating_sub(1)..=(file + 1).min(7) {
                        if f != file {
                            m |= 1u64 << (r * 8 + f);
                        }
                    }
                }
                m
            }
        };

        if (own_pawns & support_mask) == 0 {
            let advance_sq = match color {
                Color::White if rank < 7 => Some((rank + 1) * 8 + file),
                Color::Black if rank > 0 => Some((rank - 1) * 8 + file),
                _ => None,
            };

            if let Some(_adv) = advance_sq {
                let enemy_attacks = match color {
                    Color::White => {
                        let mut attacks = 0u64;
                        if file > 0 && rank < 6 {
                            attacks |= 1u64 << ((rank + 2) * 8 + file - 1);
                        }
                        if file < 7 && rank < 6 {
                            attacks |= 1u64 << ((rank + 2) * 8 + file + 1);
                        }
                        attacks
                    }
                    Color::Black => {
                        let mut attacks = 0u64;
                        if file > 0 && rank > 1 {
                            attacks |= 1u64 << ((rank - 2) * 8 + file - 1);
                        }
                        if file < 7 && rank > 1 {
                            attacks |= 1u64 << ((rank - 2) * 8 + file + 1);
                        }
                        attacks
                    }
                };
                return (enemy_pawns & enemy_attacks) != 0;
            }
        }

        false
    }

    fn eval_pieces(&self) -> Score {
        let mut score = Score::ZERO;

        if self.board.bitboards[0][2].count_ones() >= 2 {
            score += BISHOP_PAIR_BONUS;
        }
        if self.board.bitboards[1][2].count_ones() >= 2 {
            score -= BISHOP_PAIR_BONUS;
        }

        score += self.eval_rooks(Color::White);
        score -= self.eval_rooks(Color::Black);

        score += self.eval_knight_outposts(Color::White);
        score -= self.eval_knight_outposts(Color::Black);

        score += self.eval_piece_activity_for_color(Color::White);
        score -= self.eval_piece_activity_for_color(Color::Black);

        score
    }

    fn eval_piece_activity_for_color(&self, color: Color) -> Score {
        let mut score = Score::ZERO;
        let cidx = color_idx(color);
        let own_occ = if color == Color::White {
            self.white_pieces
        } else {
            self.black_pieces
        };
        let enemy_king = self.board.bitboards[1 - cidx][5];
        let enemy_king_zone = if enemy_king != 0 {
            let enemy_king_sq = enemy_king.trailing_zeros() as usize;
            enemy_king | KING_TABLE[enemy_king_sq]
        } else {
            0
        };

        for pt in 1..=4 {
            let mut bb = self.board.bitboards[cidx][pt];
            while bb != 0 {
                let sq = bb.trailing_zeros() as u8;
                let attacks = self.attacks_for_piece(pt, sq) & !own_occ;
                let mobility = attacks.count_ones() as i32;
                score += MOBILITY_BONUS[pt] * mobility;

                if enemy_king_zone != 0 {
                    let pressure = (attacks & enemy_king_zone).count_ones() as i32;
                    score += KING_ZONE_ATTACK_BONUS[pt] * pressure;
                }

                bb &= bb - 1;
            }
        }

        score
    }

    fn eval_rooks(&self, color: Color) -> Score {
        let mut score = Score::ZERO;
        let cidx = color_idx(color);
        let own_pawns = self.board.bitboards[cidx][0];
        let enemy_pawns = self.board.bitboards[1 - cidx][0];
        let rooks = self.board.bitboards[cidx][3];

        let mut bb = rooks;
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            let file = sq % 8;
            let rank = sq / 8;

            let file_mask = 0x0101010101010101u64 << file;

            if (own_pawns & file_mask) == 0 && (enemy_pawns & file_mask) == 0 {
                score += ROOK_OPEN_FILE_BONUS;
            } else if (own_pawns & file_mask) == 0 {
                score += ROOK_SEMI_OPEN_FILE_BONUS;
            }

            let seventh = if color == Color::White { 6 } else { 1 };
            if rank == seventh {
                score += ROOK_ON_7TH_BONUS;
            }

            bb &= bb - 1;
        }

        score
    }

    fn eval_knight_outposts(&self, color: Color) -> Score {
        let mut score = Score::ZERO;
        let cidx = color_idx(color);
        let own_pawns = self.board.bitboards[cidx][0];
        let enemy_pawns = self.board.bitboards[1 - cidx][0];
        let knights = self.board.bitboards[cidx][1];

        let mut bb = knights;
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            let file = sq % 8;
            let rank = sq / 8;

            let in_enemy_territory = match color {
                Color::White => rank >= 4,
                Color::Black => rank <= 3,
            };

            if in_enemy_territory {
                let supported = match color {
                    Color::White => {
                        let support_mask = if file > 0 && rank > 0 {
                            1u64 << ((rank - 1) * 8 + file - 1)
                        } else {
                            0
                        } | if file < 7 && rank > 0 {
                            1u64 << ((rank - 1) * 8 + file + 1)
                        } else {
                            0
                        };
                        (own_pawns & support_mask) != 0
                    }
                    Color::Black => {
                        let support_mask = if file > 0 && rank < 7 {
                            1u64 << ((rank + 1) * 8 + file - 1)
                        } else {
                            0
                        } | if file < 7 && rank < 7 {
                            1u64 << ((rank + 1) * 8 + file + 1)
                        } else {
                            0
                        };
                        (own_pawns & support_mask) != 0
                    }
                };

                let adjacent_files = match file {
                    0 => 0x0202020202020202u64,
                    7 => 0x4040404040404040u64,
                    _ => {
                        (0x0101010101010101u64 << (file - 1))
                            | (0x0101010101010101u64 << (file + 1))
                    }
                };

                let cant_be_attacked = match color {
                    Color::White => {
                        let mut attack_mask = 0u64;
                        for r in (rank + 1)..8 {
                            attack_mask |= adjacent_files & (0xFFu64 << (r * 8));
                        }
                        (enemy_pawns & attack_mask) == 0
                    }
                    Color::Black => {
                        let mut attack_mask = 0u64;
                        for r in 0..rank {
                            attack_mask |= adjacent_files & (0xFFu64 << (r * 8));
                        }
                        (enemy_pawns & attack_mask) == 0
                    }
                };

                if supported && cant_be_attacked {
                    score += KNIGHT_OUTPOST_BONUS;
                }
            }

            bb &= bb - 1;
        }

        score
    }

    #[inline(always)]
    fn attacks_for_piece(&self, pt: usize, sq: u8) -> u64 {
        match pt {
            1 => KNIGHT_TABLE[sq as usize],
            2 => self.bishop_attacks(sq),
            3 => self.rook_attacks(sq),
            4 => self.bishop_attacks(sq) | self.rook_attacks(sq),
            _ => 0,
        }
    }

    fn bishop_attacks(&self, sq: u8) -> u64 {
        self.ray_attacks(sq, &[(1, 1), (1, -1), (-1, 1), (-1, -1)])
    }

    fn rook_attacks(&self, sq: u8) -> u64 {
        self.ray_attacks(sq, &[(1, 0), (-1, 0), (0, 1), (0, -1)])
    }

    fn ray_attacks(&self, sq: u8, directions: &[(i8, i8)]) -> u64 {
        let x = (sq % 8) as i8;
        let y = (sq / 8) as i8;
        let mut attacks = 0u64;

        for &(dx, dy) in directions {
            let mut nx = x + dx;
            let mut ny = y + dy;
            while (0..8).contains(&nx) && (0..8).contains(&ny) {
                let idx = (ny as u8 * 8 + nx as u8) as usize;
                attacks |= 1u64 << idx;
                if (self.occupied & (1u64 << idx)) != 0 {
                    break;
                }
                nx += dx;
                ny += dy;
            }
        }

        attacks
    }

    fn eval_passed_pawn_rook_support(&self, color: Color, pawn_sq: u8) -> Score {
        let cidx = color_idx(color);
        let file_mask = 0x0101010101010101u64 << Square::file(pawn_sq);
        let mut score = Score::ZERO;

        let mut own_rooks = self.board.bitboards[cidx][3] & file_mask;
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

        let mut enemy_rooks = self.board.bitboards[1 - cidx][3] & file_mask;
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

    fn clear_file_between(&self, a: u8, b: u8) -> bool {
        if Square::file(a) != Square::file(b) {
            return false;
        }

        let file = Square::file(a);
        let start = Square::rank(a).min(Square::rank(b)) + 1;
        let end = Square::rank(a).max(Square::rank(b));

        for rank in start..end {
            let sq = Square::make(file, rank);
            if (self.occupied & (1u64 << sq)) != 0 {
                return false;
            }
        }

        true
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

#[inline]
pub(crate) fn evaluate_uncached(board: &Board) -> i32 {
    let evaluator = Evaluator::new(board);
    evaluator.evaluate()
}

#[inline]
pub fn evaluate(board: &Board, color: Color) -> i32 {
    if is_drawn_endgame(board) {
        return 0;
    }

    let base = EVAL_CACHE.get_or_insert_with(board.hash, || evaluate_uncached(board));

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
    let white_pieces: u64 = board.bitboards[0].iter().fold(0, |a, b| a | b);
    let black_pieces: u64 = board.bitboards[1].iter().fold(0, |a, b| a | b);
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

        let protected_eval = Evaluator::new(&protected).eval_pawns_for_color(
            Color::White,
            protected.bitboards[0][0],
            protected.bitboards[1][0],
        );
        let unprotected_eval = Evaluator::new(&unprotected).eval_pawns_for_color(
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

        let behind_eval = Evaluator::new(&rook_behind).eval_pawns_for_color(
            Color::White,
            rook_behind.bitboards[0][0],
            rook_behind.bitboards[1][0],
        );
        let sideways_eval = Evaluator::new(&rook_sideways).eval_pawns_for_color(
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
}
