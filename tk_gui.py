import os
import sys
import tkinter as tk

sys.path.append(os.path.join(os.path.dirname(__file__), "models"))

from game import ChessGame
from board import ChessBoard
from pieces import ChessPieceType, WHITE, BLACK, ChessPiece

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
                    self.canvas.create_text(
                        x * self.square_size + self.square_size / 2,
                        (7 - y) * self.square_size + self.square_size / 2,
                        text=PIECE_UNICODE[(piece.color, piece.type)],
                        font=("Arial", int(self.square_size / 1.2)),
                        tags="piece",
                    )

    def xy_to_square(self, x: int, y: int) -> str:
        file = x // self.square_size
        rank = 7 - (y // self.square_size)
        return ChessBoard.index_to_algebraic(file, rank)

    def on_press(self, event) -> None:
        square = self.xy_to_square(event.x, event.y)
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
            # Hide piece on board during drag
            self.game.board[square] = None
            self.draw_board()
            self.draw_pieces()

    def on_drag(self, event) -> None:
        if self.drag_item:
            self.canvas.coords(self.drag_item, event.x, event.y)

    def on_release(self, event) -> None:
        if not self.drag_item:
            return
        target_square = self.xy_to_square(event.x, event.y)
        success = self.game.make_move(self.drag_start_square, target_square)
        if not success:
            # restore piece if move illegal
            piece = self.game.board[self.drag_start_square]
            if piece is None:
                # piece was removed temporarily
                orig_piece = self.get_piece_from_unicode(
                    self.canvas.itemcget(self.drag_item, "text")
                )
                self.game.board[self.drag_start_square] = orig_piece
        self.canvas.delete(self.drag_item)
        self.drag_item = None
        self.drag_start_square = ""
        self.draw_board()
        self.draw_pieces()

    @staticmethod
    def get_piece_from_unicode(char: str) -> ChessPiece:
        for (color, ptype), symbol in PIECE_UNICODE.items():
            if symbol == char:
                return ChessPiece(ptype, color, (0, 0))
        raise ValueError("Unknown piece unicode")

    def run(self) -> None:
        self.root.mainloop()


if __name__ == "__main__":
    ChessGUI().run()

