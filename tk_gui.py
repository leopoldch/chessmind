from __future__ import annotations

import os
import sys
import tkinter as tk

sys.path.append(os.path.join(os.path.dirname(__file__), "models"))

from game import ChessGame
from board import ChessBoard
from pieces import ChessPieceType, WHITE, BLACK

PIECE_UNICODE = {
    (WHITE, ChessPieceType.KING): "\u2654",
    (WHITE, ChessPieceType.QUEEN): "\u2655",
    (WHITE, ChessPieceType.ROOK): "\u2656",
    (WHITE, ChessPieceType.BISHOP): "\u2657",
    (WHITE, ChessPieceType.KNIGHT): "\u2658",
    (WHITE, ChessPieceType.PAWN): "\u2659",
    (BLACK, ChessPieceType.KING): "\u265A",
    (BLACK, ChessPieceType.QUEEN): "\u265B",
    (BLACK, ChessPieceType.ROOK): "\u265C",
    (BLACK, ChessPieceType.BISHOP): "\u265D",
    (BLACK, ChessPieceType.KNIGHT): "\u265E",
    (BLACK, ChessPieceType.PAWN): "\u265F",
}


class ChessGUI:
    def __init__(self) -> None:
        self.game = ChessGame()
        self.game.board.setup_standard()

        self.root = tk.Tk()
        self.root.title("ChessMind")
        self.square_size = 60
        self.canvas = tk.Canvas(
            self.root,
            width=self.square_size * 8,
            height=self.square_size * 8,
        )
        self.canvas.pack()

        self.drag_item = None
        self.drag_start_square = ""

        self.draw_board()
        self.draw_pieces()

        self.canvas.bind("<ButtonPress-1>", self.on_press)
        self.canvas.bind("<B1-Motion>", self.on_drag)
        self.canvas.bind("<ButtonRelease-1>", self.on_release)

    def draw_board(self) -> None:
        self.canvas.delete("square")
        for y in range(8):
            for x in range(8):
                color = "#EEEED2" if (x + y) % 2 == 0 else "#769656"
                self.canvas.create_rectangle(
                    x * self.square_size,
                    (7 - y) * self.square_size,
                    (x + 1) * self.square_size,
                    (8 - y) * self.square_size,
                    fill=color,
                    tags="square",
                )

    def draw_pieces(self) -> None:
        self.canvas.delete("piece")
        for y in range(8):
            for x in range(8):
                piece = self.game.board.board[y][x]
                if piece:
                    square = ChessBoard.index_to_algebraic(x, y)
                    self.canvas.create_text(
                        x * self.square_size + self.square_size / 2,
                        (7 - y) * self.square_size + self.square_size / 2,
                        text=PIECE_UNICODE[(piece.color, piece.type)],
                        font=("Arial", int(self.square_size / 1.2)),
                        tags=("piece", square),
                    )

    def xy_to_square(self, x: int, y: int) -> str | None:
        file = x // self.square_size
        rank = 7 - (y // self.square_size)
        if 0 <= file < 8 and 0 <= rank < 8:
            return ChessBoard.index_to_algebraic(file, rank)
        return None

    def on_press(self, event) -> None:
        square = self.xy_to_square(event.x, event.y)
        if square is None:
            return
        piece = self.game.board[square]
        if piece and piece.color == self.game.current_turn:
            self.drag_start_square = square
            self.drag_item = self.canvas.create_text(
                event.x,
                event.y,
                text=PIECE_UNICODE[(piece.color, piece.type)],
                font=("Arial", int(self.square_size / 1.2)),
                tags="drag",
            )
            # Hide piece on board during drag (visual only)
            self.canvas.delete(square)

    def on_drag(self, event) -> None:
        if self.drag_item:
            self.canvas.coords(self.drag_item, event.x, event.y)

    def on_release(self, event) -> None:
        if not self.drag_item:
            return
        target_square = self.xy_to_square(event.x, event.y)
        if target_square:
            self.game.make_move(self.drag_start_square, target_square)
        self.canvas.delete(self.drag_item)
        self.drag_item = None
        self.drag_start_square = ""
        self.draw_board()
        self.draw_pieces()

    def run(self) -> None:
        self.root.mainloop()


if __name__ == "__main__":
    ChessGUI().run()

