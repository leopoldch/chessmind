use crate::pieces::{Color, Piece, PieceType};

#[derive(Clone)]
pub struct MoveState {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub captured: Option<Piece>,
    pub prev_en_passant: Option<(usize, usize)>,
    pub prev_castling: [[bool; 2]; 2],
    pub rook_move: Option<((usize, usize), (usize, usize))>,
}

#[derive(Clone)]
pub struct Board {
    pub squares: [[Option<Piece>; 8]; 8],
    pub bitboards: [[u64; 6]; 2],
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
            en_passant: None,
            castling: [[true, true], [true, true]],
        }
    }

    pub fn setup_standard(&mut self) {
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
            self.set_index(x, 0, Some(Piece { piece_type: pt, color: Color::White }));
            self.set_index(x, 7, Some(Piece { piece_type: pt, color: Color::Black }));
            self.set_index(x, 1, Some(Piece { piece_type: PieceType::Pawn, color: Color::White }));
            self.set_index(x, 6, Some(Piece { piece_type: PieceType::Pawn, color: Color::Black }));
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
        }
        self.squares[y][x] = piece;
        if let Some(pce) = piece {
            let c = color_idx(pce.color);
            let p = piece_index(pce.piece_type);
            self.bitboards[c][p] |= mask;
        }
    }

    pub fn get_index(&self, x: usize, y: usize) -> Option<Piece> {
        self.squares[y][x]
    }

    pub fn get(&self, pos: &str) -> Option<Piece> {
        if let Some((x, y)) = Self::algebraic_to_index(pos) {
            self.get_index(x, y)
        } else { None }
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
        let prev_ep = self.en_passant;
        let prev_castling = self.castling;
        let mut rook_move = None;

        // castling rights updates
        let cidx = color_idx(piece.color);
        match piece.piece_type {
            PieceType::King => {
                self.castling[cidx] = [false, false];
                if (sx as isize - ex as isize).abs() == 2 {
                    if ex == 6 { // king side
                        rook_move = Some(((7, sy), (5, sy)));
                        let rook = self.get_index(7, sy);
                        self.set_index(5, sy, rook);
                        self.set_index(7, sy, None);
                    } else if ex == 2 { // queen side
                        rook_move = Some(((0, sy), (3, sy)));
                        let rook = self.get_index(0, sy);
                        self.set_index(3, sy, rook);
                        self.set_index(0, sy, None);
                    }
                }
            }
            PieceType::Rook => {
                if sx == 0 { self.castling[cidx][1] = false; }
                if sx == 7 { self.castling[cidx][0] = false; }
            }
            _ => {}
        }

        // en passant updates and captures
        self.en_passant = None;
        if piece.piece_type == PieceType::Pawn {
            let dir_y: isize = if piece.color == Color::White { 1 } else { -1 };
            if (sy as isize + 2 * dir_y) as usize == ey && sx == ex && self.get_index(ex, ey).is_none() {
                self.en_passant = Some((sx, (sy as isize + dir_y) as usize));
            }
            if let Some((epx, epy)) = prev_ep {
                if ex == epx && ey == epy && self.get_index(ex, ey).is_none() {
                    let cap_y = if piece.color == Color::White { ey - 1 } else { ey + 1 };
                    let cap = self.get_index(ex, cap_y);
                    self.set_index(ex, cap_y, None);
                    return Some(MoveState { start: (sx, sy), end: (ex, ey), captured: cap, prev_en_passant: prev_ep, prev_castling, rook_move });
                }
            }
        }

        self.set_index(ex, ey, Some(piece));
        self.set_index(sx, sy, None);

        Some(MoveState { start: (sx, sy), end: (ex, ey), captured, prev_en_passant: prev_ep, prev_castling, rook_move })
    }

    pub fn unmake_move(&mut self, state: MoveState) {
        self.set_index(state.start.0, state.start.1, self.get_index(state.end.0, state.end.1));
        self.set_index(state.end.0, state.end.1, state.captured);
        if let Some(((rsx, rsy), (rex, rey))) = state.rook_move {
            let rook = self.get_index(rex, rey);
            self.set_index(rsx, rsy, rook);
            self.set_index(rex, rey, None);
        }
        self.en_passant = state.prev_en_passant;
        self.castling = state.prev_castling;
    }

    pub fn algebraic_to_index(pos: &str) -> Option<(usize, usize)> {
        if pos.len() != 2 { return None; }
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
                for (dx, dy) in [(-2,-1),(-2,1),(-1,-2),(-1,2),(1,-2),(1,2),(2,-1),(2,1)] {
                    let nx = x as isize + dx;
                    let ny = y as isize + dy;
                    if Self::inside(nx, ny) {
                        if let Some(tgt) = self.get_index(nx as usize, ny as usize) {
                            if tgt.color != color {
                                if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) { moves.push(s); }
                            }
                        } else if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) { moves.push(s); }
                    }
                }
            }
            PieceType::Bishop => {
                for (dx, dy) in [(-1,-1),(-1,1),(1,-1),(1,1)] {
                    self.add_ray(x, y, dx, dy, color, &mut moves);
                }
            }
            PieceType::Rook => {
                for (dx, dy) in [(0,1),(0,-1),(1,0),(-1,0)] {
                    self.add_ray(x, y, dx, dy, color, &mut moves);
                }
            }
            PieceType::Queen => {
                for (dx, dy) in [(-1,-1),(-1,1),(1,-1),(1,1),(0,1),(0,-1),(1,0),(-1,0)] {
                    self.add_ray(x, y, dx, dy, color, &mut moves);
                }
            }
            PieceType::King => {
                for (dx, dy) in [(-1,-1),(-1,0),(-1,1),(0,-1),(0,1),(1,-1),(1,0),(1,1)] {
                    let nx = x as isize + dx;
                    let ny = y as isize + dy;
                    if Self::inside(nx, ny) {
                        if let Some(tgt) = self.get_index(nx as usize, ny as usize) {
                            if tgt.color != color {
                                if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) { moves.push(s); }
                            }
                        } else if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) { moves.push(s); }
                    }
                }
                let rank = if color == Color::White { 0 } else { 7 };
                let cidx = color_idx(color);
                if self.castling[cidx][0]
                    && self.get_index(5, rank).is_none()
                    && self.get_index(6, rank).is_none()
                {
                    if let Some(s) = Self::index_to_algebraic(6, rank) { moves.push(s); }
                }
                if self.castling[cidx][1]
                    && self.get_index(1, rank).is_none()
                    && self.get_index(2, rank).is_none()
                    && self.get_index(3, rank).is_none()
                {
                    if let Some(s) = Self::index_to_algebraic(2, rank) { moves.push(s); }
                }
            }
            PieceType::Pawn => {
                let dir_y: isize = if color == Color::White { 1 } else { -1 };
                let start_rank: usize = if color == Color::White { 1 } else { 6 };
                let ny = y as isize + dir_y;
                if Self::inside(x as isize, ny) && self.get_index(x, ny as usize).is_none() {
                    if let Some(s) = Self::index_to_algebraic(x, ny as usize) { moves.push(s); }
                    if y == start_rank {
                        let ny2 = y as isize + 2 * dir_y;
                        if Self::inside(x as isize, ny2) && self.get_index(x, ny2 as usize).is_none() {
                            if let Some(s) = Self::index_to_algebraic(x, ny2 as usize) { moves.push(s); }
                        }
                    }
                }
                for dx in [-1, 1] {
                    let nx = x as isize + dx;
                    if Self::inside(nx, ny) {
                        if let Some(tgt) = self.get_index(nx as usize, ny as usize) {
                            if tgt.color != color {
                                if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) { moves.push(s); }
                            }
                        } else if let Some((epx, epy)) = self.en_passant {
                            if epx as isize == nx && epy as isize == ny {
                                if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) { moves.push(s); }
                            }
                        }
                    }
                }
            }
        }
        moves
    }

    fn add_ray(&self, x: usize, y: usize, dx: isize, dy: isize, color: Color, acc: &mut Vec<String>) {
        let mut nx = x as isize + dx;
        let mut ny = y as isize + dy;
        while Self::inside(nx, ny) {
            match self.get_index(nx as usize, ny as usize) {
                None => {
                    if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) { acc.push(s); }
                }
                Some(p) => {
                    if p.color != color {
                        if let Some(s) = Self::index_to_algebraic(nx as usize, ny as usize) { acc.push(s); }
                    }
                    break;
                }
            }
            nx += dx;
            ny += dy;
        }
    }

    pub fn in_check(&mut self, color: Color) -> bool {
        let king_sq = self.find_king(color);
        if king_sq.is_none() { return false; }
        let k = king_sq.unwrap();
        let opp = if color == Color::White { Color::Black } else { Color::White };
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

    fn find_king(&self, color: Color) -> Option<(usize, usize)> {
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

    pub fn capture_moves(&mut self, color: Color) -> Vec<(String, String)> {
        let mut res = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                if let Some(pos) = Self::index_to_algebraic(x, y) {
                    if let Some(p) = self.get_index(x, y) {
                        if p.color == color {
                            for m in self.pseudo_legal_moves(&pos) {
                                if let Some((mx,my)) = Self::algebraic_to_index(&m) {
                                    if self.get_index(mx,my).is_some() {
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

    pub fn piece_count(&self, piece_type: PieceType) -> usize {
        let mut c = 0;
        for y in 0..8 {
            for x in 0..8 {
                if let Some(p) = self.get_index(x, y) {
                    if p.piece_type == piece_type { c += 1; }
                }
            }
        }
        c
    }
}
