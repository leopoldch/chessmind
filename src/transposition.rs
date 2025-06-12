use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering, AtomicU8};
use std::sync::Arc;

use crate::pieces::Color;
use crate::board::Board;

#[derive(Clone, Copy)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
pub struct TTEntry {
    pub depth: u32,
    pub value: i32,
    pub bound: Bound,
    pub best: Option<(u8, u8)>,
}

struct RawEntry {
    key: AtomicU64,
    value: AtomicI32,
    packed: AtomicU64,
}

impl Default for RawEntry {
    fn default() -> Self {
        Self {
            key: AtomicU64::new(0),
            value: AtomicI32::new(0),
            packed: AtomicU64::new(0),
        }
    }
}

struct Inner {
    entries: Vec<RawEntry>,
    age: AtomicU8,
}

#[derive(Clone)]
pub struct Table(Arc<Inner>);

impl Table {
    pub fn new(size: usize) -> Self {
        let mut entries = Vec::with_capacity(size);
        entries.resize_with(size, RawEntry::default);
        Self(Arc::new(Inner { entries, age: AtomicU8::new(0) }))
    }

    fn current_age(&self) -> u8 {
        self.0.age.load(Ordering::Relaxed)
    }

    pub fn next_age(&self) {
        let age = self.current_age().wrapping_add(1);
        self.0.age.store(age, Ordering::Relaxed);
    }

    pub fn get(&self, key: u64) -> Option<TTEntry> {
        let idx = (key as usize) % self.0.entries.len();
        let entry = &self.0.entries[idx];
        if entry.key.load(Ordering::Acquire) == key {
            let value = entry.value.load(Ordering::Relaxed);
            let packed = entry.packed.load(Ordering::Relaxed);
            let depth = (packed >> 32) as u32;
            let bound = match ((packed >> 16) & 0xFF) as u8 {
                1 => Bound::Lower,
                2 => Bound::Upper,
                _ => Bound::Exact,
            };
            let from = ((packed >> 8) & 0xFF) as u8;
            let to = (packed & 0xFF) as u8;
            let best = if from == 0xFF { None } else { Some((from, to)) };
            Some(TTEntry { depth, value, bound, best })
        } else {
            None
        }
    }

    pub fn store(&self, key: u64, entry: TTEntry) {
        let idx = (key as usize) % self.0.entries.len();
        let slot = &self.0.entries[idx];
        let age = self.current_age();
        let packed_new = ((entry.depth as u64) << 32)
            | ((age as u64) << 24)
            | (((match entry.bound { Bound::Exact => 0, Bound::Lower => 1, Bound::Upper => 2 }) as u64) << 16)
            | ((entry.best.map(|b| b.0).unwrap_or(0xFF) as u64) << 8)
            | (entry.best.map(|b| b.1).unwrap_or(0xFF) as u64);

        let existing_key = slot.key.load(Ordering::Acquire);
        if existing_key != key {
            let existing = slot.packed.load(Ordering::Relaxed);
            let existing_depth = (existing >> 32) as u32;
            let existing_age = ((existing >> 24) & 0xFF) as u8;
            if entry.depth >= existing_depth || age.wrapping_sub(existing_age) > 5 {
                slot.key.store(key, Ordering::Release);
                slot.value.store(entry.value, Ordering::Relaxed);
                slot.packed.store(packed_new, Ordering::Relaxed);
            }
        } else {
            let existing = slot.packed.load(Ordering::Relaxed);
            let existing_depth = (existing >> 32) as u32;
            if entry.depth >= existing_depth {
                slot.value.store(entry.value, Ordering::Relaxed);
                slot.packed.store(packed_new, Ordering::Relaxed);
            }
        }
    }
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

pub const TABLE_SIZE: usize = 100_000;
