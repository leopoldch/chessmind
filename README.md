# chessmind

Rust implementation of a simple chess engine. This crate contains the core engine logic used by the Firefox extension in `firefox_extension/`.

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
    let engine = Engine::new(3);
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
one square to another.
