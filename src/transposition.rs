use once_cell::sync::Lazy;
use lru::LruCache;
use crate::pieces::Color;
use crate::board::Board;

#[derive(Clone, Copy)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone)]
pub struct TTEntry {
    pub depth: u32,
    pub value: i32,
    pub bound: Bound,
    pub best: Option<(String, String)>,
}

pub static ZOBRIST: Lazy<[[[u64; 64]; 6]; 2]> = Lazy::new(|| {
    let mut arr = [[[0u64; 64]; 6]; 2];
    let mut seed: u64 = 0xcbf29ce484222325;
    for c in 0..2 {
        for p in 0..6 {
            for s in 0..64 {
                seed ^= seed >> 12;
                seed ^= seed << 25;
                seed ^= seed >> 27;
                seed = seed.wrapping_mul(0x2545F4914F6CDD1D);
                arr[c][p][s] = seed;
            }
        }
    }
    arr
});

pub static ZOBRIST_SIDE: Lazy<u64> = Lazy::new(|| 0x9d39247e33776d41);

impl Board {
    pub fn hash(&self, side: Color) -> u64 {
        if side == Color::White { self.hash ^ *ZOBRIST_SIDE } else { self.hash }
    }

    pub fn recompute_hash(&mut self) {
        let mut h = 0u64;
        for c in 0..2 {
            for p in 0..6 {
                let mut bb = self.bitboards[c][p];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as usize;
                    h ^= ZOBRIST[c][p][sq];
                    bb &= bb - 1;
                }
            }
        }
        self.hash = h;
    }
}

pub type Table = LruCache<u64, TTEntry>;

pub const TABLE_SIZE: usize = 100_000;
