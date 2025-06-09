# ======================== file: chess_game.py ===========================
from __future__ import annotations

from typing import List, Tuple, Dict

from board import ChessBoard
from pieces import WHITE, BLACK, ChessPiece


class ChessGame:
    def __init__(self, current_turn: str = WHITE):
        self.board = ChessBoard()
        self.board.setup_standard()
        self.current_turn = current_turn
        self.history: List[Tuple[str, str]] = []

    def make_move(self, start: str, end: str) -> bool:
        p: ChessPiece | None = self.board[start]
        if p is None or p.color != self.current_turn:
            return False
        if not self.board.move(start, end, self.current_turn):
            return False
        self.history.append((start, end))
        self.current_turn = BLACK if self.current_turn == WHITE else WHITE
        return True

    def legal_moves(self) -> Dict[str, List[str]]:
        return self.board.all_legal_moves(self.current_turn)

    def __repr__(self) -> str:
        return str(self.board)
