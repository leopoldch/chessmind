# Chess AI engine implementing negamax with alpha-beta pruning
# and simple evaluation.

from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, Tuple, Optional, List, DefaultDict

import random
import time
from collections import defaultdict

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
        self.tt: Dict[int, TransEntry] = {}
        self.eval_cache: Dict[Tuple[int, str], int] = {}

        rng = random.Random(42)
        # mapping from (color, piece type) -> index
        self._piece_index = {}
        idx = 0
        for c in (WHITE, BLACK):
            for t in ChessPieceType:
                self._piece_index[(c, t)] = idx
                idx += 1
        self._z_table: List[List[List[int]]] = [
            [[rng.getrandbits(64) for _ in range(8)] for _ in range(8)]
            for _ in range(len(self._piece_index))
        ]
        self._z_castling = [[rng.getrandbits(64) for _ in range(2)] for _ in range(2)]
        self._z_ep = [rng.getrandbits(64) for _ in range(8)]
        self._z_turn = rng.getrandbits(64)

        self.killer_moves: List[List[Optional[Tuple[str, str]]]] = [
            [None, None] for _ in range(self.depth + 5)
        ]
        self.history_table: DefaultDict[Tuple[str, str], int] = defaultdict(int)

    def _order_moves(
        self,
        game: ChessGame,
        moves: Dict[str, list[str]],
        tt_move: Optional[Tuple[str, str]] = None,
        ply: int = 0,
    ) -> list[Tuple[str, str]]:
        ordered: list[Tuple[int, Tuple[str, str]]] = []
        for start, ends in moves.items():
            for end in ends:
                score = self.history_table[(start, end)]
                if tt_move and (start, end) == tt_move:
                    score += 10_000
                if game.board[end] is not None:
                    score += 5_000
                if (start, end) in self.killer_moves[ply]:
                    score += 7_000
                ordered.append((score, (start, end)))
        ordered.sort(key=lambda x: x[0], reverse=True)
        return [m for _, m in ordered]

    # -------- state hashing ---------
    def _board_hash(self, board: ChessBoard) -> int:
        h = 0
        for y in range(8):
            for x in range(8):
                p = board.board[y][x]
                if p:
                    idx = self._piece_index[(p.color, p.type)]
                    h ^= self._z_table[idx][y][x]
        for c_idx, c in enumerate((WHITE, BLACK)):
            rights = board.castling_rights[c]
            if rights["K"]:
                h ^= self._z_castling[c_idx][0]
            if rights["Q"]:
                h ^= self._z_castling[c_idx][1]
        if board.en_passant_target:
            h ^= self._z_ep[board.en_passant_target[0]]
        return h

    def _hash(self, game: ChessGame) -> int:
        h = self._board_hash(game.board)
        if game.current_turn == WHITE:
            h ^= self._z_turn
        return h

    # -------- evaluation -----------
    def evaluate(self, board: ChessBoard, color: str) -> int:
        key = (self._board_hash(board), color)
        if key in self.eval_cache:
            return self.eval_cache[key]
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
        self.eval_cache[key] = value
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
    def negamax(self, game: ChessGame, depth: int, alpha: int, beta: int, ply: int = 0) -> Tuple[int, Optional[Tuple[str, str]]]:
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
        ordered_moves = self._order_moves(game, moves, tt_move, ply)
        for i, (start, end) in enumerate(ordered_moves):
            is_capture = game.board[end] is not None
            new_game = self._clone_game(game)
            new_game.make_move(start, end)
            reduction = 0
            if depth > 2 and i >= 3 and not is_capture:
                reduction = 1
            if reduction:
                score, _ = self.negamax(new_game, depth - 1 - reduction, -alpha - 1, -alpha, ply + 1)
                score = -score
                if score > alpha:
                    score, _ = self.negamax(new_game, depth - 1, -beta, -alpha, ply + 1)
                    score = -score
            else:
                score, _ = self.negamax(new_game, depth - 1, -beta, -alpha, ply + 1)
                score = -score
            if score > best_score:
                best_score = score
                best_move = (start, end)
            if best_score > alpha:
                alpha = best_score
            if alpha >= beta:
                if not is_capture:
                    km = self.killer_moves[ply]
                    if (start, end) not in km:
                        km.pop()
                        km.insert(0, (start, end))
                self.history_table[(start, end)] += depth * depth
                break

        self.tt[key] = TransEntry(depth, best_score, best_move)
        return best_score, best_move

    def best_move(self, game: ChessGame) -> Tuple[str, str]:
        start_time = time.perf_counter()
        guess = 0
        best_move: Optional[Tuple[str, str]] = None
        for d in range(1, self.depth + 1):
            window = 50
            alpha = guess - window
            beta = guess + window
            while True:
                score, move = self.negamax(game, d, alpha, beta)
                if score <= alpha:
                    alpha -= window
                    window *= 2
                    continue
                if score >= beta:
                    beta += window
                    window *= 2
                    continue
                break
            guess = score
            if move:
                best_move = move
        end = time.perf_counter()
        print(f"AI search depth {self.depth} took {end - start_time:.2f}s")
        assert best_move is not None
        return best_move
