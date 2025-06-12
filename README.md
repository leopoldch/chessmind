# chessmind

Rust implementation of a simple chess engine. This crate contains the core engine logic used by the Firefox extension in `firefox_extension/`. The engine keeps a transposition table backed by an LRU cache to reuse previous evaluations and speed up searches.
To avoid draws by repetition, game states are tracked and the AI skips moves that would repeat the same position a third time. The search can run on multiple threads thanks to a simple Lazy-SMP implementation.

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

The engine exposes simple structures to manipulate a chess game. A best move can be searched with alpha-beta as follows:

```rust
use chessmind::{game::Game, engine::Engine};

fn main() {
    let mut game = Game::new();
    let mut engine = Engine::with_threads(3, 4); // depth 3 using 4 threads
    if let Some((from, to)) = engine.best_move(&mut game) {
        println!("{} -> {}", from, to);
    }
}
```

## Graphical interface

If you prefer playing locally without the WebSocket server, a simple GUI is
available. Launch it with:

```bash
cargo run --bin gui
```

The board appears in a new window and you can move pieces by dragging them from
one square to another. A checkbox at the top lets you enable a simple AI
opponent and choose whether it plays White or Black.
