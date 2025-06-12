use once_cell::sync::Lazy;
use crate::pieces::{Color, PieceType};
use crate::board::{Board, color_idx, piece_index};

const DIRS_KNIGHT: &[(isize,isize)] = &[(-2,-1),(-2,1),(-1,-2),(-1,2),(1,-2),(1,2),(2,-1),(2,1)];
const DIRS_KING: &[(isize,isize)] = &[(-1,-1),(-1,0),(-1,1),(0,-1),(0,1),(1,-1),(1,0),(1,1)];

pub static KNIGHT_TABLE: Lazy<[u64;64]> = Lazy::new(|| {
    let mut arr = [0u64;64];
    for y in 0..8 { for x in 0..8 {
        let mut bb = 0u64;
        for (dx,dy) in DIRS_KNIGHT { let nx = x as isize + dx; let ny = y as isize + dy; if nx>=0 && nx<8 && ny>=0 && ny<8 { bb |= 1u64 << (ny*8+nx); } }
        arr[y*8+x] = bb;
    }}
    arr
});

pub static KING_TABLE: Lazy<[u64;64]> = Lazy::new(|| {
    let mut arr = [0u64;64];
    for y in 0..8 { for x in 0..8 {
        let mut bb = 0u64;
        for (dx,dy) in DIRS_KING { let nx = x as isize + dx; let ny = y as isize + dy; if nx>=0 && nx<8 && ny>=0 && ny<8 { bb |= 1u64 << (ny*8+nx); } }
        arr[y*8+x] = bb;
    }}
    arr
});

pub static WHITE_PAWN_ATTACKS: Lazy<[u64;64]> = Lazy::new(|| {
    let mut arr = [0u64;64];
    for y in 0..8 { for x in 0..8 {
        let mut bb = 0u64;
        if x>0 && y<7 { bb |= 1u64 << ((y+1)*8 + (x-1)); }
        if x<7 && y<7 { bb |= 1u64 << ((y+1)*8 + (x+1)); }
        arr[y*8+x] = bb;
    }}
    arr
});

pub static BLACK_PAWN_ATTACKS: Lazy<[u64;64]> = Lazy::new(|| {
    let mut arr = [0u64;64];
    for y in 0..8 { for x in 0..8 {
        let mut bb = 0u64;
        if x>0 && y>0 { bb |= 1u64 << ((y-1)*8 + (x-1)); }
        if x<7 && y>0 { bb |= 1u64 << ((y-1)*8 + (x+1)); }
        arr[y*8+x] = bb;
    }}
    arr
});

fn rook_attacks(sq: usize, occ: u64) -> u64 {
    let x = (sq % 8) as isize;
    let y = (sq / 8) as isize;
    let mut attacks = 0u64;
    let mut ny = y+1;
    while ny < 8 { let idx = (ny*8+x) as usize; attacks |= 1u64<<idx; if (occ & (1u64<<idx)) != 0 { break; } ny += 1; }
    ny = y-1;
    while ny >=0 { let idx = (ny*8+x) as usize; attacks |= 1u64<<idx; if (occ & (1u64<<idx)) != 0 { break; } if ny==0 { break; } ny -= 1; }
    let mut nx = x+1;
    while nx < 8 { let idx = (y*8+nx) as usize; attacks |= 1u64<<idx; if (occ & (1u64<<idx)) != 0 { break; } nx += 1; }
    nx = x-1;
    while nx >=0 { let idx = (y*8+nx) as usize; attacks |= 1u64<<idx; if (occ & (1u64<<idx)) != 0 { break; } if nx==0 { break; } nx -= 1; }
    attacks
}

fn bishop_attacks(sq: usize, occ: u64) -> u64 {
    let x = (sq % 8) as isize;
    let y = (sq / 8) as isize;
    let mut attacks = 0u64;
    let mut nx = x+1; let mut ny = y+1;
    while nx<8 && ny<8 { let idx = (ny*8+nx) as usize; attacks |= 1u64<<idx; if (occ & (1u64<<idx))!=0 { break; } nx+=1; ny+=1; }
    nx = x-1; ny = y+1;
    while nx>=0 && ny<8 { let idx = (ny*8+nx) as usize; attacks |= 1u64<<idx; if (occ & (1u64<<idx))!=0 { break; } if nx==0 { break; } nx-=1; ny+=1; }
    nx = x+1; ny = y-1;
    while nx<8 && ny>=0 { let idx = (ny*8+nx) as usize; attacks |= 1u64<<idx; if (occ & (1u64<<idx))!=0 { break; } if ny==0 { break; } nx+=1; ny-=1; }
    nx = x-1; ny = y-1;
    while nx>=0 && ny>=0 { let idx = (ny*8+nx) as usize; attacks |= 1u64<<idx; if (occ & (1u64<<idx))!=0 { break; } if nx==0 || ny==0 { attacks|=0; }; nx-=1; ny-=1; if nx<0||ny<0{break;} }
    attacks
}

