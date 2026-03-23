use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

const CACHE_BITS: usize = 18;
const CACHE_SIZE: usize = 1 << CACHE_BITS;
const CACHE_MASK: usize = CACHE_SIZE - 1;

struct Entry {
    key: AtomicU64,
    value: AtomicI32,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            key: AtomicU64::new(0),
            value: AtomicI32::new(0),
        }
    }
}

pub struct EvalCache {
    entries: Box<[Entry]>,
}

impl EvalCache {
    pub fn new() -> Self {
        let mut entries = Vec::with_capacity(CACHE_SIZE);
        entries.resize_with(CACHE_SIZE, Entry::default);
        Self {
            entries: entries.into_boxed_slice(),
        }
    }

    #[inline(always)]
    pub fn get(&self, key: u64) -> Option<i32> {
        let entry = &self.entries[(key as usize) & CACHE_MASK];
        if entry.key.load(Ordering::Acquire) == key {
            Some(entry.value.load(Ordering::Relaxed))
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn store(&self, key: u64, value: i32) {
        let entry = &self.entries[(key as usize) & CACHE_MASK];
        entry.value.store(value, Ordering::Relaxed);
        entry.key.store(key, Ordering::Release);
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

pub static EVAL_CACHE: Lazy<EvalCache> = Lazy::new(EvalCache::new);
