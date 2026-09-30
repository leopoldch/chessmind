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

## Online chess.com (please do not abuse)

```bash
cargo run --release --bin ws_server
```
Then load the browser extension.
Important: please do not test against real players.

## Graphical interface

If you prefer playing locally without the WebSocket server, a simple GUI is
available. Launch it with:

```bash
cargo run --release --bin gui
```

The board appears in a new window and you can move pieces by dragging them from
one square to another. A checkbox at the top lets you enable a simple AI
opponent and choose whether it plays White or Black.

## Measuring Elo changes (UCI + self-play)

`uci` is a UCI front-end for the engine (options `Hash`, `Threads`, `OwnBook`;
supports `position startpos|fen ... moves ...`, `go wtime/btime/winc/binc/movestogo/movetime/depth/infinite`,
`stop`). It can also be loaded in any UCI GUI or in cutechess-cli.

`selfplay` plays two UCI executables against each other. Every opening is
played as a **pair** of games with colours swapped (both games on the same
worker), and the runner checks legality itself (illegal move, crash or time
forfeit = loss) and adjudicates mate, stalemate, threefold repetition, the
50-move rule, insufficient material and a ply cap. Repetitions and the 50-move
counter start from the opening position (FEN halfmove clock included).

```bash
# build the candidate, and keep a copy of the baseline binary
cargo build --release --bin uci --bin selfplay
cp target/release/uci /tmp/uci-base        # e.g. built from main

# SPRT for a small gain (+0 vs +5 Elo), stops as soon as a bound is crossed
./target/release/selfplay --engine1 target/release/uci --engine2 /tmp/uci-base \
    --openings openings/balanced.epd --games 20000 --tc 10000+100 --no-book \
    --sprt --elo0 0 --elo1 5 --quiet --pgn /tmp/match.pgn

# fixed-length match: Elo +/- 95% from the pentanomial variance
./target/release/selfplay --engine1 target/release/uci --engine2 /tmp/uci-base \
    --openings openings/balanced.epd --games 2000 --tc 5000+50 --no-book --quiet
```

Openings: `--openings FILE` reads one FEN or EPD per line (EPD without move
counters is fine; `hmvc`/`fmvn` operations are honoured; blank lines and `#`
comments are skipped). `--openings-order random|sequential` (default random,
reshuffled every pass through the file) and `--seed N` (printed at start, so a
run can be reproduced). Without `--openings` the 52 built-in lines of
`openings/builtin.txt` are used.

Statistics (engine1's point of view):

* **Pentanomial**: each pair scores 0, 0.25, 0.5, 0.75 or 1 (LL, LD, DD/WL,
  WD, WW). The Elo error bar uses the per-pair variance, which accounts for
  the correlation between the two games of an opening (a lopsided opening
  gives WL/LW pairs that cancel out) and is tighter than the trinomial one.
  Also reported: normalized Elo (nElo, fishtest convention) and LOS.
* **SPRT** (`--sprt --elo0 E0 --elo1 E1 --alpha A --beta B`, defaults
  0/5/0.05/0.05): logistic Elo bounds, generalized SPRT with the normal
  approximation used by fishtest,
  `LLR = N_pairs * (s1 - s0) * (2*mean - s0 - s1) / (2 * var_pair)`, where
  s0/s1 are the expected scores at E0/E1 and empty pentanomial cells get a
  count of 1e-3 (regularization). When the LLR crosses `ln(B/(1-A))` (H0
  accepted: no gain of E1) or `ln((1-B)/A)` (H1 accepted: gain), no new pair
  is started and the pairs in progress are finished. No verdict is taken
  before `--sprt-min-pairs` complete pairs (default 20): with very few pairs
  the variance estimate is degenerate and the LLR meaningless.
* **Trinomial** W/D/L, Elo and LLR treating games as independent are still
  printed for reference.

Typical bounds: `--elo0 0 --elo1 5` for a small search/eval tweak (can take
several thousand pairs), `--elo0 -3 --elo1 1` for a non-regression check of a
simplification or speed-up.

Other flags: `--tc BASE_MS+INC_MS` or `--movetime MS` for both engines;
`--tc1 SPEC` / `--tc2 SPEC` per engine for time-odds tests (SPEC is
`BASE_MS+INC_MS`, `movetime=MS` or `depth=N`); `--games N` (rounded up to
whole pairs) or `--pairs N`; `--concurrency K` (parallel pairs, default half
the physical cores, both engines single-threaded); `--threads T` and
`--hash MB` (sent to both engines); `--no-book` (sets `OwnBook false`; FEN
starts never use the book); `--pgn FILE` (games with `SetUp`/`FEN` tags);
`--quiet` (a compact status line every `--status-interval` seconds, default
10, instead of one line per game); `--max-plies N` (default 400);
`--timemargin MS` (clock tolerance, default 100); `--name1/--name2`.

### Regenerating the opening book

`openings/balanced.epd` is generated by `genbook`: it extends the start
position and the 52 built-in lines with 2-6 random plies chosen among moves
within 60 cp of the best one (depth-4 search), deduplicates the end positions,
drops positions in check and keeps those a depth-7 search scores within
+/-80 cp for the side to move. The output only depends on `--seed`.

```bash
cargo build --release --bin genbook
./target/release/genbook --out openings/balanced.epd --walks-per-seed 100 --seed 1
```

Options: `--min-plies/--max-plies`, `--walk-depth`, `--walk-margin CP`,
`--depth`, `--max-eval CP`, `--count N` (truncate), `--threads N`.
