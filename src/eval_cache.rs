use crate::board::Board;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};

const EVAL_CACHE_BITS: u32 = 18;
const PAWN_CACHE_BITS: u32 = 16;

const KEY_MASK: u64 = 0xFFFF_FFFF_0000_0000;

/// Lock-free cache of `i32` values keyed by a 64-bit hash.
///
/// Each entry is a single `AtomicU64` holding the upper 32 bits of the key and
/// the value in the lower 32 bits, so a reader can never see a key from one
/// write paired with a value from another (no torn entries between threads).
/// The low key bits select the slot; the upper 32 bits verify it.
pub struct EvalCache {
    entries: Box<[AtomicU64]>,
    mask: usize,
}

impl EvalCache {
    pub fn new() -> Self {
        Self::with_bits(EVAL_CACHE_BITS)
    }

    pub fn with_bits(bits: u32) -> Self {
        let size = 1usize << bits;
        let mut entries = Vec::with_capacity(size);
        entries.resize_with(size, || AtomicU64::new(0));
        Self {
            entries: entries.into_boxed_slice(),
            mask: size - 1,
        }
    }

    #[inline(always)]
    fn slot(&self, key: u64) -> &AtomicU64 {
        &self.entries[(key as usize) & self.mask]
    }

    #[inline(always)]
    pub fn get(&self, key: u64) -> Option<i32> {
        let data = self.slot(key).load(Ordering::Relaxed);
        if (data ^ key) & KEY_MASK == 0 {
            Some(data as u32 as i32)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn store(&self, key: u64, value: i32) {
        self.slot(key)
            .store((key & KEY_MASK) | (value as u32 as u64), Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn get_or_insert_with<F>(&self, key: u64, compute: F) -> i32
    where
        F: FnOnce() -> i32,
    {
        if let Some(value) = self.get(key) {
            value
        } else {
            let value = compute();
            self.store(key, value);
            value
        }
    }
}

impl Default for EvalCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Zobrist keys for pawns, built at compile time (xorshift64* sequence).
static PAWN_ZOBRIST: [[u64; 64]; 2] = {
    let mut arr = [[0u64; 64]; 2];
    let mut seed: u64 = 0x6c8e9cf570932bd5;
    let mut c = 0;
    while c < 2 {
        let mut s = 0;
        while s < 64 {
            seed ^= seed >> 12;
            seed ^= seed << 25;
            seed ^= seed >> 27;
            seed = seed.wrapping_mul(0x2545F4914F6CDD1D);
            arr[c][s] = seed;
            s += 1;
        }
        c += 1;
    }
    arr
};

#[inline(always)]
pub fn pawn_hash(board: &Board) -> u64 {
    let mut h = 0u64;

    let mut white_pawns = board.bitboards[0][0];
    while white_pawns != 0 {
        let sq = white_pawns.trailing_zeros() as usize;
        h ^= PAWN_ZOBRIST[0][sq];
        white_pawns &= white_pawns - 1;
    }

    let mut black_pawns = board.bitboards[1][0];
    while black_pawns != 0 {
        let sq = black_pawns.trailing_zeros() as usize;
        h ^= PAWN_ZOBRIST[1][sq];
        black_pawns &= black_pawns - 1;
    }

    h
}

pub static EVAL_CACHE: Lazy<EvalCache> = Lazy::new(EvalCache::new);
pub static PAWN_CACHE: Lazy<EvalCache> = Lazy::new(|| EvalCache::with_bits(PAWN_CACHE_BITS));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pawn_zobrist_matches_runtime_generator() {
        let mut seed: u64 = 0x6c8e9cf570932bd5;
        for c in 0..2 {
            for s in 0..64 {
                seed ^= seed >> 12;
                seed ^= seed << 25;
                seed ^= seed >> 27;
                seed = seed.wrapping_mul(0x2545F4914F6CDD1D);
                assert_eq!(PAWN_ZOBRIST[c][s], seed);
            }
        }
    }

    #[test]
    fn packed_entry_round_trips_values_and_rejects_other_keys() {
        let cache = EvalCache::with_bits(4);
        let key = 0xDEAD_BEEF_0000_0003u64;
        assert_eq!(cache.get(key), None);
        for value in [0, 1, -1, 12345, -12345, i32::MAX, i32::MIN] {
            cache.store(key, value);
            assert_eq!(cache.get(key), Some(value));
        }
        // Same slot, different upper key bits.
        assert_eq!(cache.get(0x1234_5678_0000_0003), None);
        assert_eq!(cache.get_or_insert_with(0x1234_5678_0000_0003, || 7), 7);
        assert_eq!(cache.get(key), None);
    }
}
