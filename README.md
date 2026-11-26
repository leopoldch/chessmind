# chessmind

Rust implementation of a simple chess engine. This crate contains the core engine logic used by the Firefox extension in `firefox_extension/`. The engine uses Principal Variation Search (PVS) with quiescence search and a lockless transposition table.
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

The WebSocket move response includes `next_move`, `time_ms`, and `win_percent`:

```json
{"next_move":"e2e4","time_ms":125,"win_percent":54.6}
```

`win_percent` is the engine's estimated winning chance (0–100) for the side
playing `next_move`. It defaults to `50.0` when no search score is available,
including opening-book and forced opening moves.

## Playing on Lichess

The `lichess` binary connects the engine to a Lichess BOT account through the
[Bot API](https://lichess.org/api#tag/Bot). It accepts the challenges that match
its settings and plays them on the real clock. Engines are allowed on Lichess
only on accounts marked as bots, so the setup needs a dedicated account. The
step-by-step guide, with troubleshooting, is in
[docs/lichess-bot.md](docs/lichess-bot.md). In short:

1. Create a new Lichess account for the bot and do not play any game with it:
   only an account without games can become a bot.
2. Logged in as that account, create a personal API token with the
   `bot:play` scope (this link preselects it):
   <https://lichess.org/account/oauth/token/create?scopes[]=bot:play&description=chessmind>.
   Keep it secret; anyone holding it can play with the account.
3. Put the token in the `LICHESS_TOKEN` environment variable:

   ```bash
   export LICHESS_TOKEN=lip_...          # bash
   $env:LICHESS_TOKEN = "lip_..."        # PowerShell
   ```

4. Turn the account into a bot. This step is irreversible:

   ```bash
   cargo run --release --bin lichess -- upgrade        # explains what happens
   cargo run --release --bin lichess -- upgrade --yes  # does it
   ```

5. Start the bot and challenge it from another account (the bot's profile
   page has a challenge button):

   ```bash
   cargo run --release --bin lichess -- --threads 4 --hash 256
   ```

Bots cannot join the lobby, so games start from challenges only. The bot
declines variants other than standard (from-position games are accepted),
correspondence and unlimited games, and speeds it is not set to play; it
answers with the matching Lichess decline reason. Options:

| Option | Description | Default |
| --- | --- | --- |
| `--threads N` | Search threads per game. | Logical cores divided by `--max-games` |
| `--hash MB` | Transposition table per game. | `256` |
| `--max-games N` | Games played at the same time. Each game has its own threads and hash table. | `1` |
| `--move-overhead MS` | Time kept back on each move for network lag. Raise it if the bot loses on time. | `300` |
| `--speeds LIST` | Accepted speeds among `bullet,blitz,rapid,classical` (Lichess bans UltraBullet for bots). | All four |
| `--casual-only` | Decline rated challenges. | Off |
| `--opponents KIND` | `all`, `humans` or `bots`. | `all` |
| `--no-book` | Do not use the opening book. | Off |
| `--no-ponder` | Do not think on the opponent's time. | Off |
| `--challenge USER` | Challenge `USER` once connected, e.g. another bot. | None |
| `--tc MIN+INC` | Clock of `--challenge`, such as `3+2` or `0.5+0`. | `3+2` |
| `--rated` | Make the `--challenge` rated. | Off |

`SYZYGY_PATH` enables tablebases as for the other front-ends. Only one bot
process can run per token: starting a second one closes the first one's event
stream. If an opponent leaves, the bot claims the win when Lichess allows it.

## Graphical interface

If you prefer playing locally without the WebSocket server, a simple GUI is
available. Launch it with:

```bash
cargo run --release --bin gui
```

Play against the engine or against a friend on the same screen. Move pieces by
dragging them or by clicking the piece and then its target square; legal moves,
the last move and checks are highlighted, and promotions ask which piece to
take. The board turns to your side when you play Black. The side panels hold
the time control, both clocks, the move list in algebraic notation and the
engine's win estimate. Shortcuts: `F` flips the board, `Ctrl/Cmd+Z` takes back
a move, `Ctrl/Cmd+N` starts a new game.

## Measuring strength

```bash
cargo run --release --bin bench            # fixed-depth node count + NPS
cargo build --release --bin uci --bin selfplay
./target/release/selfplay --engine1 target/release/uci --engine2 /path/to/old/uci \
    --openings openings/balanced.epd --tc 1000+20 --no-book --sprt --elo0 0 --elo1 5 --quiet
```

`uci` is a UCI front-end (usable in any UCI GUI). `selfplay` plays colour-swapped
opening pairs and reports pentanomial Elo and SPRT. `openings/balanced.epd` is
generated by `genbook`. `bench` takes an optional depth and thread count
(`bench 16 4`).

### Win probability

`SearchScore::wdl_at(phase)` and `Engine::last_search_wdl()` turn a search score
into win / draw / loss probabilities; `win_percent` is the expected result
(a draw counts one half, so an equal position is 50). The model is fitted on
self-play games and depends on the game phase, because the same score wins far
less often in an endgame than in a middlegame. To refit it after a change to the
evaluation:

```bash
cargo run --release --bin calibrate -- --games 1200 --depth 12 --threads 4
```

It prints the constants to paste into `src/engine.rs` and a table of predicted
against observed results. The probabilities describe a game between two players
of the engine's own strength.
