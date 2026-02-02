use core::option::Option::None;

use crate::pieces::{Color, Piece, PieceType};
use crate::transposition::ZOBRIST;
use crate::types::{Move, UndoState};

#[derive(Clone)]
pub struct MoveState {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub captured: Option<Piece>,
    pub captured_sq: Option<(usize, usize)>,
    pub prev_en_passant: Option<(usize, usize)>,
    pub prev_castling: [[bool; 2]; 2],
    pub rook_move: Option<((usize, usize), (usize, usize))>,
}

#[derive(Clone)]
pub struct Board {
    pub squares: [[Option<Piece>; 8]; 8],
    pub bitboards: [[u64; 6]; 2],
    pub hash: u64,
    pub en_passant: Option<(usize, usize)>,
    pub castling: [[bool; 2]; 2],
}

pub fn color_idx(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

pub fn piece_index(pt: PieceType) -> usize {
    match pt {
        PieceType::Pawn => 0,
        PieceType::Knight => 1,
        PieceType::Bishop => 2,
        PieceType::Rook => 3,
        PieceType::Queen => 4,
        PieceType::King => 5,
    }
}

fn sq_mask(x: usize, y: usize) -> u64 {
    1u64 << (y * 8 + x)
}

impl Board {
    pub fn new() -> Self {
        Self {
            squares: [[None; 8]; 8],
            bitboards: [[0u64; 6]; 2],
            hash: 0,
            en_passant: None,
            castling: [[true, true], [true, true]],
        }
    }

    pub fn setup_standard(&mut self) {
        self.hash = 0;
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
            self.bitboards[c][p] &= !mask;
            self.hash ^= ZOBRIST[c][p][y * 8 + x];
        }
        self.squares[y][x] = piece;
        if let Some(pce) = piece {
            let c = color_idx(pce.color);
            let p = piece_index(pce.piece_type);
            self.bitboards[c][p] |= mask;
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
        let (ex, ey) = Self::algebraic_to_index(end)?;
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
                        prev_en_passant: prev_ep,
                        prev_castling,
                        rook_move,
                    });
                }
            }
        }

        self.set_index(ex, ey, Some(piece));
        self.set_index(sx, sy, None);

        Some(MoveState {
            start: (sx, sy),
            end: (ex, ey),
            captured,
            captured_sq,
            prev_en_passant: prev_ep,
            prev_castling,
            rook_move,
        })
    }

    pub fn unmake_move(&mut self, state: MoveState) {
        let moving = self.get_index(state.end.0, state.end.1);
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
        for yy in 0..8 {
            for xx in 0..8 {
                if let Some(p) = self.get_index(xx, yy) {
                    if p.color == by_color {
                        if let Some(pos) = Self::index_to_algebraic(xx, yy) {
                            for m in self.pseudo_legal_moves(&pos) {
                                if let Some((tx, ty)) = Self::algebraic_to_index(&m) {
                                    if tx == x && ty == y {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    pub fn in_check(&mut self, color: Color) -> bool {
        let king_sq = self.find_king(color);
        if king_sq.is_none() {
            return false;
        }
        let k = king_sq.unwrap();
        let opp = if color == Color::White {
            Color::Black
        } else {
            Color::White
        };
        for y in 0..8 {
            for x in 0..8 {
                if let Some(p) = self.get_index(x, y) {
                    if p.color == opp {
                        if let Some(pos) = Self::index_to_algebraic(x, y) {
                            for m in self.pseudo_legal_moves(&pos) {
                                if let Some((tx, ty)) = Self::algebraic_to_index(&m) {
                                    if tx == k.0 && ty == k.1 {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        false
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
        let (sx, sy) = match Self::algebraic_to_index(start) {
            Some(v) => v,
            None => return false,
        };
        let (ex, ey) = match Self::algebraic_to_index(end) {
            Some(v) => v,
            None => return false,
        };

        let piece = match self.get_index(sx, sy) {
            Some(p) => p,
            None => return false,
        };
        if piece.color != color {
            return false;
        }

        if let Some(dest) = self.get_index(ex, ey) {
            if dest.color == color {
                return false;
            }
        }

        let is_castle =
            piece.piece_type == PieceType::King && (sx as isize - ex as isize).abs() == 2;
        if is_castle {
            if self.in_check(color) {
                return false;
            }
            let step = if ex > sx { 1 } else { -1 };
            let opp = if color == Color::White {
                Color::Black
            } else {
                Color::White
            };
            let mut x = sx as isize + step;
            while x != ex as isize {
                if self.square_attacked(x as usize, sy, opp) {
                    return false;
                }
                x += step;
            }
        }

        if let Some(state) = self.make_move_state(start, end) {
            let check = self.in_check(color);
            self.unmake_move(state);
            !check
        } else {
            false
        }
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
        let cidx = color_idx(color);
        self.bitboards[cidx]
            .iter()
            .map(|bb| bb.count_ones() as usize)
            .sum()
    }

    pub fn piece_count_all(&self) -> usize {
        self.bitboards
            .iter()
            .flat_map(|b| b.iter())
            .map(|bb| bb.count_ones() as usize)
            .sum()
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

    #[inline]
    pub fn make_move_fast(&mut self, mv: Move, color: Color) -> UndoState {
        let from_sq = mv.from_sq();
        let to_sq = mv.to_sq();
        let from_x = (from_sq % 8) as usize;
        let from_y = (from_sq / 8) as usize;
        let to_x = (to_sq % 8) as usize;
        let to_y = (to_sq / 8) as usize;

        let piece = self.get_index(from_x, from_y).unwrap();
        let captured = self.get_index(to_x, to_y);

        let prev_ep = self
            .en_passant
            .map(|(x, y)| (y * 8 + x) as u8)
            .unwrap_or(UndoState::NO_EP);
        let prev_castling = self.pack_castling();
        let prev_hash = self.hash;

        let mut captured_piece_idx = UndoState::NO_CAPTURE;
        let mut captured_sq = to_sq;

        if mv.is_ep() {
            let cap_y = if color == Color::White {
                to_y - 1
            } else {
                to_y + 1
            };
            captured_sq = (cap_y * 8 + to_x) as u8;
            let cap_piece = self.get_index(to_x, cap_y).unwrap();
            captured_piece_idx = piece_index(cap_piece.piece_type) as u8;
            self.set_index(to_x, cap_y, None);
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
            if to_x == 0 && (to_y == 0 || to_y == 7) {
                let rank = if opp == 0 { 0 } else { 7 };
                if to_y == rank {
                    self.castling[opp][1] = false;
                }
            }
            if to_x == 7 && (to_y == 0 || to_y == 7) {
                let rank = if opp == 0 { 0 } else { 7 };
                if to_y == rank {
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

        self.set_index(to_x, to_y, Some(moving_piece));
        self.set_index(from_x, from_y, None);

        UndoState {
            mv,
            captured: captured_piece_idx,
            captured_sq,
            prev_ep,
            prev_castling,
            prev_hash,
        }
    }

    #[inline]
    pub fn unmake_move_fast(&mut self, state: UndoState, color: Color) {
        let mv = state.mv;
        let from_sq = mv.from_sq();
        let to_sq = mv.to_sq();
        let from_x = (from_sq % 8) as usize;
        let from_y = (from_sq / 8) as usize;
        let to_x = (to_sq % 8) as usize;
        let to_y = (to_sq / 8) as usize;

        let mut moving_piece = self.get_index(to_x, to_y).unwrap();

        if mv.is_promotion() {
            moving_piece.piece_type = PieceType::Pawn;
        }

        self.set_index(from_x, from_y, Some(moving_piece));
        self.set_index(to_x, to_y, None);

        if state.has_capture() {
            let cap_sq = state.captured_sq;
            let cap_x = (cap_sq % 8) as usize;
            let cap_y = (cap_sq / 8) as usize;
            let opp_color = if color == Color::White {
                Color::Black
            } else {
                Color::White
            };
            let cap_type = Self::piece_type_from_idx(state.captured as usize);
            self.set_index(
                cap_x,
                cap_y,
                Some(Piece {
                    piece_type: cap_type,
                    color: opp_color,
                }),
            );
        }

        if mv.is_castle() {
            let rank = from_y;
            if mv.flags() == Move::FLAG_KING_CASTLE {
                let rook = self.get_index(5, rank);
                self.set_index(7, rank, rook);
                self.set_index(5, rank, None);
            } else {
                let rook = self.get_index(3, rank);
                self.set_index(0, rank, rook);
                self.set_index(3, rank, None);
            }
        }

        self.en_passant = if state.prev_ep == UndoState::NO_EP {
            None
        } else {
            Some(((state.prev_ep % 8) as usize, (state.prev_ep / 8) as usize))
        };

        self.unpack_castling(state.prev_castling);

        self.hash = state.prev_hash;
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
        match idx {
            0 => PieceType::Pawn,
            1 => PieceType::Knight,
            2 => PieceType::Bishop,
            3 => PieceType::Rook,
            4 => PieceType::Queen,
            _ => PieceType::King,
        }
    }

    #[inline(always)]
    pub fn all_pieces(&self, color: Color) -> u64 {
        let cidx = color_idx(color);
        self.bitboards[cidx].iter().fold(0, |a, &b| a | b)
    }

    #[inline(always)]
    pub fn occupied(&self) -> u64 {
        self.all_pieces(Color::White) | self.all_pieces(Color::Black)
    }

    #[inline]
    pub fn is_square_attacked_by(&self, sq: u8, by_color: Color) -> bool {
        let cidx = color_idx(by_color);
        let sq_bb = 1u64 << sq;
        let occ = self.occupied();

        let pawn_attacks = if by_color == Color::White {
            let pawns = self.bitboards[cidx][0];
            ((pawns & !0x0101010101010101) << 7) | ((pawns & !0x8080808080808080) << 9)
        } else {
            let pawns = self.bitboards[cidx][0];
            ((pawns & !0x8080808080808080) >> 7) | ((pawns & !0x0101010101010101) >> 9)
        };
        if (pawn_attacks & sq_bb) != 0 {
            return true;
        }

        let knights = self.bitboards[cidx][1];
        if (crate::movegen::KNIGHT_TABLE[sq as usize] & knights) != 0 {
            return true;
        }

        let kings = self.bitboards[cidx][5];
        if (crate::movegen::KING_TABLE[sq as usize] & kings) != 0 {
            return true;
        }

        let bishops_queens = self.bitboards[cidx][2] | self.bitboards[cidx][4];
        let rooks_queens = self.bitboards[cidx][3] | self.bitboards[cidx][4];

        if self.diagonal_attacks(sq, occ) & bishops_queens != 0 {
            return true;
        }

        if self.straight_attacks(sq, occ) & rooks_queens != 0 {
            return true;
        }

        false
    }

    #[inline]
    fn diagonal_attacks(&self, sq: u8, occ: u64) -> u64 {
        let x = (sq % 8) as isize;
        let y = (sq / 8) as isize;
        let mut attacks = 0u64;

        for (dx, dy) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let mut nx = x + dx;
            let mut ny = y + dy;
            while nx >= 0 && nx < 8 && ny >= 0 && ny < 8 {
                let idx = (ny * 8 + nx) as usize;
                attacks |= 1u64 << idx;
                if (occ & (1u64 << idx)) != 0 {
                    break;
                }
                nx += dx;
                ny += dy;
            }
        }
        attacks
    }

    #[inline]
    fn straight_attacks(&self, sq: u8, occ: u64) -> u64 {
        let x = (sq % 8) as isize;
        let y = (sq / 8) as isize;
        let mut attacks = 0u64;

        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let mut nx = x + dx;
            let mut ny = y + dy;
            while nx >= 0 && nx < 8 && ny >= 0 && ny < 8 {
                let idx = (ny * 8 + nx) as usize;
                attacks |= 1u64 << idx;
                if (occ & (1u64 << idx)) != 0 {
                    break;
                }
                nx += dx;
                ny += dy;
            }
        }
        attacks
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

        let original_hash = board.hash;
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

        let hash_before_ep = board.hash;

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

        let original_hash = board.hash;

        let state = board.make_move_state("e7", "e8").unwrap();

        let piece = board.get("e8").unwrap();
        assert_eq!(piece.color, Color::White);
        assert!(board.get("e7").is_none());

        board.unmake_move(state);
        let pawn = board.get("e7").unwrap();
        assert_eq!(pawn.piece_type, PieceType::Pawn);
        assert!(board.get("e8").is_none());
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
}
