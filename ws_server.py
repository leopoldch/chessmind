# Simple WebSocket interface for the chess engine
from __future__ import annotations

import asyncio
import json
from typing import Optional

import websockets

from models.game import ChessGame
from models.pieces import WHITE, BLACK, ChessPieceType
from san import parse_san
from engine import Engine


async def handle_client(ws: websockets.WebSocketServerProtocol) -> None:
    """Handle a single WebSocket connection."""
    game = ChessGame()
    engine = Engine()

    # Expect first message to indicate AI color ("white" or "black")
    try:
        msg = await ws.recv()
        print(f"Received initial message: {msg}")
    except websockets.ConnectionClosed:
        return
    ai_color = WHITE if msg.strip().lower().startswith("w") else BLACK
    print(f"AI color set to: {'White' if ai_color == WHITE else 'Black'}")

    if ai_color == WHITE:
        start, end = "e2", "e4"
        game.make_move(start, end)
        try:
            await ws.send(start + end)
        except websockets.ConnectionClosed:
            return

    while not game.result:
        try:
            msg = await ws.recv()
        except websockets.ConnectionClosed:
            break
        if len(msg) >= 4 and msg[0] in "abcdefgh" and msg[2] in "12345678":
            opp_move = (msg[:2], msg[2:4])
            promo: Optional[ChessPieceType] = None
        else:
            try:
                start_sq, end_sq, promo = parse_san(game, msg, BLACK if ai_color == WHITE else WHITE)
            except ValueError:
                continue
            opp_move = (start_sq, end_sq)
        game.make_move(opp_move[0], opp_move[1], (lambda p=promo: p) if promo else None)
        if game.result:
            try:
                await ws.send(json.dumps({"result": game.result}))
            except websockets.ConnectionClosed:
                pass
            break
        start, end = engine.best_move(game)
        game.make_move(start, end)
        try:
            await ws.send(start + end)
        except websockets.ConnectionClosed:
            break


async def main() -> None:
    async with websockets.serve(handle_client, "localhost", 8765):
        print("WebSocket server started on ws://localhost:8765")
        await asyncio.Future()  # run forever


if __name__ == "__main__":
    asyncio.run(main())
