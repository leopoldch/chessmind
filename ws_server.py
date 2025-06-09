# Simple WebSocket interface for the chess engine
from __future__ import annotations

import asyncio
import json
from typing import Tuple

import websockets

from models.game import ChessGame
from models.pieces import WHITE, BLACK, ChessPieceType
from engine import Engine


def parse_san(game: ChessGame, san: str) -> Tuple[str, str]:
    """Parse a simple SAN move to start/end squares."""
    s = san.strip()
    if not s:
        raise ValueError("Empty move")

    # Handle castling
    if s.startswith("O-O-O"):
        return ("e1" if game.current_turn == WHITE else "e8",
                "c1" if game.current_turn == WHITE else "c8")
    if s.startswith("O-O"):
        return ("e1" if game.current_turn == WHITE else "e8",
                "g1" if game.current_turn == WHITE else "g8")

    # Strip check or mate markers
    while s and s[-1] in "+#":
        s = s[:-1]

    promotion = None
    if "=" in s:
        s, promo = s.split("=")
        promotion = promo  # currently unused

    capture = "x" in s
    if capture:
        left, dest = s.split("x")
    else:
        dest = s[-2:]
        left = s[:-2]

    piece_letter = left[0] if left and left[0] in "KQRBN" else ""
    if piece_letter:
        left = left[1:]
    piece_map = {
        "K": ChessPieceType.KING,
        "Q": ChessPieceType.QUEEN,
        "R": ChessPieceType.ROOK,
        "B": ChessPieceType.BISHOP,
        "N": ChessPieceType.KNIGHT,
    }
    piece_type = piece_map.get(piece_letter, ChessPieceType.PAWN)

    dis_file = None
    dis_rank = None
    if len(left) == 2:
        if left[0] in "abcdefgh":
            dis_file = left[0]
        if left[1] in "12345678":
            dis_rank = left[1]
    elif len(left) == 1:
        if left[0] in "abcdefgh":
            dis_file = left[0]
        else:
            dis_rank = left[0]

    # Find matching legal move
    legal = game.legal_moves()
    for start, ends in legal.items():
        piece = game.board[start]
        if not piece or piece.type != piece_type:
            continue
        if dis_file and start[0] != dis_file:
            continue
        if dis_rank and start[1] != dis_rank:
            continue
        for end in ends:
            if end == dest:
                return start, end

    raise ValueError(f"Illegal SAN: {san}")


async def handle_client(ws: websockets.WebSocketServerProtocol) -> None:
    """Handle a single WebSocket connection."""
    game = ChessGame()
    engine = Engine()

    # Expect first message to indicate AI color ("white" or "black")
    try:
        msg = await ws.recv()
    except websockets.ConnectionClosed:
        return
    ai_color = WHITE if msg.strip().lower().startswith("w") else BLACK

    if ai_color == WHITE:
        start, end = engine.best_move(game)
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
        else:
            try:
                opp_move = parse_san(game, msg)
            except ValueError:
                continue
        game.make_move(*opp_move)
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
