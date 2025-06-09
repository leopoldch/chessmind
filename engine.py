# Chess AI engine implementing negamax with alpha-beta pruning
# and simple evaluation.

from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, Tuple, Optional

from models.game import ChessGame
from models.board import ChessBoard
from models.pieces import (
    ChessPieceType,
    WHITE,
    BLACK,
)

PIECE_VALUES = {
    ChessPieceType.PAWN: 1,
    ChessPieceType.KNIGHT: 3,
    ChessPieceType.BISHOP: 3,
    ChessPieceType.ROOK: 5,
    ChessPieceType.QUEEN: 8,
    ChessPieceType.KING: 0,
}

CENTER_SQUARES = {
    (3, 3),
    (3, 4),
    (4, 3),
    (4, 4),
}

@dataclass
class TransEntry:
    depth: int
    score: int
    move: Tuple[str, str]


class Engine:
    def __init__(self, depth: int = 3) -> None:
        self.depth = depth
        self.tt: Dict[str, TransEntry] = {}

    def _order_moves(
        self,
        game: ChessGame,
        moves: Dict[str, list[str]],
        tt_move: Optional[Tuple[str, str]] = None,
    ) -> list[Tuple[str, str]]:
        ordered: list[Tuple[int, Tuple[str, str]]] = []
        for start, ends in moves.items():
            for end in ends:
                score = 0
                if tt_move and (start, end) == tt_move:
                    score = 2
                elif game.board[end] is not None:
                    score = 1
                ordered.append((score, (start, end)))
        ordered.sort(key=lambda x: x[0], reverse=True)
        return [m for _, m in ordered]

    # -------- state hashing ---------
    def _hash(self, game: ChessGame) -> str:
        board = game.board
        rows = []
        for y in range(8):
            for x in range(8):
                p = board.board[y][x]
                if p is None:
                    rows.append(".")
                else:
                    c = p.type.name[0]
                    rows.append(c.upper() if p.color == WHITE else c.lower())
        castling = (
            ("K" if board.castling_rights[WHITE]["K"] else "")
            + ("Q" if board.castling_rights[WHITE]["Q"] else "")
            + ("k" if board.castling_rights[BLACK]["K"] else "")
            + ("q" if board.castling_rights[BLACK]["Q"] else "")
        )
        ep = (
            ChessBoard.index_to_algebraic(*board.en_passant_target)
            if board.en_passant_target
            else "-"
        )
        return "".join(rows) + game.current_turn[0] + castling + ep

    # -------- evaluation -----------
    def evaluate(self, board: ChessBoard, color: str) -> int:
        value = 0
        for y in range(8):
            for x in range(8):
                p = board.board[y][x]
                if p:
                    mul = 1 if p.color == color else -1
                    value += PIECE_VALUES[p.type] * mul
                    if (x, y) in CENTER_SQUARES:
                        value += mul
        # simple king safety: reward castled positions
        wk = board._king_square(WHITE)
        bk = board._king_square(BLACK)
        if wk in ("g1", "c1"):
            value += 1 if color == WHITE else -1
        if bk in ("g8", "c8"):
            value += 1 if color == BLACK else -1
        return value

    # -------- quiescence search ---------
    def quiescence(self, game: ChessGame, alpha: int, beta: int, color: str) -> int:
        stand = self.evaluate(game.board, color)
        if stand >= beta:
            return beta
        if alpha < stand:
            alpha = stand
        moves = game.board.all_legal_moves(game.current_turn)
        for start, ends in moves.items():
            piece = game.board[start]
            for end in ends:
                target = game.board[end]
                if target is None or target.color == piece.color:
                    continue
                new_game = self._clone_game(game)
                new_game.make_move(start, end)
                score = -self.quiescence(new_game, -beta, -alpha, BLACK if color == WHITE else WHITE)
                if score >= beta:
                    return beta
                if score > alpha:
                    alpha = score
        return alpha

    # -------- clone ---------
    def _clone_game(self, game: ChessGame) -> ChessGame:
        new = ChessGame(game.current_turn)
        new.board = game.board.clone()
        new.history = list(game.history)
        new.result = game.result
        return new

    # -------- negamax search --------
    def negamax(self, game: ChessGame, depth: int, alpha: int, beta: int) -> Tuple[int, Optional[Tuple[str, str]]]:
        key = self._hash(game)
        entry = self.tt.get(key)
        if entry and entry.depth >= depth:
            return entry.score, entry.move

        if depth == 0 or game.result is not None:
            return self.quiescence(game, alpha, beta, game.current_turn), None

        best_score = -10_000
        best_move: Optional[Tuple[str, str]] = None
        moves = game.legal_moves()
        if not moves:
            # checkmate or stalemate
            if game.board.in_check(game.current_turn):
                return -9999 + (self.depth - depth), None
            return 0, None
        tt_move = entry.move if entry else None
        ordered_moves = self._order_moves(game, moves, tt_move)
        for start, end in ordered_moves:
            new_game = self._clone_game(game)
            new_game.make_move(start, end)
            score, _ = self.negamax(new_game, depth - 1, -beta, -alpha)
            score = -score
            if score > best_score:
                best_score = score
                best_move = (start, end)
            if best_score > alpha:
                alpha = best_score
            if alpha >= beta:
                break

        self.tt[key] = TransEntry(depth, best_score, best_move)
        return best_score, best_move

    def best_move(self, game: ChessGame) -> Tuple[str, str]:
        _, move = self.negamax(game, self.depth, -10_000, 10_000)
        assert move is not None
        return move
