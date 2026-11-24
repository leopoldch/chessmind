# Running chessmind as a Lichess bot

This guide takes you from nothing to the engine playing games on
[Lichess](https://lichess.org). The `lichess` binary talks to the Lichess
[Bot API](https://lichess.org/api#tag/Bot): it receives challenges, accepts the
ones that match its settings and plays them on the real clock.

Lichess only allows engines on accounts marked as bots. A bot account is a
separate account that can never play as a human again, so do not use your
personal account.

## 1. Create the bot account

Sign up for a new Lichess account for the bot. A name that says it is a bot
helps other players, for example `yourname_BOT`.

Do not play any game with this account, not even an aborted one: Lichess only
turns an account into a bot if it has never played.

## 2. Create an API token

While logged in as the bot account, open:

<https://lichess.org/account/oauth/token/create?scopes[]=bot:play&description=chessmind>

The link preselects the `bot:play` permission, which is the only one the bot
needs. Create the token and copy it; it starts with `lip_`.

The token gives full control of the bot account. Do not commit it, paste it in
an issue or share it. If it leaks, delete it on
<https://lichess.org/account/oauth/token> and create a new one.

## 3. Give the token to the bot

The bot reads the token from the `LICHESS_TOKEN` environment variable.

For the current terminal only:

```powershell
$env:LICHESS_TOKEN = "lip_..."        # PowerShell
```

```bash
export LICHESS_TOKEN=lip_...          # bash / zsh
```

To keep it on Windows, store it once for your user:

```powershell
setx LICHESS_TOKEN "lip_..."
```

`setx` does not change terminals that are already open. Open a new one, and if
you use the terminal inside VS Code, close and reopen VS Code itself: its
terminals inherit the variables VS Code started with. Check with
`echo $env:LICHESS_TOKEN`.

On Linux or macOS, add the `export` line to your shell profile (`~/.bashrc`,
`~/.zshrc`).

## 4. Turn the account into a bot

```bash
cargo run --release --bin lichess -- upgrade
```

This only checks the token and shows which account it belongs to. If the
account is the right one, run:

```bash
cargo run --release --bin lichess -- upgrade --yes
```

This step is irreversible. The account gets the BOT title and can only play
through the Bot API from now on.

## 5. Start the bot

```bash
cargo run --release --bin lichess
```

It prints the account it is logged in as, its settings and its profile link,
then waits for challenges. Each game is logged with the moves played, the
search depth, the score and the estimated winning chance.

Useful options (all of them are listed by `--help` and in the README):

```bash
# 4 search threads, 512 MB hash, blitz and rapid only, casual games only
cargo run --release --bin lichess -- --threads 4 --hash 512 --speeds blitz,rapid --casual-only
```

## 6. Play a game

Bots cannot look for opponents in the lobby: every game starts from a
challenge.

- From your personal account, open the bot's profile and use the challenge
  button. Pick a standard game with a clock, for example 3+2.
- Or let the bot challenge someone, usually another bot:

  ```bash
  cargo run --release --bin lichess -- --challenge SomeOtherBot --tc 3+2
  ```

  The challenge expires after 20 seconds if it is not accepted.

The bot accepts standard chess, including games from a custom position, at
bullet, blitz, rapid and classical speeds. It declines other variants,
correspondence and games without a clock, and tells Lichess why, so the
challenger sees the reason.

## 7. Stop the bot

Press `Ctrl+C` in the terminal where it runs.

Stop it between games. If you stop it during a game, the game goes on without
it and its clock keeps running. Start the bot again before the clock runs out
and it picks the game up where it was, because Lichess sends the games in
progress when the bot reconnects.

## Troubleshooting

| Message or symptom | Cause and fix |
| --- | --- |
| `LICHESS_TOKEN is not set` | The terminal does not see the variable. See step 3, including the VS Code restart. |
| `Lichess rejected the token (HTTP 401)` | The token is wrong, was deleted, or was copied with a typo. Create a new one. |
| `... is not a BOT account` | Run step 4. |
| `Upgrade refused` | The account has already played a game. Create a new account. |
| `Event stream closed ... reconnecting` repeatedly | Another bot process uses the same token. Only one can run at a time. |
| The bot loses on time | Raise `--move-overhead` (milliseconds kept back per move for network lag, 300 by default). |
| `Rate limited by Lichess` | Too many requests in a short time. The bot waits one minute and carries on. |
