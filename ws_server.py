# Simple WebSocket interface for the chess engine
from __future__ import annotations

import asyncio
import json
from typing import Tuple

import websockets
import chess

from models.game import ChessGame
from models.pieces import WHITE, BLACK
from engine import Engine


async def handle_client(ws: websockets.WebSocketServerProtocol) -> None:
    """Handle a single WebSocket connection."""
    game = ChessGame()
    engine = Engine()
    parse_board = chess.Board()

    # Expect first message to indicate AI color ("white" or "black")
    try:
        msg = await ws.recv()
    except websockets.ConnectionClosed:
        return
    ai_color = WHITE if msg.strip().lower().startswith("w") else BLACK

    if ai_color == WHITE:
        start, end = engine.best_move(game)
        game.make_move(start, end)
        parse_board.push(chess.Move.from_uci(start + end))
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
        else:
            try:
                san_move = parse_board.parse_san(msg)
            except ValueError:
                continue
            opp_move = (
                chess.square_name(san_move.from_square),
                chess.square_name(san_move.to_square),
            )
        game.make_move(*opp_move)
        parse_board.push(chess.Move.from_uci("".join(opp_move)))
        if game.result:
            try:
                await ws.send(json.dumps({"result": game.result}))
            except websockets.ConnectionClosed:
                pass
            break
        start, end = engine.best_move(game)
        game.make_move(start, end)
        parse_board.push(chess.Move.from_uci(start + end))
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
