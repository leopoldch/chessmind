use crate::board::{Board, color_idx};
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

#[allow(dead_code)]
const MOBILITY_BONUS_MG: [i16; 6] = [0, 4, 3, 2, 1, 0]; // P, N, B, R, Q, K
#[allow(dead_code)]
const MOBILITY_BONUS_EG: [i16; 6] = [0, 4, 3, 3, 2, 0];

const PAWN_SHELTER_PENALTY: Score = Score::new(20, 5);

const KING_OPEN_FILE_PENALTY: Score = Score::new(30, 0);

const KING_SEMI_OPEN_FILE_PENALTY: Score = Score::new(15, 0);

const BISHOP_PAIR_BONUS: Score = Score::new(30, 50);

const ROOK_OPEN_FILE_BONUS: Score = Score::new(20, 10);

const ROOK_SEMI_OPEN_FILE_BONUS: Score = Score::new(10, 5);

const ROOK_ON_7TH_BONUS: Score = Score::new(20, 30);

const KNIGHT_OUTPOST_BONUS: Score = Score::new(25, 15);

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

    pub fn evaluate(&self, color: Color) -> i32 {
        let mut score = Score::ZERO;

        score += self.eval_material_and_pst();

        score += self.eval_pawn_structure();

        score += self.eval_pieces();

        score += self.eval_king_safety();

        let tapered = score.taper(self.phase);

        let final_score = if color == Color::White {
            tapered + TEMPO_BONUS
        } else {
            -tapered + TEMPO_BONUS
        };

        final_score
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
            }

            if self.is_backward_pawn(sq, color, own_pawns, enemy_pawns) {
                score -= BACKWARD_PAWN_PENALTY;
            }

            pawns &= pawns - 1;
        }

        score
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
}

#[inline]
pub fn evaluate(board: &Board, color: Color) -> i32 {
    let evaluator = Evaluator::new(board);
    evaluator.evaluate(color)
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
}
