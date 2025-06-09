"""Simple WebSocket interface for the chess engine (fixed)"""
from __future__ import annotations

import asyncio
import json
from typing import Optional

import websockets

from models.game import ChessGame
from models.pieces import WHITE, BLACK, ChessPieceType
from san import parse_san
from engine import Engine


# ------------------------------
# Helper
# ------------------------------

def _is_coordinate(move: str) -> bool:
    """Return True if *move* looks like long algebraic (e2e4, g7g8)"""
    return (
        len(move) == 4
        and move[0] in "abcdefgh"
        and move[1] in "12345678"
        and move[2] in "abcdefgh"
        and move[3] in "12345678"
    )


# ------------------------------
# Main handler
# ------------------------------
async def handle_client(ws: websockets.WebSocketServerProtocol) -> None:
    """Handle a single WebSocket connection."""
    game = ChessGame()
    engine = Engine()

    # ── 1. Receive initial colour ─────────────────────────────────────────────
    try:
        msg = await ws.recv()
        print(f"Received initial message: {msg}")
    except websockets.ConnectionClosed:
        return

    ai_color = WHITE if msg.strip().lower().startswith("w") else BLACK
    print(f"AI colour set to: {'White' if ai_color == WHITE else 'Black'}")

    if ai_color == WHITE:
        start, end = "e2", "e4"
        game.make_move(start, end)
        try:
            await ws.send(start + end)  # send as long algebraic e2e4
        except websockets.ConnectionClosed:
            return

    while not game.result:
        # 3‑a. Wait for the human move (opponent of the engine)
        try:
            msg = await ws.recv()
            print(f"Received move: {msg}")
        except websockets.ConnectionClosed:
            break
        if not msg:
            continue

        if _is_coordinate(msg):
            opp_move = (msg[:2], msg[2:4])
            promo: Optional[ChessPieceType] = None
        else:
            try:
                start_sq, end_sq, promo = parse_san(
                    game, msg, BLACK if ai_color == WHITE else WHITE
                )
            except ValueError:
                print(f"Could not parse move '{msg}', ignoring.")
                continue
            opp_move = (start_sq, end_sq)

        game.make_move(opp_move[0], opp_move[1], (lambda p=promo: p) if promo else None)
        print(f"Opponent move applied: {opp_move[0]} to {opp_move[1]}")

        if game.result:
            try:
                await ws.send(json.dumps({"result": game.result}))
            except websockets.ConnectionClosed:
                pass
            break

        start, end = engine.best_move(game)
        game.make_move(start, end)
        print(f"AI move applied: {start} to {end}")
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