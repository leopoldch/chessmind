use once_cell::sync::Lazy;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use crate::board::{Board, color_idx, piece_index};
use crate::pieces::{Color, PieceType};

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
    data: AtomicU64,
}

impl Default for RawEntry {
    fn default() -> Self {
        Self {
            key: AtomicU64::new(0),
            data: AtomicU64::new(0),
        }
    }
}

struct Bucket {
    lock: AtomicU8,
    entries: [RawEntry; CLUSTER_SIZE],
}

impl Default for Bucket {
    fn default() -> Self {
        Self {
            lock: AtomicU8::new(0),
            entries: std::array::from_fn(|_| RawEntry::default()),
        }
    }
}

struct Inner {
    buckets: Vec<Bucket>,
    age: AtomicU8,
}

#[derive(Clone)]
pub struct Table(Arc<Inner>);

const CLUSTER_SIZE: usize = 4;
const DATA_OCCUPIED_BIT: u64 = 1 << 63;
const VALUE_MASK: u64 = 0xFFFF_FFFF;
const DEPTH_SHIFT: u64 = 32;
const DEPTH_MASK: u64 = 0x7F;
const AGE_SHIFT: u64 = 39;
const AGE_MASK: u64 = 0xFF;
const BOUND_SHIFT: u64 = 47;
const BOUND_MASK: u64 = 0x03;
const FROM_SHIFT: u64 = 49;
const TO_SHIFT: u64 = 56;
const MOVE_MASK: u64 = 0x7F;
const NO_SQUARE: u8 = 0x7F;

impl Table {
    pub fn new(size: usize) -> Self {
        let bucket_count = size.max(1).div_ceil(CLUSTER_SIZE);
        let mut buckets = Vec::with_capacity(bucket_count);
        buckets.resize_with(bucket_count, Bucket::default);
        Self(Arc::new(Inner {
            buckets,
            age: AtomicU8::new(0),
        }))
    }

    fn current_age(&self) -> u8 {
        self.0.age.load(Ordering::Relaxed)
    }

    pub fn next_age(&self) {
        let age = self.current_age().wrapping_add(1);
        self.0.age.store(age, Ordering::Relaxed);
    }

    pub fn get(&self, key: u64) -> Option<TTEntry> {
        let bucket = self.bucket(key);
        for entry in &bucket.entries {
            let key_before = entry.key.load(Ordering::Acquire);
            let data = entry.data.load(Ordering::Relaxed);
            let key_after = entry.key.load(Ordering::Acquire);

            if key_before != key_after || key_after != key || !Self::is_occupied(data) {
                continue;
            }

            return Some(Self::decode_entry(data));
        }
        None
    }

    pub fn store(&self, key: u64, entry: TTEntry) {
        let age = self.current_age();
        let incoming = Self::encode_entry(entry, age);
        let bucket = self.bucket(key);
        Self::lock_bucket(bucket);

        let mut empty_slot: Option<&RawEntry> = None;
        let mut victim = &bucket.entries[0];
        let mut victim_score = i32::MAX;

        for slot in &bucket.entries {
            let existing_key = slot.key.load(Ordering::Acquire);
            let existing_data = slot.data.load(Ordering::Relaxed);

            if Self::is_occupied(existing_data) && existing_key == key {
                if Self::should_replace(existing_data, incoming, age) {
                    slot.data.store(incoming, Ordering::Release);
                }
                Self::unlock_bucket(bucket);
                return;
            }

            if !Self::is_occupied(existing_data) {
                empty_slot = Some(slot);
                continue;
            }

            let score = Self::retention_score(existing_data, age);
            if score < victim_score {
                victim = slot;
                victim_score = score;
            }
        }

        let slot = empty_slot.unwrap_or(victim);
        slot.data.store(incoming, Ordering::Relaxed);
        slot.key.store(key, Ordering::Release);
        Self::unlock_bucket(bucket);
    }

    fn bucket(&self, key: u64) -> &Bucket {
        let idx = (key as usize) % self.0.buckets.len();
        &self.0.buckets[idx]
    }