fn pawn_moves(sq: usize, color: Color, occ: u64, opp_occ: u64, en_passant: Option<(usize,usize)>) -> u64 {
    let x = sq % 8;
    let y = sq / 8;
    let mut moves = 0u64;
    match color {
        Color::White => {
            if y<7 && (occ & (1u64<<((y+1)*8+x))) == 0 { moves |= 1u64<<((y+1)*8+x); if y==1 && (occ & (1u64<<((y+2)*8+x)))==0 { moves |= 1u64<<((y+2)*8+x); } }
            if x>0 && y<7 && (opp_occ & (1u64<<((y+1)*8+x-1))) != 0 { moves |= 1u64<<((y+1)*8+x-1); }
            if x<7 && y<7 && (opp_occ & (1u64<<((y+1)*8+x+1))) != 0 { moves |= 1u64<<((y+1)*8+x+1); }
            if let Some((ex,ey)) = en_passant { if ey==y+1 && ((ex==x+1)||(ex+1==x)) { moves |= 1u64<<(ey*8+ex); } }
        }
        Color::Black => {
            if y>0 && (occ & (1u64<<((y-1)*8+x))) == 0 { moves |= 1u64<<((y-1)*8+x); if y==6 && (occ & (1u64<<((y-2)*8+x)))==0 { moves |= 1u64<<((y-2)*8+x); } }
            if x>0 && y>0 && (opp_occ & (1u64<<((y-1)*8+x-1))) != 0 { moves |= 1u64<<((y-1)*8+x-1); }
            if x<7 && y>0 && (opp_occ & (1u64<<((y-1)*8+x+1))) != 0 { moves |= 1u64<<((y-1)*8+x+1); }
            if let Some((ex,ey)) = en_passant { if ey+1==y && ((ex==x+1)||(ex+1==x)) { moves |= 1u64<<(ey*8+ex); } }
        }
    }
    moves
}

pub fn generate_moves(board: &mut Board, color: Color) -> Vec<(String,String)> {
    let cidx = color_idx(color);
    let occ_self: u64 = board.bitboards[cidx].iter().fold(0u64, |a,&b| a|b);
    let occ_opp: u64 = board.bitboards[1-cidx].iter().fold(0u64, |a,&b| a|b);
    let occ_all = occ_self | occ_opp;

    let mut res = Vec::new();

    for pt in [PieceType::Pawn,PieceType::Knight,PieceType::Bishop,PieceType::Rook,PieceType::Queen,PieceType::King] {
        let mut bb = board.bitboards[cidx][piece_index(pt)];
        while bb != 0 {
            let sq = bb.trailing_zeros() as usize;
            let from = Board::index_to_algebraic(sq %8, sq /8).unwrap();
            let mut targets;
            match pt {
                PieceType::Pawn => targets = pawn_moves(sq, color, occ_all, occ_opp, board.en_passant),
                PieceType::Knight => targets = KNIGHT_TABLE[sq],
                PieceType::Bishop => targets = bishop_attacks(sq, occ_all),
                PieceType::Rook => targets = rook_attacks(sq, occ_all),
                PieceType::Queen => targets = bishop_attacks(sq, occ_all) | rook_attacks(sq, occ_all),
                PieceType::King => {
                    targets = KING_TABLE[sq];
                    let rank = if color == Color::White {0} else {7};
                    if sq == rank*8 + 4 {
                        if board.castling[cidx][0] && board.get_index(5,rank).is_none() && board.get_index(6,rank).is_none() { targets |= 1u64<<(rank*8+6); }
                        if board.castling[cidx][1] && board.get_index(1,rank).is_none() && board.get_index(2,rank).is_none() && board.get_index(3,rank).is_none() { targets |= 1u64<<(rank*8+2); }
                    }
                }
            }
            targets &= !occ_self;
            while targets != 0 { let to_sq = targets.trailing_zeros() as usize; let to = Board::index_to_algebraic(to_sq%8,to_sq/8).unwrap(); if board.is_legal(&from,&to,color) { res.push((from.clone(),to)); } targets &= targets-1; }
            bb &= bb - 1;
        }
    }
    res
}
