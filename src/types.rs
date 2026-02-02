use crate::pieces::PieceType;

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
#[repr(transparent)]
pub struct Move(pub u16);

impl Move {
    pub const NONE: Move = Move(0);

    pub const FLAG_NORMAL: u16 = 0b0000;
    pub const FLAG_DOUBLE_PUSH: u16 = 0b0001;
    pub const FLAG_KING_CASTLE: u16 = 0b0010;
    pub const FLAG_QUEEN_CASTLE: u16 = 0b0011;
    pub const FLAG_CAPTURE: u16 = 0b0100;
    pub const FLAG_EP_CAPTURE: u16 = 0b0101;
    pub const FLAG_PROMO_KNIGHT: u16 = 0b1000;
    pub const FLAG_PROMO_BISHOP: u16 = 0b1001;
    pub const FLAG_PROMO_ROOK: u16 = 0b1010;
    pub const FLAG_PROMO_QUEEN: u16 = 0b1011;
    pub const FLAG_PROMO_KNIGHT_CAP: u16 = 0b1100;
    pub const FLAG_PROMO_BISHOP_CAP: u16 = 0b1101;
    pub const FLAG_PROMO_ROOK_CAP: u16 = 0b1110;
    pub const FLAG_PROMO_QUEEN_CAP: u16 = 0b1111;

    #[inline(always)]
    pub const fn new(from: u8, to: u8, flags: u16) -> Self {
        Move(((flags & 0xF) << 12) | ((to as u16 & 0x3F) << 6) | (from as u16 & 0x3F))
    }

    #[inline(always)]
    pub const fn normal(from: u8, to: u8) -> Self {
        Self::new(from, to, Self::FLAG_NORMAL)
    }

    #[inline(always)]
    pub const fn capture(from: u8, to: u8) -> Self {
        Self::new(from, to, Self::FLAG_CAPTURE)
    }

    #[inline(always)]
    pub const fn promotion(from: u8, to: u8, piece: PieceType, is_capture: bool) -> Self {
        let base_flag = match piece {
            PieceType::Knight => Self::FLAG_PROMO_KNIGHT,
            PieceType::Bishop => Self::FLAG_PROMO_BISHOP,
            PieceType::Rook => Self::FLAG_PROMO_ROOK,
            _ => Self::FLAG_PROMO_QUEEN,
        };
        let flag = if is_capture {
            base_flag | 0b0100
        } else {
            base_flag
        };
        Self::new(from, to, flag)
    }

    #[inline(always)]
    pub const fn from_sq(self) -> u8 {
        (self.0 & 0x3F) as u8
    }

    #[inline(always)]
    pub const fn to_sq(self) -> u8 {
        ((self.0 >> 6) & 0x3F) as u8
    }

    #[inline(always)]
    pub const fn flags(self) -> u16 {
        (self.0 >> 12) & 0xF
    }

    #[inline(always)]
    pub const fn is_capture(self) -> bool {
        let f = self.flags();
        f == Self::FLAG_CAPTURE || f == Self::FLAG_EP_CAPTURE || f >= Self::FLAG_PROMO_KNIGHT_CAP
    }

    #[inline(always)]
    pub const fn is_promotion(self) -> bool {
        self.flags() >= Self::FLAG_PROMO_KNIGHT
    }

    #[inline(always)]
    pub const fn is_ep(self) -> bool {
        self.flags() == Self::FLAG_EP_CAPTURE
    }

    #[inline(always)]
    pub const fn is_castle(self) -> bool {
        let f = self.flags();
        f == Self::FLAG_KING_CASTLE || f == Self::FLAG_QUEEN_CASTLE
    }

    #[inline(always)]
    pub const fn is_double_push(self) -> bool {
        self.flags() == Self::FLAG_DOUBLE_PUSH
    }

    #[inline(always)]
    pub const fn promotion_piece(self) -> Option<PieceType> {
        match self.flags() {
            Self::FLAG_PROMO_KNIGHT | Self::FLAG_PROMO_KNIGHT_CAP => Some(PieceType::Knight),
            Self::FLAG_PROMO_BISHOP | Self::FLAG_PROMO_BISHOP_CAP => Some(PieceType::Bishop),
            Self::FLAG_PROMO_ROOK | Self::FLAG_PROMO_ROOK_CAP => Some(PieceType::Rook),
            Self::FLAG_PROMO_QUEEN | Self::FLAG_PROMO_QUEEN_CAP => Some(PieceType::Queen),
            _ => None,
        }
    }

    #[inline(always)]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }

