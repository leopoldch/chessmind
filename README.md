# chessmind

Rust implementation of a simple chess engine. This crate contains the core engine logic used by the Firefox extension in `firefox_extension/`. The engine uses Principal Variation Search (PVS) with quiescence search and keeps a transposition table backed by an LRU cache to reuse previous evaluations.
To avoid draws by repetition, game states are tracked and the AI skips moves that would repeat the same position a third time. The search can run on multiple threads thanks to a simple Lazy-SMP implementation.


## Warning 

This implementation is indeed not perfect and could be improved a lot; This programs was just a experiment.
Apprx ELO when tested on chess.com: 2000

## Building

Install Rust from [rust-lang.org](https://www.rust-lang.org/tools/install) and run:

```bash
cargo build --release
```

## Running tests

```bash
cargo test
```

## Example usage

The engine exposes simple structures to manipulate a chess game. A best move can be searched with PVS as follows:

```rust
use chessmind::{game::Game, engine::Engine};

fn main() {
    let mut game = Game::new();
    let mut engine = Engine::from_env(3, 4); // depth 3 using 4 threads by default
    if let Some((from, to)) = engine.best_move(&mut game) {
        println!("{} -> {}", from, to);
    }
}
```

### Opening book

To stabilise the engine's play in the first moves (and quickly reach roughly 1000 Elo without extra tuning), the engine now
ships with a tiny built-in opening book covering a handful of solid classical systems (Italian, Queen's Gambit Declined,
Sicilian, English, King's Indian, French, and Caro-Kann setups). If the current game history matches one of the book
lines, the next move is played instantly instead of searching, preventing early blunders and saving time for the middlegame.

### Optional tuning via environment variables

The engine can be configured without code changes via environment variables:

| Variable | Description | Default |
| --- | --- | --- |
| `CHESSMIND_DEPTH` | Search depth in plies. | Value passed to `from_env` (e.g. `6`). |
| `CHESSMIND_THREADS` | Number of worker threads for Lazy-SMP. | Value passed to `from_env` (e.g. all logical cores). |
| `CHESSMIND_TT_SIZE` | Transposition table size (number of entries). | `4_194_304`. |
| `SYZYGY_PATH` | Path to Syzygy tablebases to enable endgame probing. | Disabled if not set. |

## Graphical interface

If you prefer playing locally without the WebSocket server, a simple GUI is
available. Launch it with:

```bash
cargo run --bin gui
```

The board appears in a new window and you can move pieces by dragging them from
one square to another. A checkbox at the top lets you enable a simple AI
opponent and choose whether it plays White or Black.
