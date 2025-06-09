# Simple WebSocket interface for the chess engine
from __future__ import annotations

import asyncio
import json
from typing import Tuple

import websockets

from models.game import ChessGame
from models.pieces import WHITE, BLACK
from engine import Engine


async def handle_client(ws: websockets.WebSocketServerProtocol) -> None:
    """Handle a single WebSocket connection."""
    game = ChessGame()
    engine = Engine()

    # Expect first message to indicate AI color ("white" or "black")
    msg = await ws.recv()
    ai_color = WHITE if msg.strip().lower().startswith("w") else BLACK

    if ai_color == WHITE:
        start, end = engine.best_move(game)
        game.make_move(start, end)
        await ws.send(start + end)

    while not game.result:
        try:
            msg = await ws.recv()
        except websockets.ConnectionClosed:
            break
        if len(msg) < 4:
            continue
        opp_move: Tuple[str, str] = (msg[:2], msg[2:4])
        game.make_move(*opp_move)
        if game.result:
            await ws.send(json.dumps({"result": game.result}))
            break
        start, end = engine.best_move(game)
        game.make_move(start, end)
        await ws.send(start + end)


async def main() -> None:
    async with websockets.serve(handle_client, "localhost", 8765):
        print("WebSocket server started on ws://localhost:8765")
        await asyncio.Future()  # run forever


if __name__ == "__main__":
    asyncio.run(main())
