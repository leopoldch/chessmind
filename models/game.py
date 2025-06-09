# ======================== file: chess_game.py ===========================
from __future__ import annotations

from typing import List, Tuple, Dict, Callable

from models.board import ChessBoard
from models.pieces import WHITE, BLACK, ChessPiece, ChessPieceType


class ChessGame:
    def __init__(self, current_turn: str = WHITE):
        self.board = ChessBoard()
        self.board.setup_standard()
        self.current_turn = current_turn
        self.history: List[Tuple[str, str]] = []
        self.result: str | None = None  # "white", "black", "draw" or None

    def make_move(
        self,
        start: str,
        end: str,
        promotion_callback: Callable[[], ChessPieceType] | None = None,
    ) -> bool:
        p: ChessPiece | None = self.board[start]
        if p is None or p.color != self.current_turn:
            return False
        if not self.board.move(start, end, self.current_turn):
            return False
        # handle pawn promotion
        piece = self.board[end]
        if piece and piece.type == ChessPieceType.PAWN:
            _, y = ChessBoard.algebraic_to_index(end)
            if (piece.color == WHITE and y == 7) or (
                piece.color == BLACK and y == 0
            ):
                new_type = (
                    promotion_callback()
                    if promotion_callback is not None
                    else ChessPieceType.QUEEN
                )
                piece.type = new_type

        self.history.append((start, end))
        self.current_turn = BLACK if self.current_turn == WHITE else WHITE
        self._check_game_over()
        return True

    def legal_moves(self) -> Dict[str, List[str]]:
        return self.board.all_legal_moves(self.current_turn)

    def _check_game_over(self) -> None:
        moves = self.legal_moves()
        if moves:
            return
        in_check = self.board.in_check(self.current_turn)
        if in_check:
            self.result = BLACK if self.current_turn == WHITE else WHITE
        else:
            self.result = "draw"

    def __repr__(self) -> str:
        return str(self.board)