    pub fn to_algebraic(self) -> String {
        let from = self.from_sq();
        let to = self.to_sq();
        let from_file = (b'a' + (from % 8)) as char;
        let from_rank = (b'1' + (from / 8)) as char;
        let to_file = (b'a' + (to % 8)) as char;
        let to_rank = (b'1' + (to / 8)) as char;

        let mut s = format!("{}{}{}{}", from_file, from_rank, to_file, to_rank);

        if let Some(promo) = self.promotion_piece() {
            let c = match promo {
                PieceType::Knight => 'n',
                PieceType::Bishop => 'b',
                PieceType::Rook => 'r',
                PieceType::Queen => 'q',
                _ => 'q',
            };
            s.push(c);
        }

        s
    }

    pub fn from_algebraic(
        s: &str,
        from_sq: u8,
        to_sq: u8,
        is_capture: bool,
        flags_override: Option<u16>,
    ) -> Self {
        if let Some(flags) = flags_override {
            return Self::new(from_sq, to_sq, flags);
        }

        if s.len() == 5 {
            let promo = match s.chars().nth(4).unwrap_or('q') {
                'n' | 'N' => PieceType::Knight,
                'b' | 'B' => PieceType::Bishop,
                'r' | 'R' => PieceType::Rook,
                _ => PieceType::Queen,
            };
            return Self::promotion(from_sq, to_sq, promo, is_capture);
        }

        if is_capture {
            Self::capture(from_sq, to_sq)
        } else {
            Self::normal(from_sq, to_sq)
        }
    }
}

pub const MAX_MOVES: usize = 256;

#[derive(Clone)]
pub struct MoveList {
    moves: [Move; MAX_MOVES],
    count: usize,
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}

