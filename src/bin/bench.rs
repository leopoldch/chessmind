//! Fixed-depth benchmark.
//!
//! Prints the node count and best move for a set of positions, plus total NPS.
//! The total node count is a behaviour signature: a pure speed optimisation must
//! leave it unchanged, while a search change is expected to modify it.
//!
//! Usage: cargo run --release --bin bench -- [depth]

use chessmind::engine::{Engine, TimeConfig};
use chessmind::game::Game;
use std::time::Instant;

const POSITIONS: &[(&str, &str)] = &[
    (
        "ruy_lopez",
        "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5a4 g8f6 e1g1 f8e7 d2d4 e5d4",
    ),
    (
        "najdorf",
        "e2e4 c7c5 g1f3 d7d6 d2d4 c5d4 f3d4 g8f6 b1c3 a7a6 c1e3 e7e5 d4b3 c8e6",
    ),
    (
        "qgd",
        "d2d4 d7d5 c2c4 e7e6 b1c3 g8f6 c1g5 f8e7 e2e3 e8g8 g1f3 b8d7 a1c1 c7c6",
    ),
    (
        "kid",
        "d2d4 g8f6 c2c4 g7g6 b1c3 f8g7 e2e4 d7d6 g1f3 e8g8 f1e2 e7e5 e1g1 b8c6 d4d5 c6e7",
    ),
    (
        "french",
        "e2e4 e7e6 d2d4 d7d5 b1c3 f8b4 e4e5 c7c5 a2a3 b4c3 b2c3 g8e7 d1g4 e8g8",
    ),
    (
        "italian",
        "e2e4 e7e5 g1f3 b8c6 f1c4 f8c5 c2c3 g8f6 d2d3 d7d6 e1g1 a7a6 a2a4 e8g8 f1e1 c5a7",
    ),
];

fn main() {
    let depth: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(10);

    let mut total_nodes = 0u64;
    let start = Instant::now();

    for (name, moves) in POSITIONS {
        let mut game = Game::new();
        for mv in moves.split_whitespace() {
            let (from, to) = mv.split_at(2);
            assert!(game.make_move(from, to), "illegal move {mv} in {name}");
        }

        // A fresh engine per position keeps results independent of order.
        let mut engine = Engine::new(depth);
        let t = Instant::now();
        let result = engine.best_move_timed(&mut game, &TimeConfig::fixed_depth(depth));
        let ms = t.elapsed().as_millis();
        let nodes = engine.last_search_nodes();
        total_nodes += nodes;

        let best = result
            .map(|((f, t), _)| format!("{f}{t}"))
            .unwrap_or_else(|| "none".to_string());
        println!("{name:<10} best {best:<6} nodes {nodes:>10} time {ms:>6} ms");
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!("---");
    println!("depth {depth}");
    println!("total nodes {total_nodes}");
    println!("time {:.2} s", elapsed);
    println!("nps {:.0}", total_nodes as f64 / elapsed.max(1e-9));
}
