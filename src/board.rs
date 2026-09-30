use core::option::Option::None;

use crate::attacks::{BISHOP_PSEUDO, ROOK_PSEUDO, bishop_attacks, rook_attacks};
use crate::pieces::{Color, Piece, PieceType};
use crate::transposition::ZOBRIST;
use crate::types::{Move, UndoState};

#[derive(Clone)]
pub struct MoveState {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub captured: Option<Piece>,
    pub captured_sq: Option<(usize, usize)>,
    pub promotion: Option<PieceType>,
    pub prev_en_passant: Option<(usize, usize)>,
    pub prev_castling: [[bool; 2]; 2],
    pub rook_move: Option<((usize, usize), (usize, usize))>,
}

#[derive(Clone)]
pub struct Board {
    pub squares: [[Option<Piece>; 8]; 8],
    pub bitboards: [[u64; 6]; 2],
    pub white_occ: u64,
    pub black_occ: u64,
    pub hash: u64,
    pub eval_mg: i32,
    pub eval_eg: i32,
    pub eval_phase: i32,
    pub en_passant: Option<(usize, usize)>,
    pub castling: [[bool; 2]; 2],
    /// Plies since the last capture or pawn move (fifty-move rule). Counted from
    /// the position the board was set up from, where it starts at 0.
    pub halfmove: u16,
}

#[inline(always)]
pub fn color_idx(color: Color) -> usize {
    color as usize
}

#[inline(always)]
pub fn piece_index(pt: PieceType) -> usize {
    pt as usize
}

fn sq_mask(x: usize, y: usize) -> u64 {
    1u64 << (y * 8 + x)
}

impl Board {
    pub fn new() -> Self {
        Self {
            squares: [[None; 8]; 8],
            bitboards: [[0u64; 6]; 2],
            white_occ: 0,
            black_occ: 0,
            hash: 0,
            eval_mg: 0,
            eval_eg: 0,
            eval_phase: 0,
            en_passant: None,
            castling: [[true, true], [true, true]],
            halfmove: 0,
        }
    }

    pub fn setup_standard(&mut self) {
        self.halfmove = 0;
        self.white_occ = 0;
        self.black_occ = 0;
        self.hash = 0;
        self.eval_mg = 0;
        self.eval_eg = 0;
        self.eval_phase = 0;
        for y in 0..8 {
            for x in 0..8 {
                self.squares[y][x] = None;
            }
        }
        self.bitboards = [[0u64; 6]; 2];
        let back = [
            PieceType::Rook,
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Queen,
            PieceType::King,
            PieceType::Bishop,
            PieceType::Knight,
            PieceType::Rook,
        ];
        for (x, &pt) in back.iter().enumerate() {
            self.set_index(
                x,
                0,
                Some(Piece {
                    piece_type: pt,
                    color: Color::White,
                }),
            );
            self.set_index(
                x,
                7,
                Some(Piece {
                    piece_type: pt,
                    color: Color::Black,
                }),
            );
            self.set_index(
                x,
                1,
                Some(Piece {
                    piece_type: PieceType::Pawn,
                    color: Color::White,
                }),
            );
            self.set_index(
                x,
                6,
                Some(Piece {
                    piece_type: PieceType::Pawn,
                    color: Color::Black,
                }),
            );
        }
        self.en_passant = None;
        self.castling = [[true, true], [true, true]];
    }

    pub fn set_index(&mut self, x: usize, y: usize, piece: Option<Piece>) {
        let mask = sq_mask(x, y);
        if let Some(old) = self.squares[y][x] {
            let c = color_idx(old.color);
            let p = piece_index(old.piece_type);
            let (mg, eg, phase) = crate::eval::piece_eval_delta(old, y * 8 + x);
            self.eval_mg -= mg;
            self.eval_eg -= eg;
            self.eval_phase -= phase;
            self.bitboards[c][p] &= !mask;
            if old.color == Color::White {
                self.white_occ &= !mask;
            } else {
                self.black_occ &= !mask;
            }
            self.hash ^= ZOBRIST[c][p][y * 8 + x];
        }
        self.squares[y][x] = piece;
        if let Some(pce) = piece {
            let c = color_idx(pce.color);
            let p = piece_index(pce.piece_type);
            let (mg, eg, phase) = crate::eval::piece_eval_delta(pce, y * 8 + x);
            self.eval_mg += mg;
            self.eval_eg += eg;
            self.eval_phase += phase;
            self.bitboards[c][p] |= mask;
            if pce.color == Color::White {
                self.white_occ |= mask;
            } else {
                self.black_occ |= mask;
            }
            self.hash ^= ZOBRIST[c][p][y * 8 + x];
        }
    }

    pub fn get_index(&self, x: usize, y: usize) -> Option<Piece> {
        self.squares[y][x]
    }

    pub fn get(&self, pos: &str) -> Option<Piece> {
        if let Some((x, y)) = Self::algebraic_to_index(pos) {
            self.get_index(x, y)
        } else {
            None
        }
    }

    pub fn set(&mut self, pos: &str, piece: Option<Piece>) -> bool {
        if let Some((x, y)) = Self::algebraic_to_index(pos) {
            self.set_index(x, y, piece);
            true
        } else {
            false
        }
    }