    fn lock_bucket(bucket: &Bucket) {
        while bucket
            .lock
            .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while bucket.lock.load(Ordering::Relaxed) != 0 {
                std::hint::spin_loop();
            }
        }
    }

    fn unlock_bucket(bucket: &Bucket) {
        bucket.lock.store(0, Ordering::Release);
    }

    fn encode_entry(entry: TTEntry, age: u8) -> u64 {
        let depth = entry.depth.min(DEPTH_MASK as u32) as u64;
        let bound = match entry.bound {
            Bound::Exact => 0,
            Bound::Lower => 1,
            Bound::Upper => 2,
        } as u64;
        let from = entry.best.map(|mv| mv.0).unwrap_or(NO_SQUARE) as u64;
        let to = entry.best.map(|mv| mv.1).unwrap_or(NO_SQUARE) as u64;

        DATA_OCCUPIED_BIT
            | ((entry.value as u32 as u64) & VALUE_MASK)
            | (depth << DEPTH_SHIFT)
            | ((age as u64) << AGE_SHIFT)
            | (bound << BOUND_SHIFT)
            | (from << FROM_SHIFT)
            | (to << TO_SHIFT)
    }

    fn decode_entry(data: u64) -> TTEntry {
        let value = (data & VALUE_MASK) as u32 as i32;
        let depth = ((data >> DEPTH_SHIFT) & DEPTH_MASK) as u32;
        let bound = match ((data >> BOUND_SHIFT) & BOUND_MASK) as u8 {
            1 => Bound::Lower,
            2 => Bound::Upper,
            _ => Bound::Exact,
        };
        let from = ((data >> FROM_SHIFT) & MOVE_MASK) as u8;
        let to = ((data >> TO_SHIFT) & MOVE_MASK) as u8;
        let best = if from == NO_SQUARE || to == NO_SQUARE {
            None
        } else {
            Some((from, to))
        };

        TTEntry {
            depth,
            value,
            bound,
            best,
        }
    }

    fn is_occupied(data: u64) -> bool {
        (data & DATA_OCCUPIED_BIT) != 0
    }

    fn age_of(data: u64) -> u8 {
        ((data >> AGE_SHIFT) & AGE_MASK) as u8
    }

    fn depth_of(data: u64) -> u32 {
        ((data >> DEPTH_SHIFT) & DEPTH_MASK) as u32
    }

    fn bound_of(data: u64) -> Bound {
        match ((data >> BOUND_SHIFT) & BOUND_MASK) as u8 {
            1 => Bound::Lower,
            2 => Bound::Upper,
            _ => Bound::Exact,
        }
    }

    fn has_best_move(data: u64) -> bool {
        ((data >> FROM_SHIFT) & MOVE_MASK) as u8 != NO_SQUARE
            && ((data >> TO_SHIFT) & MOVE_MASK) as u8 != NO_SQUARE
    }

    fn retention_score(data: u64, current_age: u8) -> i32 {
        let depth_score = (Self::depth_of(data) as i32) * 8;
        let freshness_penalty = (current_age.wrapping_sub(Self::age_of(data)) as i32) * 4;
        let bound_bonus = match Self::bound_of(data) {
            Bound::Exact => 6,
            Bound::Lower => 3,
            Bound::Upper => 0,
        };
        let best_move_bonus = if Self::has_best_move(data) { 1 } else { 0 };

        depth_score + bound_bonus + best_move_bonus - freshness_penalty
    }

    fn should_replace(existing: u64, incoming: u64, current_age: u8) -> bool {
        if !Self::is_occupied(existing) {
            return true;
        }

        if Self::retention_score(incoming, current_age)
            > Self::retention_score(existing, current_age)
        {
            return true;
        }

        let existing_depth = Self::depth_of(existing);
        let incoming_depth = Self::depth_of(incoming);
        matches!(Self::bound_of(incoming), Bound::Exact)
            && !matches!(Self::bound_of(existing), Bound::Exact)
            && incoming_depth.saturating_add(1) >= existing_depth
    }
}

const FILE_A: u64 = 0x0101_0101_0101_0101;
const FILE_H: u64 = 0x8080_8080_8080_8080;

