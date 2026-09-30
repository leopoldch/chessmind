const DIR_N: usize = 0;
const DIR_S: usize = 1;
const DIR_E: usize = 2;
const DIR_W: usize = 3;
const DIR_NE: usize = 4;
const DIR_NW: usize = 5;
const DIR_SE: usize = 6;
const DIR_SW: usize = 7;

const DIRS: [(i32, i32); 8] = [
    (0, 1),
    (0, -1),
    (1, 0),
    (-1, 0),
    (1, 1),
    (-1, 1),
    (1, -1),
    (-1, -1),
];

/// Empty-board ray from each square in each direction (4 KB).
pub static RAYS: [[u64; 8]; 64] = build_rays();

/// Squares strictly between two aligned squares (0 when not aligned).
pub static BETWEEN: [[u64; 64]; 64] = build_between();

/// Empty-board rook / bishop attack sets, used as cheap pre-filters.
pub static ROOK_PSEUDO: [u64; 64] = build_pseudo(0);
pub static BISHOP_PSEUDO: [u64; 64] = build_pseudo(4);

const fn build_rays() -> [[u64; 8]; 64] {
    let mut rays = [[0u64; 8]; 64];
    let mut from = 0;
    while from < 64 {
        let fx = (from % 8) as i32;
        let fy = (from / 8) as i32;
        let mut dir = 0;
        while dir < 8 {
            let (dx, dy) = DIRS[dir];
            let mut x = fx + dx;
            let mut y = fy + dy;
            let mut mask = 0u64;
            while x >= 0 && x < 8 && y >= 0 && y < 8 {
                mask |= 1u64 << (y * 8 + x);
                x += dx;
                y += dy;
            }
            rays[from][dir] = mask;
            dir += 1;
        }
        from += 1;
    }
    rays
}

const fn build_between() -> [[u64; 64]; 64] {
    let mut between = [[0u64; 64]; 64];
    let mut from = 0;
    while from < 64 {
        let fx = (from % 8) as i32;
        let fy = (from / 8) as i32;
        let mut dir = 0;
        while dir < 8 {
            let (dx, dy) = DIRS[dir];
            let mut x = fx + dx;
            let mut y = fy + dy;
            let mut prefix = 0u64;
            while x >= 0 && x < 8 && y >= 0 && y < 8 {
                let sq = (y * 8 + x) as usize;
                between[from][sq] = prefix;
                prefix |= 1u64 << sq;
                x += dx;
                y += dy;
            }
            dir += 1;
        }
        from += 1;
    }
    between
}

const fn build_pseudo(first_dir: usize) -> [u64; 64] {
    let rays = build_rays();
    let mut out = [0u64; 64];
    let mut sq = 0;
    while sq < 64 {
        out[sq] = rays[sq][first_dir]
            | rays[sq][first_dir + 1]
            | rays[sq][first_dir + 2]
            | rays[sq][first_dir + 3];
        sq += 1;
    }
    out
}

/// Attacks along an ascending ray (N, E, NE, NW): the nearest blocker is the
/// lowest set bit. Bit 63 is forced in so that an empty blocker set indexes
/// square 63, whose ascending rays are all empty.
#[inline(always)]
fn ray_attacks_up(sq: usize, occ: u64, dir: usize) -> u64 {
    let ray = RAYS[sq & 63][dir];
    let blocker = ((ray & occ) | (1u64 << 63)).trailing_zeros() as usize;
    ray ^ RAYS[blocker & 63][dir]
}

/// Attacks along a descending ray (S, W, SE, SW): the nearest blocker is the
/// highest set bit. Bit 0 is forced in so that an empty blocker set indexes
/// square 0, whose descending rays are all empty.
#[inline(always)]
fn ray_attacks_down(sq: usize, occ: u64, dir: usize) -> u64 {
    let ray = RAYS[sq & 63][dir];
    let blocker = 63 - ((ray & occ) | 1).leading_zeros() as usize;
    ray ^ RAYS[blocker & 63][dir]
}

#[inline(always)]
pub fn rook_attacks(sq: usize, occ: u64) -> u64 {
    ray_attacks_up(sq, occ, DIR_N)
        | ray_attacks_down(sq, occ, DIR_S)
        | ray_attacks_up(sq, occ, DIR_E)
        | ray_attacks_down(sq, occ, DIR_W)
}

#[inline(always)]
pub fn bishop_attacks(sq: usize, occ: u64) -> u64 {
    ray_attacks_up(sq, occ, DIR_NE)
        | ray_attacks_up(sq, occ, DIR_NW)
        | ray_attacks_down(sq, occ, DIR_SE)
        | ray_attacks_down(sq, occ, DIR_SW)
}

#[inline(always)]
pub fn queen_attacks(sq: usize, occ: u64) -> u64 {
    rook_attacks(sq, occ) | bishop_attacks(sq, occ)
}

#[inline(always)]
pub fn between_mask(from_sq: usize, to_sq: usize) -> u64 {
    BETWEEN[from_sq & 63][to_sq & 63]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slow_rook_attacks(sq: usize, occ: u64) -> u64 {
        let x = (sq % 8) as isize;
        let y = (sq / 8) as isize;
        let mut attacks = 0u64;

        for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
            let mut nx = x + dx;
            let mut ny = y + dy;
            while (0..8).contains(&nx) && (0..8).contains(&ny) {
                let idx = (ny * 8 + nx) as usize;
                attacks |= 1u64 << idx;
                if occ & (1u64 << idx) != 0 {
                    break;
                }
                nx += dx;
                ny += dy;
            }
        }

        attacks
    }

    fn slow_bishop_attacks(sq: usize, occ: u64) -> u64 {
        let x = (sq % 8) as isize;
        let y = (sq / 8) as isize;
        let mut attacks = 0u64;

        for (dx, dy) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let mut nx = x + dx;
            let mut ny = y + dy;
            while (0..8).contains(&nx) && (0..8).contains(&ny) {
                let idx = (ny * 8 + nx) as usize;
                attacks |= 1u64 << idx;
                if occ & (1u64 << idx) != 0 {
                    break;
                }
                nx += dx;
                ny += dy;
            }
        }

        attacks
    }

    #[test]
    fn ray_tables_match_slow_reference() {
        let occupancies = [
            0u64,
            1u64 << 18,
            (1u64 << 18) | (1u64 << 36) | (1u64 << 45),
            (1u64 << 7) | (1u64 << 8) | (1u64 << 63),
        ];

        let mut seed = 0x9e3779b97f4a7c15u64;
        let mut occs: Vec<u64> = occupancies.to_vec();
        for _ in 0..200 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let a = seed;
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            occs.push(a & seed);
            occs.push(a);
        }
        for sq in 0usize..64 {
            for &occ in &occs {
                assert_eq!(rook_attacks(sq, occ), slow_rook_attacks(sq, occ));
                assert_eq!(bishop_attacks(sq, occ), slow_bishop_attacks(sq, occ));
            }
        }
    }

    #[test]
    fn between_masks_cover_only_intermediate_squares() {
        assert_eq!(between_mask(0, 7), 0x7e);
        let a_file_between = [8usize, 16, 24, 32, 40, 48]
            .into_iter()
            .fold(0u64, |acc, sq| acc | (1u64 << sq));
        assert_eq!(between_mask(0, 56), a_file_between);
        assert_eq!(between_mask(27, 45), (1u64 << 36));
        assert_eq!(between_mask(27, 28), 0);
    }
}
