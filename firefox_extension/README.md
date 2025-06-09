# Chess.com Move Reporter Firefox Extension

This extension connects to a local WebSocket server and reports your color and the opponent's latest moves when playing on Chess.com.

## Installation

1. In Firefox, open `about:debugging#/runtime/this-firefox`.
2. Choose **Load Temporary Add-on** and select the `manifest.json` file in this folder.

## Usage

Run the WebSocket server from this repository:

```bash
python ws_server.py
```

Open a game on Chess.com. The extension will detect whether you're playing White or Black based on the board orientation and will send the opponent's latest moves to the WebSocket server at `ws://localhost:8765`.
