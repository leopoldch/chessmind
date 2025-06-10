import asyncio
import json
from typing import Optional

import websockets

from models.game import ChessGame
from models.pieces import WHITE, BLACK, ChessPieceType
from san import parse_san
from engine import Engine


def _is_coordinate(move: str) -> bool:
    """Return True if *move* looks like long algebraic (e2e4)."""
    return (
        len(move) == 4
        and move[0] in "abcdefgh"
        and move[1] in "12345678"
        and move[2] in "abcdefgh"
        and move[3] in "12345678"
    )


async def handle_client(ws: websockets.WebSocketServerProtocol) -> None:
    """Handle a single WebSocket connection."""
    game = ChessGame()
    engine = Engine()
    ai_color: Optional[str] = None
    last_len = 0

    while True:
        try:
            msg = await ws.recv()
            print(f"Received message: {msg}")
        except websockets.ConnectionClosed:
            break

        try:
            data = json.loads(msg)
        except json.JSONDecodeError:
            # Fallback for raw coordinate/SAN moves (legacy)
            if ai_color is None:
                continue
            if _is_coordinate(msg):
                start, end = msg[:2], msg[2:4]
                promo: Optional[ChessPieceType] = None
            else:
                try:
                    start, end, promo = parse_san(
                        game, msg, BLACK if game.current_turn == WHITE else WHITE
                    )
                except ValueError:
                    print(f"Could not parse move '{msg}', ignoring.")
                    continue
            game.make_move(start, end, (lambda p=promo: p) if promo else None)
            last_len += 1
        else:
            if data.get("type") == "color":
                col = str(data.get("color", "white")).lower()
                ai_color = WHITE if col.startswith("w") else BLACK
                print(
                    f"AI colour set to: {'White' if ai_color == WHITE else 'Black'}"
                )
                # reset game in case of reconnect
                game = ChessGame()
                last_len = 0
                if ai_color == WHITE and game.current_turn == WHITE:
                    # Fixed opening move when playing white
                    start, end = "d2", "d4"
                    game.make_move(start, end)
                    last_len = 1
                    await ws.send(start + end)
                continue

            msg_type = data.get("type")
            if msg_type == "move":
                move = str(data.get("move", "")).replace("+", "")
                if _is_coordinate(move):
                    start, end = move[:2], move[2:4]
                    promo: Optional[ChessPieceType] = None
                else:
                    try:
                        start, end, promo = parse_san(game, move, game.current_turn)
                    except ValueError:
                        print(f"Could not parse move '{move}', ignoring.")
                        continue
                game.make_move(start, end, (lambda p=promo: p) if promo else None)
                last_len += 1
            elif msg_type == "moves":
                raw_moves = data.get("moves", [])
                if len(raw_moves) == last_len:
                    continue
                print(f"Received moves: {raw_moves}")
                # rebuild game from scratch to handle reconnects
                game = ChessGame()
                for entry in raw_moves:
                    if isinstance(entry, dict):
                        move_str = str(entry.get("move", "")).replace("+", "")
                        color_str = str(entry.get("color", "white")).lower()
                        color = WHITE if color_str.startswith("w") else BLACK
                    else:
                        move_str = str(entry).replace("+", "")
                        color = game.current_turn
                    try:
                        s, e, promo = parse_san(game, move_str, color)
                    except ValueError:
                        print(f"Could not parse move '{entry}', stopping replay.")
                        return
                    game.make_move(s, e, (lambda p=promo: p) if promo else None)
                last_len = len(raw_moves)
            else:
                print(f"Unknown message type: {msg_type}")
                continue

        if ai_color is None:
            continue
        if game.result:
            await ws.send(json.dumps({"result": game.result}))
            break
        if game.current_turn != ai_color:
            continue

        start, end = engine.best_move(game)
        game.make_move(start, end)
        await ws.send(start + end)


def main() -> None:
    asyncio.run(_main())


async def _main() -> None:
    async with websockets.serve(handle_client, "localhost", 8765):
        print("WebSocket server started on ws://localhost:8765")
        await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(_main())