    pub fn make_move_state(&mut self, start: &str, end: &str) -> Option<MoveState> {
        let (sx, sy) = Self::algebraic_to_index(start)?;
        let ((ex, ey), promotion) = Self::parse_move_destination(end)?;
        let piece = self.get_index(sx, sy)?;
        let captured = self.get_index(ex, ey);
        let mut captured_sq = if captured.is_some() {
            Some((ex, ey))
        } else {
            None
        };
        let prev_ep = self.en_passant;
        let prev_castling = self.castling;
        let mut rook_move = None;

        let cidx = color_idx(piece.color);
        match piece.piece_type {
            PieceType::King => {
                self.castling[cidx] = [false, false];
                if (sx as isize - ex as isize).abs() == 2 {
                    if ex == 6 {
                        rook_move = Some(((7, sy), (5, sy)));
                        let rook = self.get_index(7, sy);
                        self.set_index(5, sy, rook);
                        self.set_index(7, sy, None);
                    } else if ex == 2 {
                        rook_move = Some(((0, sy), (3, sy)));
                        let rook = self.get_index(0, sy);
                        self.set_index(3, sy, rook);
                        self.set_index(0, sy, None);
                    }
                }
            }
            PieceType::Rook => {
                if sx == 0 {
                    self.castling[cidx][1] = false;
                }
                if sx == 7 {
                    self.castling[cidx][0] = false;
                }
            }
            _ => {}
        }

        self.en_passant = None;
        if piece.piece_type == PieceType::Pawn {
            let dir_y: isize = if piece.color == Color::White { 1 } else { -1 };
            if (sy as isize + 2 * dir_y) as usize == ey
                && sx == ex
                && self.get_index(ex, ey).is_none()
            {
                self.en_passant = Some((sx, (sy as isize + dir_y) as usize));
            }
            if let Some((epx, epy)) = prev_ep {
                if ex == epx && ey == epy && self.get_index(ex, ey).is_none() {
                    let cap_y = if piece.color == Color::White {
                        ey - 1
                    } else {
                        ey + 1
                    };
                    let cap = self.get_index(ex, cap_y);
                    self.set_index(ex, cap_y, None);
                    self.set_index(ex, ey, Some(piece));
                    self.set_index(sx, sy, None);
                    captured_sq = Some((ex, cap_y));
                    return Some(MoveState {
                        start: (sx, sy),
                        end: (ex, ey),
                        captured: cap,
                        captured_sq,
                        promotion: None,
                        prev_en_passant: prev_ep,
                        prev_castling,
                        rook_move,
                    });
                }
            }
        }

        let moved_piece = if piece.piece_type == PieceType::Pawn && (ey == 0 || ey == 7) {
            Piece {
                piece_type: promotion.unwrap_or(PieceType::Queen),
                color: piece.color,
            }
        } else {
            piece
        };

        self.set_index(ex, ey, Some(moved_piece));
        self.set_index(sx, sy, None);

        Some(MoveState {
            start: (sx, sy),
            end: (ex, ey),
            captured,
            captured_sq,
            promotion: if moved_piece.piece_type != piece.piece_type {
                Some(moved_piece.piece_type)
            } else {
                None
            },
            prev_en_passant: prev_ep,
            prev_castling,
            rook_move,
        })
    }

    pub fn unmake_move(&mut self, state: MoveState) {
        let mut moving = self.get_index(state.end.0, state.end.1);
        if state.promotion.is_some() {
            if let Some(mut piece) = moving {
                piece.piece_type = PieceType::Pawn;
                moving = Some(piece);
            }
        }
        self.set_index(state.start.0, state.start.1, moving);
        self.set_index(state.end.0, state.end.1, None);
        if let Some((cx, cy)) = state.captured_sq {
            self.set_index(cx, cy, state.captured);
        }
        if let Some(((rsx, rsy), (rex, rey))) = state.rook_move {
            let rook = self.get_index(rex, rey);
            self.set_index(rsx, rsy, rook);
            self.set_index(rex, rey, None);
        }
        self.en_passant = state.prev_en_passant;
        self.castling = state.prev_castling;
    }

    pub fn algebraic_to_index(pos: &str) -> Option<(usize, usize)> {
        if pos.len() != 2 {
            return None;
        }
        let bytes = pos.as_bytes();
        let file = bytes[0] as char;
        let rank = bytes[1] as char;
        let x = match file {
            'a'..='h' => (file as u8 - b'a') as usize,
            _ => return None,
        };
        let y = match rank {
            '1'..='8' => (rank as u8 - b'1') as usize,
            _ => return None,
        };
        Some((x, y))
    }

    pub fn index_to_algebraic(x: usize, y: usize) -> Option<String> {
        if x < 8 && y < 8 {
            let file = (b'a' + x as u8) as char;
            let rank = (b'1' + y as u8) as char;
            Some(format!("{}{}", file, rank))
        } else {
            None
        }
    }

    fn inside(x: isize, y: isize) -> bool {
        x >= 0 && x < 8 && y >= 0 && y < 8
    }

    fn parse_move_destination(end: &str) -> Option<((usize, usize), Option<PieceType>)> {
        let bytes = end.as_bytes();
        let promotion = match bytes.len() {
            2 => None,
            3 => Some(match bytes[2].to_ascii_lowercase() {
                b'n' => PieceType::Knight,
                b'b' => PieceType::Bishop,
                b'r' => PieceType::Rook,
                b'q' => PieceType::Queen,
                _ => return None,
            }),
            _ => return None,
        };

        let square = Self::algebraic_to_index(std::str::from_utf8(&bytes[..2]).ok()?)?;
        Some((square, promotion))
    }

    pub fn encode_move(&self, start: &str, end: &str, color: Color) -> Option<Move> {
        let (sx, sy) = Self::algebraic_to_index(start)?;
        let ((ex, ey), promotion) = Self::parse_move_destination(end)?;
        let piece = self.get_index(sx, sy)?;
        if piece.color != color {
            return None;
        }
        if matches!(self.get_index(ex, ey), Some(dest) if dest.color == color) {
            return None;
        }

        let from = (sy * 8 + sx) as u8;
        let to = (ey * 8 + ex) as u8;
        let is_ep = piece.piece_type == PieceType::Pawn
            && sx != ex
            && self.get_index(ex, ey).is_none()
            && self.en_passant == Some((ex, ey));
        let is_capture = self.get_index(ex, ey).is_some() || is_ep;

        Some(match piece.piece_type {
            PieceType::Pawn if ey == 0 || ey == 7 => {
                Move::promotion(from, to, promotion.unwrap_or(PieceType::Queen), is_capture)
            }
            PieceType::Pawn if sx == ex && sy.abs_diff(ey) == 2 => {
                Move::new(from, to, Move::FLAG_DOUBLE_PUSH)
            }
            PieceType::Pawn if is_ep => Move::new(from, to, Move::FLAG_EP_CAPTURE),
            PieceType::King if sy == ey && sx.abs_diff(ex) == 2 => {
                let flag = if ex > sx {
                    Move::FLAG_KING_CASTLE
                } else {
                    Move::FLAG_QUEEN_CASTLE
                };
                Move::new(from, to, flag)
            }
            _ if is_capture => Move::capture(from, to),
            _ => Move::normal(from, to),
        })
    }

