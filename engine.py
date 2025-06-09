# Chess AI engine implementing negamax with alpha-beta pruning
# and simple evaluation.

from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, Tuple, Optional, List, DefaultDict
from concurrent.futures import ThreadPoolExecutor

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
    def __init__(self, depth: int = 3, threads: int = 1) -> None:
        self.depth = depth
        self.threads = threads
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
        board: ChessBoard,
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
                target = board[end]
                if target is not None:
                    attacker = board[start]
                    score += 5_000
                    score += PIECE_VALUES[target.type] * 100 - PIECE_VALUES[attacker.type]
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

    def _hash(self, board: ChessBoard, color: str) -> int:
        h = self._board_hash(board)
        if color == WHITE:
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
    def quiescence(self, board: ChessBoard, alpha: int, beta: int, color: str) -> int:
        stand = self.evaluate(board, color)
        if stand >= beta:
            return beta
        if alpha < stand:
            alpha = stand
        moves = board.all_legal_moves(color)
        for start, ends in moves.items():
            piece = board[start]
            for end in ends:
                target = board[end]
                if target is None or target.color == piece.color:
                    continue
                state = board.make_move_state(start, end)
                next_color = BLACK if color == WHITE else WHITE
                score = -self.quiescence(board, -beta, -alpha, next_color)
                board.unmake_move(state)
                if score >= beta:
                    return beta
                if score > alpha:
                    alpha = score
        return alpha

    # -------- negamax search --------
    def negamax(self, board: ChessBoard, color: str, depth: int, alpha: int, beta: int, ply: int = 0) -> Tuple[int, Optional[Tuple[str, str]]]:
        key = self._hash(board, color)
        entry = self.tt.get(key)
        if entry and entry.depth >= depth:
            return entry.score, entry.move

        if depth == 0:
            return self.quiescence(board, alpha, beta, color), None

        best_score = -10_000
        best_move: Optional[Tuple[str, str]] = None
        moves = board.all_legal_moves(color)
        if not moves:
            # checkmate or stalemate
            if board.in_check(color):
                return -9999 + (self.depth - depth), None
            return 0, None
        tt_move = entry.move if entry else None
        ordered_moves = self._order_moves(board, moves, tt_move, ply)
        for i, (start, end) in enumerate(ordered_moves):
            is_capture = board[end] is not None
            state = board.make_move_state(start, end)
            reduction = 0
            if depth > 2 and i >= 3 and not is_capture:
                reduction = 1
            if reduction:
                score, _ = self.negamax(board, BLACK if color == WHITE else WHITE, depth - 1 - reduction, -alpha - 1, -alpha, ply + 1)
                score = -score
                if score > alpha:
                    score, _ = self.negamax(board, BLACK if color == WHITE else WHITE, depth - 1, -beta, -alpha, ply + 1)
                    score = -score
            else:
                score, _ = self.negamax(board, BLACK if color == WHITE else WHITE, depth - 1, -beta, -alpha, ply + 1)
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
                board.unmake_move(state)
                break

            board.unmake_move(state)

        self.tt[key] = TransEntry(depth, best_score, best_move)
        return best_score, best_move

    def _negamax_root_parallel(
        self,
        board: ChessBoard,
        color: str,
        depth: int,
        alpha: int,
        beta: int,
    ) -> Tuple[int, Optional[Tuple[str, str]]]:
        moves = board.all_legal_moves(color)
        ordered = self._order_moves(board, moves, None, 0)
        if not ordered:
            return 0, None
        results: List[Tuple[int, Tuple[str, str]]] = []
        next_color = BLACK if color == WHITE else WHITE
        with ThreadPoolExecutor(max_workers=self.threads) as ex:
            futs = []
            boards = []
            for start, end in ordered:
                clone = board.clone()
                clone._apply_move(start, end)
                futs.append(ex.submit(self.negamax, clone, next_color, depth - 1, -beta, -alpha, 1))
                boards.append((start, end))
            for fut, move in zip(futs, boards):
                score, _ = fut.result()
                score = -score
                results.append((score, move))
        if not results:
            return 0, None
        best_score, best_move = max(results, key=lambda x: x[0])
        return best_score, best_move

    def best_move(self, game: ChessGame) -> Tuple[str, str]:
        start_time = time.perf_counter()
        guess = 0
        best_move: Optional[Tuple[str, str]] = None
        moves_root = game.board.all_legal_moves(game.current_turn)
        move_count = sum(len(v) for v in moves_root.values())
        max_depth = self.depth + 1 if move_count <= 10 else self.depth
        for d in range(1, max_depth + 1):
            window = 50
            alpha = guess - window
            beta = guess + window
            while True:
                if self.threads > 1 and d == max_depth:
                    score, move = self._negamax_root_parallel(game.board, game.current_turn, d, alpha, beta)
                else:
                    score, move = self.negamax(game.board, game.current_turn, d, alpha, beta)
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
        print(f"AI search depth {max_depth} took {end - start_time:.2f}s")
        assert best_move is not None
        return best_move
