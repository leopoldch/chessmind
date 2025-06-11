# Chess.com Move Reporter Firefox Extension

This extension connects to a local WebSocket server and reports your color and the opponent's latest moves when playing on Chess.com.

## Installation

1. In Firefox, open `about:debugging#/runtime/this-firefox`.
2. Choose **Load Temporary Add-on** and select the `manifest.json` file in this folder.

## Usage

Run the WebSocket server from this repository:

```bash
cargo run --bin ws_server
```

Open a game on Chess.com. The extension waits for the board and move list to
appear, then determines your colour from the board orientation. The opponent's
moves are observed in real time and sent to the WebSocket server at
`ws://localhost:8765`.