fn splitmix64(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

pub static ZOBRIST: Lazy<[[[u64; 64]; 6]; 2]> = Lazy::new(|| {
    let mut arr = [[[0u64; 64]; 6]; 2];
    let mut seed: u64 = 0xcbf29ce484222325;
    for c in 0..2 {
        for p in 0..6 {
            for s in 0..64 {
                arr[c][p][s] = splitmix64(&mut seed);
            }
        }
    }
    arr
});

pub static ZOBRIST_CASTLING: Lazy<[u64; 4]> = Lazy::new(|| {
    let mut seed: u64 = 0x3243f6a8885a308d;
    std::array::from_fn(|_| splitmix64(&mut seed))
});

pub static ZOBRIST_EP_FILE: Lazy<[u64; 8]> = Lazy::new(|| {
    let mut seed: u64 = 0x13198a2e03707344;
    std::array::from_fn(|_| splitmix64(&mut seed))
});

pub static ZOBRIST_SIDE: Lazy<u64> = Lazy::new(|| 0x9d39247e33776d41);

impl Board {
    #[inline(always)]
    fn castling_hash(&self) -> u64 {
        let mut h = 0;
        if self.castling[0][0] {
            h ^= ZOBRIST_CASTLING[0];
        }
        if self.castling[0][1] {
            h ^= ZOBRIST_CASTLING[1];
        }
        if self.castling[1][0] {
            h ^= ZOBRIST_CASTLING[2];
        }
        if self.castling[1][1] {
            h ^= ZOBRIST_CASTLING[3];
        }
        h
    }

    #[inline(always)]
    fn en_passant_hash(&self, side: Color) -> u64 {
        let Some((file, rank)) = self.en_passant else {
            return 0;
        };

        if file >= 8 || rank >= 8 || !self.has_en_passant_capture(side, file, rank) {
            return 0;
        }

        ZOBRIST_EP_FILE[file]
    }

    #[inline(always)]
    fn has_en_passant_capture(&self, side: Color, file: usize, rank: usize) -> bool {
        let ep_sq = rank * 8 + file;
        let ep_mask = 1u64 << ep_sq;

        let pawns = self.bitboards[color_idx(side)][piece_index(PieceType::Pawn)];
        let attacks = if side == Color::White {
            ((pawns & !FILE_A) << 7) | ((pawns & !FILE_H) << 9)
        } else {
            ((pawns & !FILE_H) >> 7) | ((pawns & !FILE_A) >> 9)
        };
        if (attacks & ep_mask) == 0 {
            return false;
        }

        let captured_sq = if side == Color::White {
            ep_sq.checked_sub(8)
        } else {
            ep_sq.checked_add(8).filter(|sq| *sq < 64)
        };
        let Some(captured_sq) = captured_sq else {
            return false;
        };

        let opp = if side == Color::White {
            Color::Black
        } else {
            Color::White
        };
        let opp_pawns = self.bitboards[color_idx(opp)][piece_index(PieceType::Pawn)];
        (opp_pawns & (1u64 << captured_sq)) != 0
    }

    pub fn hash(&self, side: Color) -> u64 {
        let mut h = self.hash ^ self.castling_hash() ^ self.en_passant_hash(side);
        if side == Color::White {
            h ^= *ZOBRIST_SIDE;
        }
        h
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

pub const TABLE_SIZE: usize = 4_194_304;

#[cfg(test)]
mod tests {
    use super::{Bound, TTEntry, Table};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn stores_multiple_colliding_keys_in_same_bucket() {
        let table = Table::new(4);
        let keys = [0u64, 4, 8, 12];

        for (idx, key) in keys.into_iter().enumerate() {
            table.store(
                key,
                TTEntry {
                    depth: 4 + idx as u32,
                    value: 50 + idx as i32,
                    bound: Bound::Exact,
                    best: Some((idx as u8, (idx + 1) as u8)),
                },
            );
        }

        for (idx, key) in keys.into_iter().enumerate() {
            let entry = table.get(key).expect("clustered slot should retain key");
            assert_eq!(entry.depth, 4 + idx as u32);
            assert_eq!(entry.value, 50 + idx as i32);
            assert_eq!(entry.best, Some((idx as u8, (idx + 1) as u8)));
        }
    }

    #[test]
    fn deeper_exact_entry_survives_shallower_collision() {
        let table = Table::new(4);
        table.store(
            0,
            TTEntry {
                depth: 10,
                value: 100,
                bound: Bound::Exact,
                best: Some((1, 2)),
            },
        );
        table.store(
            4,
            TTEntry {
                depth: 2,
                value: -20,
                bound: Bound::Upper,
                best: Some((3, 4)),
            },
        );
        table.store(
            8,
            TTEntry {
                depth: 3,
                value: -10,
                bound: Bound::Lower,
                best: Some((5, 6)),
            },
        );
        table.store(
            12,
            TTEntry {
                depth: 1,
                value: 0,
                bound: Bound::Upper,
                best: None,
            },
        );

        table.store(
            16,
            TTEntry {
                depth: 2,
                value: 5,
                bound: Bound::Upper,
                best: Some((7, 8)),
            },
        );

        let kept = table.get(0).expect("deep exact entry should be retained");
        assert_eq!(kept.depth, 10);
        assert_eq!(kept.value, 100);
        assert_eq!(kept.best, Some((1, 2)));
    }

    #[test]
    fn concurrent_access_keeps_entries_decodable() {
        let table = Arc::new(Table::new(32));
        let mut handles = Vec::new();

        for id in 0..4u64 {
            let table = table.clone();
            handles.push(thread::spawn(move || {
                for n in 0..2000u64 {
                    let key = (n * 32) + id;
                    table.store(
                        key,
                        TTEntry {
                            depth: ((n % 32) + 1) as u32,
                            value: (n as i32) - 500,
                            bound: if n % 3 == 0 {
                                Bound::Exact
                            } else if n % 3 == 1 {
                                Bound::Lower
                            } else {
                                Bound::Upper
                            },
                            best: Some(((n % 64) as u8, ((n + 1) % 64) as u8)),
                        },
                    );

                    if let Some(entry) = table.get(key) {
                        assert!(entry.depth <= 32);
                        if let Some((from, to)) = entry.best {
                            assert!(from < 64);
                            assert!(to < 64);
                        }
                    }
                }
            }));
        }

        for handle in handles {
            handle.join().expect("threaded TT access should succeed");
        }
    }
}