    pub fn pseudo_legal_moves(&self, pos: &str) -> Vec<String> {
        let mut moves = Vec::new();
        let (x, y) = match Self::algebraic_to_index(pos) {
            Some(v) => v,
            None => return moves,
        };
        let piece = match self.get_index(x, y) {
            Some(p) => p,
            None => return moves,
        };
        let color = piece.color;
        match piece.piece_type {
            PieceType::Knight => {
                for (dx, dy) in [
                    (-2, -1),
                    (-2, 1),
                    (-1, -2),
                    (-1, 2),
                    (1, -2),
                    (1, 2),
                    (2, -1),
                    (2, 1),
                ] {
                    let nx = x as isize + dx;
                    let ny = y as isize + dy;
                    if Self::inside(nx, ny) {
                        if let Some(tgt) = self.get_index(nx as usize, ny as usize) {
                            if tgt.color != color {
                                if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize)
                                {
                                    moves.push(s);
                                }
                            }
                        } else if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) {
                            moves.push(s);
                        }
                    }
                }
            }
            PieceType::Bishop => {
                for (dx, dy) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                    self.add_ray(x, y, dx, dy, color, &mut moves);
                }
            }
            PieceType::Rook => {
                for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                    self.add_ray(x, y, dx, dy, color, &mut moves);
                }
            }
            PieceType::Queen => {
                for (dx, dy) in [
                    (-1, -1),
                    (-1, 1),
                    (1, -1),
                    (1, 1),
                    (0, 1),
                    (0, -1),
                    (1, 0),
                    (-1, 0),
                ] {
                    self.add_ray(x, y, dx, dy, color, &mut moves);
                }
            }
            PieceType::King => {
                for (dx, dy) in [
                    (-1, -1),
                    (-1, 0),
                    (-1, 1),
                    (0, -1),
                    (0, 1),
                    (1, -1),
                    (1, 0),
                    (1, 1),
                ] {
                    let nx = x as isize + dx;
                    let ny = y as isize + dy;
                    if Self::inside(nx, ny) {
                        if let Some(tgt) = self.get_index(nx as usize, ny as usize) {
                            if tgt.color != color {
                                if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize)
                                {
                                    moves.push(s);
                                }
                            }
                        } else if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) {
                            moves.push(s);
                        }
                    }
                }
                let rank = if color == Color::White { 0 } else { 7 };
                let cidx = color_idx(color);
                if self.castling[cidx][0]
                    && self.get_index(5, rank).is_none()
                    && self.get_index(6, rank).is_none()
                {
                    if let Some(s) = Self::index_to_algebraic(6, rank) {
                        moves.push(s);
                    }
                }
                if self.castling[cidx][1]
                    && self.get_index(1, rank).is_none()
                    && self.get_index(2, rank).is_none()
                    && self.get_index(3, rank).is_none()
                {
                    if let Some(s) = Self::index_to_algebraic(2, rank) {
                        moves.push(s);
                    }
                }
            }
            PieceType::Pawn => {
                let dir_y: isize = if color == Color::White { 1 } else { -1 };
                let start_rank: usize = if color == Color::White { 1 } else { 6 };
                let ny = y as isize + dir_y;
                if Self::inside(x as isize, ny) && self.get_index(x, ny as usize).is_none() {
                    if let Some(s) = Self::index_to_algebraic(x, ny as usize) {
                        moves.push(s);
                    }
                    if y == start_rank {
                        let ny2 = y as isize + 2 * dir_y;
                        if Self::inside(x as isize, ny2)
                            && self.get_index(x, ny2 as usize).is_none()
                        {
                            if let Some(s) = Self::index_to_algebraic(x, ny2 as usize) {
                                moves.push(s);
                            }
                        }
                    }
                }
                for dx in [-1, 1] {
                    let nx = x as isize + dx;
                    if Self::inside(nx, ny) {
                        if let Some(tgt) = self.get_index(nx as usize, ny as usize) {
                            if tgt.color != color {
                                if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize)
                                {
                                    moves.push(s);
                                }
                            }
                        } else if let Some((epx, epy)) = self.en_passant {
                            if epx as isize == nx && epy as isize == ny {
                                if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize)
                                {
                                    moves.push(s);
                                }
                            }
                        }
                    }
                }
            }
        }
        moves
    }

    fn add_ray(
        &self,
        x: usize,
        y: usize,
        dx: isize,
        dy: isize,
        color: Color,
        acc: &mut Vec<String>,
    ) {
        let mut nx = x as isize + dx;
        let mut ny = y as isize + dy;
        while Self::inside(nx, ny) {
            match self.get_index(nx as usize, ny as usize) {
                None => {
                    if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) {
                        acc.push(s);
                    }
                }
                Some(p) => {
                    if p.color != color {
                        if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) {
                            acc.push(s);
                        }
                    }
                    break;
                }
            }
            nx += dx;
            ny += dy;
        }
    }

    pub fn square_attacked(&mut self, x: usize, y: usize, by_color: Color) -> bool {
        self.is_square_attacked_by((y * 8 + x) as u8, by_color)
    }

    pub fn in_check(&mut self, color: Color) -> bool {
        self.in_check_fast(color)
    }

    pub fn find_king(&self, color: Color) -> Option<(usize, usize)> {
        for y in 0..8 {
            for x in 0..8 {
                if let Some(p) = self.get_index(x, y) {
                    if p.piece_type == PieceType::King && p.color == color {
                        return Some((x, y));
                    }
                }
            }
        }
        None
    }

    pub fn is_legal(&mut self, start: &str, end: &str, color: Color) -> bool {
        let Some(mv) = self.encode_move(start, end, color) else {
            return false;
        };
        self.is_move_legal_fast(mv, color)
    }

    pub fn all_legal_moves(&mut self, color: Color) -> Vec<(String, String)> {
        let mut res = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                if let Some(pos) = Self::index_to_algebraic(x, y) {
                    if let Some(p) = self.get_index(x, y) {
                        if p.color == color {
                            for m in self.pseudo_legal_moves(&pos) {
                                if self.is_legal(&pos, &m, color) {
                                    res.push((pos.clone(), m));
                                }
                            }
                        }
                    }
                }
            }
        }
        res
    }

    pub fn all_legal_moves_fast(&mut self, color: Color) -> Vec<(String, String)> {
        crate::movegen::generate_moves(self, color)
    }

    #[inline]
    pub fn is_move_legal_fast(&mut self, mv: Move, color: Color) -> bool {
        crate::movegen::is_move_pseudo_legal(self, mv, color)
            && self.is_generated_move_legal(mv, color)
    }

    #[inline]
    pub fn is_generated_move_legal(&mut self, mv: Move, color: Color) -> bool {
        let from_sq = mv.from_sq();
        let from_x = (from_sq % 8) as usize;
        let from_y = (from_sq / 8) as usize;

        if mv.is_castle() {
            let cidx = color_idx(color);
            let rank = if color == Color::White { 0 } else { 7 };
            if from_x != 4 || from_y != rank {
                return false;
            }

            let (rook_x, transit_sq, side) = if mv.flags() == Move::FLAG_KING_CASTLE {
                (7, rank * 8 + 5, 0usize)
            } else {
                (0, rank * 8 + 3, 1usize)
            };

            if !self.castling[cidx][side] {
                return false;
            }

            if !matches!(
                self.get_index(rook_x, rank),
                Some(Piece {
                    piece_type: PieceType::Rook,
                    color: rook_color
                }) if rook_color == color
            ) {
                return false;
            }

            let opp = if color == Color::White {
                Color::Black
            } else {
                Color::White
            };
            if self.in_check_fast(color) || self.is_square_attacked_by(transit_sq as u8, opp) {
                return false;
            }
        }

        let undo = self.make_move_fast(mv, color);
        let legal = !self.in_check_fast(color);
        self.unmake_move_fast(undo, color);
        legal
    }

    pub fn capture_moves(&mut self, color: Color) -> Vec<(String, String)> {
        let mut res = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                if let Some(pos) = Self::index_to_algebraic(x, y) {
                    if let Some(p) = self.get_index(x, y) {
                        if p.color == color {
                            for m in self.pseudo_legal_moves(&pos) {
                                if let Some((mx, my)) = Self::algebraic_to_index(&m) {
                                    if self.get_index(mx, my).is_some() {
                                        if self.is_legal(&pos, &m, color) {
                                            res.push((pos.clone(), m));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        res
    }

    pub fn capture_moves_fast(&mut self, color: Color) -> Vec<(String, String)> {
        self.all_legal_moves_fast(color)
            .into_iter()
            .filter(|(_, e)| {
                if let Some((ex, ey)) = Board::algebraic_to_index(e) {
                    self.get_index(ex, ey).is_some()
                } else {
                    false
                }
            })
            .collect()
    }

    pub fn piece_count(&self, piece_type: PieceType) -> usize {
        let mut c = 0;
        for y in 0..8 {
            for x in 0..8 {
                if let Some(p) = self.get_index(x, y) {
                    if p.piece_type == piece_type {
                        c += 1;
                    }
                }
            }
        }
        c
    }

    pub fn piece_count_color(&self, piece_type: PieceType, color: Color) -> usize {
        let cidx = color_idx(color);
        let mut count = 0;
        let mut bb = self.bitboards[cidx][piece_index(piece_type)];
        while bb != 0 {
            count += 1;
            bb &= bb - 1;
        }
        count
    }

    pub fn piece_count_total(&self, color: Color) -> usize {
        if color == Color::White {
            self.white_occ.count_ones() as usize
        } else {
            self.black_occ.count_ones() as usize
        }
    }

    /// Whether `color` has at least one knight, bishop, rook or queen. Used to
    /// disable null-move pruning in king-and-pawn endings (zugzwang).
    #[inline(always)]
    /// Whether `color` has any legal move (early exit, see `movegen::has_legal_move`).
    pub fn has_legal_move(&mut self, color: Color) -> bool {
        crate::movegen::has_legal_move(self, color)
    }

    pub fn has_non_pawn_material(&self, color: Color) -> bool {
        let bb = &self.bitboards[color_idx(color)];
        (bb[piece_index(PieceType::Knight)]
            | bb[piece_index(PieceType::Bishop)]
            | bb[piece_index(PieceType::Rook)]
            | bb[piece_index(PieceType::Queen)])
            != 0
    }

    pub fn piece_count_all(&self) -> usize {
        (self.white_occ | self.black_occ).count_ones() as usize
    }

    pub fn to_fen(&self, turn: Color) -> String {
        let mut fen = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                if let Some(piece) = self.get_index(file, rank) {
                    if empty > 0 {
                        fen.push_str(&empty.to_string());
                        empty = 0;
                    }
                    let mut ch = match piece.piece_type {
                        PieceType::Pawn => 'p',
                        PieceType::Knight => 'n',
                        PieceType::Bishop => 'b',
                        PieceType::Rook => 'r',
                        PieceType::Queen => 'q',
                        PieceType::King => 'k',
                    };
                    if piece.color == Color::White {
                        ch = ch.to_ascii_uppercase();
                    }
                    fen.push(ch);
                } else {
                    empty += 1;
                }
            }
            if empty > 0 {
                fen.push_str(&empty.to_string());
            }
            if rank > 0 {
                fen.push('/');
            }
        }
        fen.push(' ');
        fen.push(if turn == Color::White { 'w' } else { 'b' });
        fen.push(' ');
        let mut castle = String::new();
        if self.castling[0][0] {
            castle.push('K');
        }
        if self.castling[0][1] {
            castle.push('Q');
        }
        if self.castling[1][0] {
            castle.push('k');
        }
        if self.castling[1][1] {
            castle.push('q');
        }
        if castle.is_empty() {
            castle.push('-');
        }
        fen.push_str(&castle);
        fen.push(' ');
        if let Some((x, y)) = self.en_passant {
            if let Some(ep) = Self::index_to_algebraic(x, y) {
                fen.push_str(&ep);
            } else {
                fen.push('-');
            }
        } else {
            fen.push('-');
        }
        fen.push_str(" 0 1");
        fen
    }

    /// Removes `piece` from `sq`, updating bitboards, occupancy, eval and hash.
    #[inline(always)]
    fn remove_piece(&mut self, sq: usize, piece: Piece) {
        let sq = sq & 63;
        let (mg, eg, phase) = crate::eval::piece_eval_delta(piece, sq);
        self.eval_mg -= mg;
        self.eval_eg -= eg;
        self.eval_phase -= phase;
        self.hash ^= ZOBRIST[color_idx(piece.color)][piece_index(piece.piece_type)][sq];
        self.remove_raw(sq, piece);
    }

    /// Puts `piece` on the empty square `sq`, updating eval and hash too.
    #[inline(always)]
    fn add_piece(&mut self, sq: usize, piece: Piece) {
        let sq = sq & 63;
        let (mg, eg, phase) = crate::eval::piece_eval_delta(piece, sq);
        self.eval_mg += mg;
        self.eval_eg += eg;
        self.eval_phase += phase;
        self.hash ^= ZOBRIST[color_idx(piece.color)][piece_index(piece.piece_type)][sq];
        self.put_raw(sq, piece);
    }

    /// Mailbox/bitboard/occupancy-only removal (no eval or hash update).
    #[inline(always)]
    fn remove_raw(&mut self, sq: usize, piece: Piece) {
        let sq = sq & 63;
        let mask = 1u64 << sq;
        self.squares[sq >> 3][sq & 7] = None;
        self.bitboards[color_idx(piece.color)][piece_index(piece.piece_type)] &= !mask;
        if piece.color == Color::White {
            self.white_occ &= !mask;
        } else {
            self.black_occ &= !mask;
        }
    }

    /// Mailbox/bitboard/occupancy-only placement on an empty square.
    #[inline(always)]
    fn put_raw(&mut self, sq: usize, piece: Piece) {
        let sq = sq & 63;
        let mask = 1u64 << sq;
        self.squares[sq >> 3][sq & 7] = Some(piece);
        self.bitboards[color_idx(piece.color)][piece_index(piece.piece_type)] |= mask;
        if piece.color == Color::White {
            self.white_occ |= mask;
        } else {
            self.black_occ |= mask;
        }
    }

    /// `set_index` without eval or hash maintenance.
    #[inline(always)]
    fn set_raw(&mut self, sq: usize, piece: Option<Piece>) {
        let sq = sq & 63;
        if let Some(old) = self.squares[sq >> 3][sq & 7] {
            self.remove_raw(sq, old);
        }
        if let Some(pce) = piece {
            self.put_raw(sq, pce);
        }
    }

    #[inline]
    pub fn make_move_fast(&mut self, mv: Move, color: Color) -> UndoState {
        let from_sq = mv.from_sq();
        let to_sq = mv.to_sq();
        let from = (from_sq & 63) as usize;
        let to = (to_sq & 63) as usize;
        let from_x = from & 7;
        let from_y = from >> 3;
        let to_x = to & 7;
        let to_y = to >> 3;

        let piece = self.squares[from_y][from_x].unwrap();
        let captured = self.squares[to_y][to_x];
        let halfmove = self.halfmove;
        self.halfmove = if piece.piece_type == PieceType::Pawn || captured.is_some() {
            0
        } else {
            halfmove.saturating_add(1)
        };

        let prev_ep = self
            .en_passant
            .map(|(x, y)| (y * 8 + x) as u8)
            .unwrap_or(UndoState::NO_EP);
        let prev_castling = self.pack_castling();
        let undo_base = UndoState {
            mv,
            captured: UndoState::NO_CAPTURE,
            captured_sq: to_sq,
            prev_ep,
            prev_castling,
            prev_hash: self.hash,
            prev_eval_mg: self.eval_mg,
            prev_eval_eg: self.eval_eg,
            prev_eval_phase: self.eval_phase,
            prev_halfmove: halfmove,
        };

        let mut captured_piece_idx = UndoState::NO_CAPTURE;
        let mut captured_sq = to_sq;

        if mv.is_ep() {
            let cap_y = if color == Color::White {
                to_y - 1
            } else {
                to_y + 1
            };
            captured_sq = (cap_y * 8 + to_x) as u8;
            let cap_piece = self.squares[cap_y][to_x].unwrap();
            captured_piece_idx = piece_index(cap_piece.piece_type) as u8;
            self.remove_piece(captured_sq as usize, cap_piece);
        } else if let Some(cap) = captured {
            captured_piece_idx = piece_index(cap.piece_type) as u8;
        }

        let cidx = color_idx(color);
        match piece.piece_type {
            PieceType::King => {
                self.castling[cidx] = [false, false];
            }
            PieceType::Rook => {
                if from_x == 0 {
                    self.castling[cidx][1] = false; // Queen-side
                }
                if from_x == 7 {
                    self.castling[cidx][0] = false; // King-side
                }
            }
            _ => {}
        }

        if captured.is_some() {
            let opp = 1 - cidx;
            let opp_rank = if opp == 0 { 0 } else { 7 };
            if to_y == opp_rank {
                if to_x == 0 {
                    self.castling[opp][1] = false;
                } else if to_x == 7 {
                    self.castling[opp][0] = false;
                }
            }
        }

        self.en_passant = None;

        if mv.is_castle() {
            let rank = from_y;
            if mv.flags() == Move::FLAG_KING_CASTLE {
                let rook = self.get_index(7, rank);
                self.set_index(5, rank, rook);
                self.set_index(7, rank, None);
            } else {
                let rook = self.get_index(0, rank);
                self.set_index(3, rank, rook);
                self.set_index(0, rank, None);
            }
        }

        if mv.is_double_push() {
            let ep_y = if color == Color::White {
                from_y + 1
            } else {
                from_y - 1
            };
            self.en_passant = Some((from_x, ep_y));
        }

        let moving_piece = if let Some(promo_type) = mv.promotion_piece() {
            Piece {
                piece_type: promo_type,
                color,
            }
        } else {
            piece
        };

        // Same net effect as set_index(to, moving) followed by set_index(from, None).
        if let Some(cap) = self.squares[to_y][to_x] {
            self.remove_piece(to, cap);
        }
        self.add_piece(to, moving_piece);
        self.remove_piece(from, piece);

        UndoState {
            captured: captured_piece_idx,
            captured_sq,
            ..undo_base
        }
    }

    #[inline]
    pub fn unmake_move_fast(&mut self, state: UndoState, color: Color) {
        let mv = state.mv;
        let from = (mv.from_sq() & 63) as usize;
        let to = (mv.to_sq() & 63) as usize;

        let on_target = self.squares[to >> 3][to & 7].unwrap();
        let mut moving_piece = on_target;
        if mv.is_promotion() {
            moving_piece.piece_type = PieceType::Pawn;
        }

        self.set_raw(from, Some(moving_piece));
        self.remove_raw(to, on_target);

        if state.has_capture() {
            let opp_color = if color == Color::White {
                Color::Black
            } else {
                Color::White
            };
            let cap_type = Self::piece_type_from_idx(state.captured as usize);
            self.set_raw(
                state.captured_sq as usize,
                Some(Piece {
                    piece_type: cap_type,
                    color: opp_color,
                }),
            );
        }

        if mv.is_castle() {
            let base = from & !7;
            if mv.flags() == Move::FLAG_KING_CASTLE {
                let rook = self.squares[base >> 3][5];
                self.set_raw(base + 7, rook);
                self.set_raw(base + 5, None);
            } else {
                let rook = self.squares[base >> 3][3];
                self.set_raw(base, rook);
                self.set_raw(base + 3, None);
            }
        }

        self.en_passant = if state.prev_ep == UndoState::NO_EP {
            None
        } else {
            Some(((state.prev_ep % 8) as usize, (state.prev_ep / 8) as usize))
        };

        self.unpack_castling(state.prev_castling);

        self.hash = state.prev_hash;
        self.eval_mg = state.prev_eval_mg;
        self.eval_eg = state.prev_eval_eg;
        self.eval_phase = state.prev_eval_phase;
        self.halfmove = state.prev_halfmove;
    }

    pub fn recompute_eval_state(&mut self) {
        self.white_occ = 0;
        self.black_occ = 0;
        self.eval_mg = 0;
        self.eval_eg = 0;
        self.eval_phase = 0;

        for y in 0..8 {
            for x in 0..8 {
                if let Some(piece) = self.squares[y][x] {
                    let mask = sq_mask(x, y);
                    let (mg, eg, phase) = crate::eval::piece_eval_delta(piece, y * 8 + x);
                    self.eval_mg += mg;
                    self.eval_eg += eg;
                    self.eval_phase += phase;
                    if piece.color == Color::White {
                        self.white_occ |= mask;
                    } else {
                        self.black_occ |= mask;
                    }
                }
            }
        }
    }

    #[inline(always)]
    fn pack_castling(&self) -> u8 {
        let mut c = 0u8;
        if self.castling[0][0] {
            c |= 1;
        } // White king-side
        if self.castling[0][1] {
            c |= 2;
        } // White queen-side
        if self.castling[1][0] {
            c |= 4;
        } // Black king-side
        if self.castling[1][1] {
            c |= 8;
        } // Black queen-side
        c
    }

    #[inline(always)]
    fn unpack_castling(&mut self, c: u8) {
        self.castling[0][0] = (c & 1) != 0;
        self.castling[0][1] = (c & 2) != 0;
        self.castling[1][0] = (c & 4) != 0;
        self.castling[1][1] = (c & 8) != 0;
    }

    #[inline(always)]
    fn piece_type_from_idx(idx: usize) -> PieceType {
        const TYPES: [PieceType; 6] = [
            PieceType::Pawn,
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Rook,
            PieceType::Queen,
            PieceType::King,
        ];
        TYPES[idx.min(5)]
    }

    #[inline(always)]
    pub fn all_pieces(&self, color: Color) -> u64 {
        if color == Color::White {
            self.white_occ
        } else {
            self.black_occ
        }
    }

    #[inline(always)]
    pub fn occupied(&self) -> u64 {
        self.white_occ | self.black_occ
    }

    #[inline]
    pub fn is_square_attacked_by(&self, sq: u8, by_color: Color) -> bool {
        self.is_square_attacked_by_occ(sq, by_color, self.occupied())
    }

    /// Like `is_square_attacked_by`, but slider attacks use `occ` as occupancy
    /// (e.g. with the moving king removed).
    #[inline]
    pub fn is_square_attacked_by_occ(&self, sq: u8, by_color: Color, occ: u64) -> bool {
        let sq = (sq & 63) as usize;
        let by = &self.bitboards[color_idx(by_color)];

        if (crate::movegen::pawn_attackers_to_square(sq, by_color) & by[0]) != 0 {
            return true;
        }
        if (crate::movegen::KNIGHT_TABLE[sq] & by[1]) != 0 {
            return true;
        }
        if (crate::movegen::KING_TABLE[sq] & by[5]) != 0 {
            return true;
        }

        let bishops_queens = by[2] | by[4];
        if (BISHOP_PSEUDO[sq] & bishops_queens) != 0
            && (bishop_attacks(sq, occ) & bishops_queens) != 0
        {
            return true;
        }

        let rooks_queens = by[3] | by[4];
        (ROOK_PSEUDO[sq] & rooks_queens) != 0 && (rook_attacks(sq, occ) & rooks_queens) != 0
    }

    #[inline]
    pub fn in_check_fast(&self, color: Color) -> bool {
        let cidx = color_idx(color);
        let king_bb = self.bitboards[cidx][5];
        if king_bb == 0 {
            return false;
        }
        let king_sq = king_bb.trailing_zeros() as u8;
        let opp = if color == Color::White {
            Color::Black
        } else {
            Color::White
        };
        self.is_square_attacked_by(king_sq, opp)
    }

    /// Whether `mv` (a legal move of `color`) leaves the opponent in check,
    /// computed before making it. Handles direct and discovered checks,
    /// promotions, en passant and castling (the rook gives the check).
    #[inline]
    pub fn gives_check(&self, mv: Move, color: Color) -> bool {
        let them = if color == Color::White {
            Color::Black
        } else {
            Color::White
        };
        let king_bb = self.bitboards[color_idx(them)][5];
        if king_bb == 0 {
            return false;
        }
        let ksq = king_bb.trailing_zeros() as usize;

        let from = (mv.from_sq() & 63) as usize;
        let to = (mv.to_sq() & 63) as usize;
        let from_bb = 1u64 << from;
        let to_bb = 1u64 << to;

        let mut pieces = self.bitboards[color_idx(color)];
        let Some(moved) = (0..6).find(|&pt| pieces[pt] & from_bb != 0) else {
            return false;
        };
        let landed = mv.promotion_piece().map_or(moved, piece_index);
        pieces[moved] &= !from_bb;
        pieces[landed] |= to_bb;

        let mut occ = (self.occupied() & !from_bb) | to_bb;
        if mv.is_ep() {
            let captured = if color == Color::White {
                to - 8
            } else {
                to + 8
            };
            occ &= !(1u64 << captured);
        } else if mv.is_castle() {
            let (rook_from, rook_to) = if mv.flags() == Move::FLAG_KING_CASTLE {
                (from + 3, from + 1)
            } else {
                (from - 4, from - 1)
            };
            let rook_move = (1u64 << rook_from) | (1u64 << rook_to);
            pieces[3] ^= rook_move;
            occ = (occ & !(1u64 << rook_from)) | (1u64 << rook_to);
        }

        (crate::movegen::pawn_attackers_to_square(ksq, color) & pieces[0]) != 0
            || (crate::movegen::KNIGHT_TABLE[ksq] & pieces[1]) != 0
            || (bishop_attacks(ksq, occ) & (pieces[2] | pieces[4])) != 0
            || (rook_attacks(ksq, occ) & (pieces[3] | pieces[4])) != 0
    }

    #[inline(always)]
    pub fn piece_at_sq(&self, sq: u8) -> Option<(PieceType, Color)> {
        let x = (sq % 8) as usize;
        let y = (sq / 8) as usize;
        self.get_index(x, y).map(|p| (p.piece_type, p.color))
    }

    #[inline(always)]
    pub fn piece_type_idx_at(&self, sq: u8) -> usize {
        let x = (sq % 8) as usize;
        let y = (sq / 8) as usize;
        match self.get_index(x, y) {
            Some(p) => piece_index(p.piece_type),
            None => 6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_board() -> Board {
        let mut board = Board::new();
        board.setup_standard();
        board
    }

    #[test]
    fn test_make_unmake_normal_move() {
        let mut board = setup_board();
        let original_hash = board.hash;

        let state = board.make_move_state("e2", "e4").unwrap();
        assert!(board.get("e2").is_none());
        assert!(board.get("e4").is_some());

        board.unmake_move(state);
        assert!(board.get("e2").is_some());
        assert!(board.get("e4").is_none());
        assert_eq!(
            board.hash, original_hash,
            "Zobrist hash not restored after unmake"
        );
    }

    #[test]
    fn test_make_unmake_capture() {
        let mut board = setup_board();

        board.make_move_state("e2", "e4");
        board.make_move_state("d7", "d5");
        let hash_before_capture = board.hash;

        let state = board.make_move_state("e4", "d5").unwrap();
        assert!(state.captured.is_some());
        assert_eq!(state.captured.unwrap().piece_type, PieceType::Pawn);

        board.unmake_move(state);
        assert!(board.get("e4").is_some());
        assert!(board.get("d5").is_some()); // Black pawn restored
        assert_eq!(
            board.hash, hash_before_capture,
            "Zobrist hash not restored after capture unmake"
        );
    }

    #[test]
    fn test_make_unmake_kingside_castling() {
        let mut board = setup_board();

        board.set("f1", None);
        board.set("g1", None);

        let _original_hash = board.hash;
        let original_castling = board.castling;

        let state = board.make_move_state("e1", "g1").unwrap();

        assert!(board.get("e1").is_none());
        assert!(board.get("g1").is_some());
        assert_eq!(board.get("g1").unwrap().piece_type, PieceType::King);
        assert!(board.get("f1").is_some());
        assert_eq!(board.get("f1").unwrap().piece_type, PieceType::Rook);
        assert!(board.get("h1").is_none());

        board.unmake_move(state);
        assert!(board.get("e1").is_some());
        assert!(board.get("h1").is_some());
        assert!(board.get("f1").is_none());
        assert!(board.get("g1").is_none());
        assert_eq!(
            board.castling, original_castling,
            "Castling rights not restored"
        );
    }

    #[test]
    fn test_make_unmake_queenside_castling() {
        let mut board = setup_board();

        board.set("b1", None);
        board.set("c1", None);
        board.set("d1", None);

        let state = board.make_move_state("e1", "c1").unwrap();

        assert!(board.get("c1").is_some());
        assert_eq!(board.get("c1").unwrap().piece_type, PieceType::King);
        assert!(board.get("d1").is_some());
        assert_eq!(board.get("d1").unwrap().piece_type, PieceType::Rook);

        board.unmake_move(state);
        assert!(board.get("e1").is_some());
        assert!(board.get("a1").is_some());
    }

    #[test]
    fn test_make_unmake_en_passant() {
        let mut board = setup_board();

        board.make_move_state("e2", "e4");
        board.make_move_state("a7", "a6"); // Black move
        board.make_move_state("e4", "e5");

        board.make_move_state("d7", "d5");

        let _hash_before_ep = board.hash;

        let state = board.make_move_state("e5", "d6").unwrap();

        assert!(board.get("d5").is_none());
        assert!(board.get("d6").is_some());
        assert_eq!(board.get("d6").unwrap().color, Color::White);

        board.unmake_move(state);
        assert!(board.get("e5").is_some());
        assert!(board.get("d5").is_some()); // Black pawn restored
        assert!(board.get("d6").is_none());
    }

    #[test]
    fn test_has_non_pawn_material() {
        let mut board = Board::new();
        let put = |board: &mut Board, sq: &str, piece_type: PieceType, color: Color| {
            board.set(sq, Some(Piece { piece_type, color }));
        };
        put(&mut board, "e1", PieceType::King, Color::White);
        put(&mut board, "e8", PieceType::King, Color::Black);
        for sq in ["a2", "b2", "c2", "d2"] {
            put(&mut board, sq, PieceType::Pawn, Color::White);
        }
        // King + pawns only: no non-pawn material, whatever the piece count.
        assert!(!board.has_non_pawn_material(Color::White));
        assert!(!board.has_non_pawn_material(Color::Black));

        for (sq, piece_type) in [
            ("b8", PieceType::Knight),
            ("c8", PieceType::Bishop),
            ("a8", PieceType::Rook),
            ("d8", PieceType::Queen),
        ] {
            put(&mut board, sq, piece_type, Color::Black);
            assert!(board.has_non_pawn_material(Color::Black), "{piece_type:?}");
            assert!(!board.has_non_pawn_material(Color::White));
            board.set(sq, None);
        }
        assert!(!board.has_non_pawn_material(Color::Black));
        assert!(setup_board().has_non_pawn_material(Color::White));
    }

    #[test]
    fn test_make_unmake_promotion() {
        let mut board = Board::new();

        board.set(
            "e7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "h8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );

        let _original_hash = board.hash;

        let state = board.make_move_state("e7", "e8q").unwrap();

        let piece = board.get("e8").unwrap();
        assert_eq!(piece.color, Color::White);
        assert_eq!(piece.piece_type, PieceType::Queen);
        assert!(board.get("e7").is_none());

        board.unmake_move(state);
        let pawn = board.get("e7").unwrap();
        assert_eq!(pawn.piece_type, PieceType::Pawn);
        assert!(board.get("e8").is_none());
    }

    #[test]
    fn test_make_move_state_supports_underpromotion_suffix() {
        let mut board = Board::new();
        board.set(
            "e7",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "h8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );

        let state = board.make_move_state("e7", "e8n").unwrap();
        assert_eq!(board.get("e8").unwrap().piece_type, PieceType::Knight);
        board.unmake_move(state);
        assert_eq!(board.get("e7").unwrap().piece_type, PieceType::Pawn);
    }

    #[test]
    fn test_zobrist_hash_consistency() {
        let mut board = setup_board();
        let original_hash = board.hash;

        let s1 = board.make_move_state("e2", "e4").unwrap();
        let h1 = board.hash;
        let s2 = board.make_move_state("e7", "e5").unwrap();
        let h2 = board.hash;
        let s3 = board.make_move_state("g1", "f3").unwrap();
        let h3 = board.hash;

        assert_ne!(original_hash, h1);
        assert_ne!(h1, h2);
        assert_ne!(h2, h3);

        board.unmake_move(s3);
        assert_eq!(board.hash, h2);
        board.unmake_move(s2);
        assert_eq!(board.hash, h1);
        board.unmake_move(s1);
        assert_eq!(board.hash, original_hash);
    }

    #[test]
    fn test_full_hash_distinguishes_castling_rights() {
        let mut board = Board::new();
        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "h1",
            Some(Piece {
                piece_type: PieceType::Rook,
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
        board.castling = [[true, false], [false, false]];

        let mut without_rights = board.clone();
        without_rights.castling = [[false, false], [false, false]];

        assert_ne!(board.hash(Color::White), without_rights.hash(Color::White));
    }

    #[test]
    fn test_full_hash_ignores_non_capturable_en_passant() {
        let mut board = Board::new();
        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "a8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "e4",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.castling = [[false, false], [false, false]];

        let without_ep = board.hash(Color::Black);
        board.en_passant = Some((4, 2)); // e3, but Black has no adjacent pawn to capture it

        assert_eq!(board.hash(Color::Black), without_ep);
    }

    #[test]
    fn test_full_hash_distinguishes_capturable_en_passant() {
        let mut board = Board::new();
        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "a8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "e4",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "d4",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.castling = [[false, false], [false, false]];

        let without_ep = board.hash(Color::Black);
        board.en_passant = Some((4, 2)); // e3, capturable by the black pawn on d4

        assert_ne!(board.hash(Color::Black), without_ep);
    }

    #[test]
    fn test_recompute_hash_preserves_full_position_hash() {
        let mut board = Board::new();
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
            "e4",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "d4",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.set(
            "h1",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::White,
            }),
        );
        board.castling = [[true, false], [false, false]];
        board.en_passant = Some((4, 2)); // e3

        let full_hash = board.hash(Color::Black);
        board.hash = 0;
        board.recompute_hash();

        assert_eq!(board.hash(Color::Black), full_hash);
    }

    #[test]
    fn test_in_check_detection() {
        let mut board = Board::new();

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
                piece_type: PieceType::Queen,
                color: Color::Black,
            }),
        );
        board.set(
            "h8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );

        assert!(board.in_check(Color::White));
        assert!(!board.in_check(Color::Black));
    }

    #[test]
    fn test_is_legal_blocks_king_in_check() {
        let mut board = Board::new();

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
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "d2",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "h8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );

        assert!(!board.is_legal("d2", "d3", Color::White));

        assert!(board.is_legal("e1", "d1", Color::White));
    }

    #[test]
    fn test_is_legal_rejects_castle_through_check() {
        let mut board = Board::new();

        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "h1",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::White,
            }),
        );
        board.set(
            "a8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "f8",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );

        assert!(!board.is_legal("e1", "g1", Color::White));
    }

    #[test]
    fn test_to_fen_starting_position() {
        let mut board = Board::new();
        board.setup_standard();

        let fen = board.to_fen(Color::White);
        assert!(fen.starts_with("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR"));
    }

    #[test]
    fn test_piece_count() {
        let mut board = Board::new();
        board.setup_standard();

        assert_eq!(board.piece_count(PieceType::Pawn), 16);
        assert_eq!(board.piece_count(PieceType::Knight), 4);
        assert_eq!(board.piece_count(PieceType::Bishop), 4);
        assert_eq!(board.piece_count(PieceType::Rook), 4);
        assert_eq!(board.piece_count(PieceType::Queen), 2);
        assert_eq!(board.piece_count(PieceType::King), 2);
        assert_eq!(board.piece_count_all(), 32);
    }

    #[test]
    fn test_make_move_fast_and_unmake() {
        let mut board = setup_board();
        let original_hash = board.hash;

        let mv = Move::new(12, 28, Move::FLAG_DOUBLE_PUSH); // e2=12, e4=28

        let undo = board.make_move_fast(mv, Color::White);
        assert!(board.get("e2").is_none());
        assert!(board.get("e4").is_some());

        board.unmake_move_fast(undo, Color::White);
        assert!(board.get("e2").is_some());
        assert!(board.get("e4").is_none());
        assert_eq!(board.hash, original_hash);
    }

    #[test]
    fn test_full_hash_restored_after_fast_unmake() {
        let mut board = setup_board();
        let original_full_hash = board.hash(Color::White);

        let mv = Move::new(12, 28, Move::FLAG_DOUBLE_PUSH); // e2=12, e4=28
        let undo = board.make_move_fast(mv, Color::White);

        board.unmake_move_fast(undo, Color::White);

        assert_eq!(board.hash(Color::White), original_full_hash);
    }

    #[test]
    fn test_eval_state_restored_after_fast_unmake() {
        let mut board = Board::new();
        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "a1",
            Some(Piece {
                piece_type: PieceType::Rook,
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
            "a8",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );

        let original = (
            board.white_occ,
            board.black_occ,
            board.eval_mg,
            board.eval_eg,
            board.eval_phase,
        );
        let mv = Move::capture(0, 56); // a1xa8
        let undo = board.make_move_fast(mv, Color::White);

        let mut recomputed = board.clone();
        recomputed.recompute_eval_state();
        assert_eq!(
            (
                board.white_occ,
                board.black_occ,
                board.eval_mg,
                board.eval_eg,
                board.eval_phase,
            ),
            (
                recomputed.white_occ,
                recomputed.black_occ,
                recomputed.eval_mg,
                recomputed.eval_eg,
                recomputed.eval_phase
            )
        );
        assert_ne!(
            (
                board.white_occ,
                board.black_occ,
                board.eval_mg,
                board.eval_eg,
                board.eval_phase,
            ),
            original
        );

        board.unmake_move_fast(undo, Color::White);
        assert_eq!(
            (
                board.white_occ,
                board.black_occ,
                board.eval_mg,
                board.eval_eg,
                board.eval_phase,
            ),
            original
        );

        let mut restored = board.clone();
        restored.recompute_eval_state();
        assert_eq!(
            (
                board.white_occ,
                board.black_occ,
                board.eval_mg,
                board.eval_eg,
                board.eval_phase,
            ),
            (
                restored.white_occ,
                restored.black_occ,
                restored.eval_mg,
                restored.eval_eg,
                restored.eval_phase,
            )
        );
    }

    #[test]
    fn test_is_move_legal_fast_rejects_en_passant_discovered_check() {
        let mut board = Board::new();

        board.set(
            "e1",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::White,
            }),
        );
        board.set(
            "e5",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::White,
            }),
        );
        board.set(
            "a8",
            Some(Piece {
                piece_type: PieceType::King,
                color: Color::Black,
            }),
        );
        board.set(
            "e8",
            Some(Piece {
                piece_type: PieceType::Rook,
                color: Color::Black,
            }),
        );
        board.set(
            "d5",
            Some(Piece {
                piece_type: PieceType::Pawn,
                color: Color::Black,
            }),
        );
        board.en_passant = Some((3, 5)); // d6

        assert!(!board.is_move_legal_fast(Move::new(36, 43, Move::FLAG_EP_CAPTURE), Color::White));
    }
}
