# =========================== file: pieces.py ============================
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Tuple

WHITE: str = "white"
BLACK: str = "black"


class ChessPieceType(Enum):
    PAWN = "Pawn"
    KNIGHT = "Knight"
    BISHOP = "Bishop"
    ROOK = "Rook"
    QUEEN = "Queen"
    KING = "King"


@dataclass
class ChessPiece:
    type: ChessPieceType
    color: str  # "white" or "black"
    position: Tuple[int, int]  # (x, y) indices 0‑based

    def __repr__(self) -> str:
        from chess_board import ChessBoard  # local import to avoid cycle at runtime
        return f"{self.color} {self.type.value}({ChessBoard.index_to_algebraic(*self.position)})"
