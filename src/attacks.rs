use once_cell::sync::Lazy;

const DIR_N: usize = 0;
const DIR_S: usize = 1;
const DIR_E: usize = 2;
const DIR_W: usize = 3;
const DIR_NE: usize = 4;
const DIR_NW: usize = 5;
const DIR_SE: usize = 6;
const DIR_SW: usize = 7;

const DIRS: [(isize, isize); 8] = [
    (0, 1),
    (0, -1),
    (1, 0),
    (-1, 0),
    (1, 1),
    (-1, 1),
    (1, -1),
    (-1, -1),
];

const DIR_ASCENDING: [bool; 8] = [true, false, true, false, true, true, false, false];

pub struct SliderTables {
    pub rays: [[u64; 8]; 64],
    pub attacks: [[[u64; 64]; 8]; 64],
    pub between: [[u64; 64]; 64],
}

pub static TABLES: Lazy<SliderTables> = Lazy::new(build_tables);

#[inline(always)]
fn sq_mask(sq: usize) -> u64 {
    1u64 << sq
}

fn build_tables() -> SliderTables {
    let mut rays = [[0u64; 8]; 64];
    let mut attacks = [[[0u64; 64]; 8]; 64];
    let mut between = [[0u64; 64]; 64];

    for from in 0..64 {
        let fx = (from % 8) as isize;
        let fy = (from / 8) as isize;

        for (dir_idx, (dx, dy)) in DIRS.iter().copied().enumerate() {
            let mut x = fx + dx;
            let mut y = fy + dy;
            let mut ray_mask = 0u64;
            let mut prefix = 0u64;

            while (0..8).contains(&x) && (0..8).contains(&y) {
                let sq = (y * 8 + x) as usize;
                let bit = sq_mask(sq);
                ray_mask |= bit;
                attacks[from][dir_idx][sq] = prefix | bit;
                between[from][sq] = prefix;
                prefix |= bit;
                x += dx;
                y += dy;
            }

            rays[from][dir_idx] = ray_mask;
        }
    }

    SliderTables {
        rays,
        attacks,
        between,
    }
}

#[inline(always)]
fn ray_attacks(sq: usize, occ: u64, dir: usize) -> u64 {
    let ray = TABLES.rays[sq][dir];
    let blockers = ray & occ;
    if blockers == 0 {
        return ray;
    }

    let blocker_sq = if DIR_ASCENDING[dir] {
        blockers.trailing_zeros() as usize
    } else {
        63 - blockers.leading_zeros() as usize
    };

    TABLES.attacks[sq][dir][blocker_sq]
}

#[inline(always)]
pub fn rook_attacks(sq: usize, occ: u64) -> u64 {
    ray_attacks(sq, occ, DIR_N)
        | ray_attacks(sq, occ, DIR_S)
        | ray_attacks(sq, occ, DIR_E)
        | ray_attacks(sq, occ, DIR_W)
}

#[inline(always)]
pub fn bishop_attacks(sq: usize, occ: u64) -> u64 {
    ray_attacks(sq, occ, DIR_NE)
        | ray_attacks(sq, occ, DIR_NW)
        | ray_attacks(sq, occ, DIR_SE)
        | ray_attacks(sq, occ, DIR_SW)
}

#[inline(always)]
pub fn queen_attacks(sq: usize, occ: u64) -> u64 {
    rook_attacks(sq, occ) | bishop_attacks(sq, occ)
}

#[inline(always)]
pub fn between_mask(from_sq: usize, to_sq: usize) -> u64 {
    TABLES.between[from_sq][to_sq]
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

        for sq in [0usize, 7, 27, 36, 63] {
            for &occ in &occupancies {
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
