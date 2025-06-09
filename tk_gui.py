from __future__ import annotations

import os
import sys
import tkinter as tk
from tkinter import messagebox


from models.game import ChessGame
from models.board import ChessBoard
from models.pieces import ChessPieceType, WHITE, BLACK
from engine import Engine

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
        self.ai_mode = messagebox.askyesno("Game Mode", "Play against AI?")
        self.ai_color = BLACK
        if self.ai_mode:
            if messagebox.askyesno("Choose Color", "Should AI play White?"):
                self.ai_color = WHITE
            self.engine = Engine(depth=5)
        else:
            self.engine = None
        self.square_size = 60
        self.canvas = tk.Canvas(
            self.root,
            width=self.square_size * 8,
            height=self.square_size * 8,
        )
        self.canvas.pack()

        self.drag_item = None
        self.drag_start_square = ""
        self.last_move: tuple[str, str] | None = None

        self.draw_board()
        self.draw_pieces()

        self.canvas.bind("<ButtonPress-1>", self.on_press)
        self.canvas.bind("<B1-Motion>", self.on_drag)
        self.canvas.bind("<ButtonRelease-1>", self.on_release)

    def ask_promotion(self) -> ChessPieceType:
        choice = tk.StringVar(value=ChessPieceType.QUEEN.name)
        win = tk.Toplevel(self.root)
        win.title("Promote pawn")
        for t in [
            ChessPieceType.QUEEN,
            ChessPieceType.ROOK,
            ChessPieceType.BISHOP,
            ChessPieceType.KNIGHT,
        ]:
            tk.Radiobutton(
                win,
                text=t.value,
                variable=choice,
                value=t.name,
            ).pack(anchor="w")

        done = tk.BooleanVar(value=False)

        def on_ok() -> None:
            done.set(True)
            win.destroy()

        tk.Button(win, text="OK", command=on_ok).pack()
        win.grab_set()
        win.protocol("WM_DELETE_WINDOW", on_ok)
        self.root.wait_variable(done)
        return ChessPieceType[choice.get()]

    def draw_board(self) -> None:
        self.canvas.delete("square")
        for y in range(8):
            for x in range(8):
                square = ChessBoard.index_to_algebraic(x, y)
                color = "#EEEED2" if (x + y) % 2 == 0 else "#769656"
                if self.last_move and square in self.last_move:
                    color = "#a8e6a3"
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
            moved = self.game.make_move(
                self.drag_start_square, target_square, self.ask_promotion
            )
            if moved:
                self.last_move = (self.drag_start_square, target_square)
            self.check_end_or_ai()
        self.canvas.delete(self.drag_item)
        self.drag_item = None
        self.drag_start_square = ""

    def run(self) -> None:
        self.root.mainloop()

    def check_end_or_ai(self) -> None:
        if self.game.result:
            if self.game.result == "draw":
                messagebox.showinfo("Game Over", "Stalemate: draw")
            else:
                messagebox.showinfo(
                    "Game Over", f"{self.game.result.capitalize()} wins"
                )
        elif self.ai_mode and self.game.current_turn == self.ai_color:
            start, end = self.engine.best_move(self.game)
            self.game.make_move(start, end)
            self.last_move = (start, end)
            if self.game.result:
                if self.game.result == "draw":
                    messagebox.showinfo("Game Over", "Stalemate: draw")
                else:
                    messagebox.showinfo(
                        "Game Over", f"{self.game.result.capitalize()} wins"
                    )
        self.draw_board()
        self.draw_pieces()


if __name__ == "__main__":
    ChessGUI().run()