impl MoveList {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            moves: [Move::NONE; MAX_MOVES],
            count: 0,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, m: Move) {
        debug_assert!(self.count < MAX_MOVES);
        self.moves[self.count] = m;
        self.count += 1;
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.count
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline(always)]
    pub fn get(&self, idx: usize) -> Option<Move> {
        if idx < self.count {
            Some(self.moves[idx])
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut Move> {
        if idx < self.count {
            Some(&mut self.moves[idx])
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn iter(&self) -> impl Iterator<Item = &Move> {
        self.moves[..self.count].iter()
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.count = 0;
    }

    #[inline(always)]
    pub fn swap(&mut self, i: usize, j: usize) {
        self.moves.swap(i, j);
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.count]
    }

    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [Move] {
        &mut self.moves[..self.count]
    }
}

impl std::ops::Index<usize> for MoveList {
    type Output = Move;

    #[inline(always)]
    fn index(&self, idx: usize) -> &Self::Output {
        &self.moves[idx]
    }
}

impl std::ops::IndexMut<usize> for MoveList {
    #[inline(always)]
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        &mut self.moves[idx]
    }
}

pub struct PieceValues;

impl PieceValues {
    pub const PAWN: i32 = 100;
    pub const KNIGHT: i32 = 320;
    pub const BISHOP: i32 = 330;
    pub const ROOK: i32 = 500;
    pub const QUEEN: i32 = 900;
    pub const KING: i32 = 20000; // Arbitrary high value

    pub const VALUES: [i32; 6] = [
        Self::PAWN,
        Self::KNIGHT,
        Self::BISHOP,
        Self::ROOK,
        Self::QUEEN,
        Self::KING,
    ];

    #[inline(always)]
    pub const fn value(pt: PieceType) -> i32 {
        match pt {
            PieceType::Pawn => Self::PAWN,
            PieceType::Knight => Self::KNIGHT,
            PieceType::Bishop => Self::BISHOP,
            PieceType::Rook => Self::ROOK,
            PieceType::Queen => Self::QUEEN,
            PieceType::King => Self::KING,
        }
    }

    #[inline(always)]
    pub const fn value_by_idx(idx: usize) -> i32 {
        Self::VALUES[idx]
    }
}

pub static MVV_LVA: [[i32; 6]; 6] = {
    let mut table = [[0i32; 6]; 6];
    let values = [100, 320, 330, 500, 900, 20000]; // Pawn, Knight, Bishop, Rook, Queen, King

    let mut victim = 0;
    while victim < 6 {
        let mut attacker = 0;
        while attacker < 6 {
            table[victim][attacker] = values[victim] * 10 - values[attacker];
            attacker += 1;
        }
        victim += 1;
    }
    table
};

#[inline(always)]
pub const fn mvv_lva_score(victim_idx: usize, attacker_idx: usize) -> i32 {
    MVV_LVA[victim_idx][attacker_idx]
}

pub struct Phase;

impl Phase {
    pub const PAWN_PHASE: i32 = 0;
    pub const KNIGHT_PHASE: i32 = 1;
    pub const BISHOP_PHASE: i32 = 1;
    pub const ROOK_PHASE: i32 = 2;
    pub const QUEEN_PHASE: i32 = 4;

    pub const TOTAL_PHASE: i32 = 4 * Self::KNIGHT_PHASE
        + 4 * Self::BISHOP_PHASE
        + 4 * Self::ROOK_PHASE
        + 2 * Self::QUEEN_PHASE;

    pub const WEIGHTS: [i32; 6] = [
        Self::PAWN_PHASE,
        Self::KNIGHT_PHASE,
        Self::BISHOP_PHASE,
        Self::ROOK_PHASE,
        Self::QUEEN_PHASE,
        0, // King
    ];

    #[inline(always)]
    pub const fn calculate(knights: i32, bishops: i32, rooks: i32, queens: i32) -> i32 {
        knights * Self::KNIGHT_PHASE
            + bishops * Self::BISHOP_PHASE
            + rooks * Self::ROOK_PHASE
            + queens * Self::QUEEN_PHASE
    }
}

#[derive(Clone, Copy)]
pub struct UndoState {
    pub mv: Move,
    pub captured: u8,
    pub captured_sq: u8,
    pub prev_ep: u8,
    pub prev_castling: u8,
    pub prev_hash: u64,
}

impl UndoState {
    pub const NO_CAPTURE: u8 = 6;
    pub const NO_EP: u8 = 64;

    #[inline(always)]
    pub const fn has_capture(&self) -> bool {
        self.captured != Self::NO_CAPTURE
    }
}

pub struct Square;

impl Square {
    #[inline(always)]
    pub const fn file(sq: u8) -> u8 {
        sq % 8
    }

    #[inline(always)]
    pub const fn rank(sq: u8) -> u8 {
        sq / 8
    }

    #[inline(always)]
    pub const fn make(file: u8, rank: u8) -> u8 {
        rank * 8 + file
    }

    #[inline(always)]
    pub const fn flip(sq: u8) -> u8 {
        sq ^ 56
    }

    pub fn to_algebraic(sq: u8) -> String {
        let file = (b'a' + (sq % 8)) as char;
        let rank = (b'1' + (sq / 8)) as char;
        format!("{}{}", file, rank)
    }

    pub fn from_algebraic(s: &str) -> Option<u8> {
        if s.len() != 2 {
            return None;
        }
        let bytes = s.as_bytes();
        let file = bytes[0].wrapping_sub(b'a');
        let rank = bytes[1].wrapping_sub(b'1');
        if file < 8 && rank < 8 {
            Some(rank * 8 + file)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_encoding() {
        let m = Move::new(12, 28, Move::FLAG_NORMAL); // e2-e4
        assert_eq!(m.from_sq(), 12);
        assert_eq!(m.to_sq(), 28);
        assert_eq!(m.flags(), Move::FLAG_NORMAL);
        assert!(!m.is_capture());
        assert!(!m.is_promotion());
    }

    #[test]
    fn test_move_capture() {
        let m = Move::capture(12, 21); // e2xf3
        assert!(m.is_capture());
        assert!(!m.is_promotion());
    }

    #[test]
    fn test_move_promotion() {
        let m = Move::promotion(52, 60, PieceType::Queen, false); // e7-e8=Q
        assert!(m.is_promotion());
        assert!(!m.is_capture());
        assert_eq!(m.promotion_piece(), Some(PieceType::Queen));
    }

    #[test]
    fn test_move_list() {
        let mut list = MoveList::new();
        assert!(list.is_empty());

        list.push(Move::normal(12, 28));
        list.push(Move::capture(12, 21));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].from_sq(), 12);
        assert_eq!(list[1].is_capture(), true);
    }

    #[test]
    fn test_mvv_lva() {
        let pxq = mvv_lva_score(4, 0); // Queen victim, Pawn attacker
        let qxp = mvv_lva_score(0, 4); // Pawn victim, Queen attacker
        assert!(pxq > qxp);
    }

    #[test]
    fn test_square_utils() {
        assert_eq!(Square::file(12), 4); // e2 -> file e (4)
        assert_eq!(Square::rank(12), 1); // e2 -> rank 2 (1)
        assert_eq!(Square::make(4, 1), 12); // e2
        assert_eq!(Square::flip(12), 52); // e2 -> e7
    }
}
