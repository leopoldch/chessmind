# ======================== file: chess_board.py ==========================
"""ChessBoard: 8×8 board with legal‑move logic including castling and
en-passant."""
from __future__ import annotations

from typing import List, Optional, Tuple, Dict
import copy
import string

from models.pieces import ChessPiece, ChessPieceType, WHITE, BLACK

FILES = "abcdefgh"
RANKS = "12345678"


class ChessBoard:
    def __init__(self) -> None:
        # 2‑D array: board[y][x]
        self.board: List[List[Optional[ChessPiece]]] = [[None for _ in range(8)] for _ in range(8)]
        self.en_passant_target: Optional[Tuple[int, int]] = None
        self.castling_rights: Dict[str, Dict[str, bool]] = {
            WHITE: {"K": True, "Q": True},
            BLACK: {"K": True, "Q": True},
        }

    def setup_standard(self) -> None:
        """Place the pieces in the standard chess starting position."""
        order = [
            ChessPieceType.ROOK,
            ChessPieceType.KNIGHT,
            ChessPieceType.BISHOP,
            ChessPieceType.QUEEN,
            ChessPieceType.KING,
            ChessPieceType.BISHOP,
            ChessPieceType.KNIGHT,
            ChessPieceType.ROOK,
        ]
        # Clear board first
        for y in range(8):
            for x in range(8):
                self.board[y][x] = None

        for x, piece_type in enumerate(order):
            self[ChessBoard.index_to_algebraic(x, 0)] = ChessPiece(piece_type, WHITE, (x, 0))
            self[ChessBoard.index_to_algebraic(x, 7)] = ChessPiece(piece_type, BLACK, (x, 7))
            self[ChessBoard.index_to_algebraic(x, 1)] = ChessPiece(ChessPieceType.PAWN, WHITE, (x, 1))
            self[ChessBoard.index_to_algebraic(x, 6)] = ChessPiece(ChessPieceType.PAWN, BLACK, (x, 6))

        self.castling_rights = {
            WHITE: {"K": True, "Q": True},
            BLACK: {"K": True, "Q": True},
        }
        self.en_passant_target = None

    # ── Algebraic helpers ───────────────────────────────────────────
    @staticmethod
    def algebraic_to_index(pos: str) -> Tuple[int, int]:
        if len(pos) != 2 or pos[0] not in FILES or pos[1] not in RANKS:
            raise ValueError(f"Invalid square: {pos}")
        return FILES.index(pos[0]), int(pos[1]) - 1

    @staticmethod
    def index_to_algebraic(x: int, y: int) -> str:
        if not (0 <= x < 8 and 0 <= y < 8):
            raise ValueError(f"Invalid indices: {(x, y)}")
        return f"{FILES[x]}{y + 1}"

    # ── Basic square access ─────────────────────────────────────────
    def __getitem__(self, pos: str) -> Optional[ChessPiece]:
        x, y = self.algebraic_to_index(pos)
        return self.board[y][x]

    def __setitem__(self, pos: str, piece: Optional[ChessPiece]):
        x, y = self.algebraic_to_index(pos)
        self.board[y][x] = piece
        if piece:
            piece.position = (x, y)

    def move_piece_unchecked(self, start: str, end: str) -> None:
        piece = self[start]
        if piece is None:
            raise ValueError(f"No piece on {start}")
        self[end] = piece
        self[start] = None

    def _apply_move(self, start: str, end: str) -> None:
        piece = self[start]
        if piece is None:
            raise ValueError(f"No piece on {start}")
        sx, sy = self.algebraic_to_index(start)
        ex, ey = self.algebraic_to_index(end)
        color = piece.color
        dir_y = 1 if color == WHITE else -1

        # Handle castling
        if piece.type == ChessPieceType.KING and abs(ex - sx) == 2:
            # move rook as well
            if ex > sx:  # kingside
                rook_start = self.index_to_algebraic(7, sy)
                rook_end = self.index_to_algebraic(ex - 1, sy)
            else:  # queenside
                rook_start = self.index_to_algebraic(0, sy)
                rook_end = self.index_to_algebraic(ex + 1, sy)
            self.move_piece_unchecked(rook_start, rook_end)
            self.castling_rights[color]["K"] = False
            self.castling_rights[color]["Q"] = False
        elif piece.type == ChessPieceType.ROOK:
            if sx == 0:
                self.castling_rights[color]["Q"] = False
            elif sx == 7:
                self.castling_rights[color]["K"] = False
        elif piece.type == ChessPieceType.KING:
            self.castling_rights[color]["K"] = False
            self.castling_rights[color]["Q"] = False

        # Handle en-passant capture
        if piece.type == ChessPieceType.PAWN:
            # capture the pawn if move is en-passant
            if self.en_passant_target and (ex, ey) == self.en_passant_target and self.board[ey][ex] is None:
                self.board[ey - dir_y][ex] = None
            # set new en-passant target after double push
            if abs(ey - sy) == 2:
                self.en_passant_target = (sx, sy + dir_y)
            else:
                self.en_passant_target = None
        else:
            self.en_passant_target = None

        self.move_piece_unchecked(start, end)

    def move(self, start: str, end: str, color: str) -> bool:
        if not self.is_legal(start, end, color):
            return False
        self._apply_move(start, end)
        return True

    def clone(self) -> "ChessBoard":
        return copy.deepcopy(self)

    @staticmethod
    def _inside(x: int, y: int) -> bool:
        return 0 <= x < 8 and 0 <= y < 8

    def _add_ray(self, x: int, y: int, dx: int, dy: int, color: str, acc: List[str]):
        nx, ny = x + dx, y + dy
        while self._inside(nx, ny):
            target = self.board[ny][nx]
            if target is None:
                acc.append(self.index_to_algebraic(nx, ny))
            elif target.color != color:
                acc.append(self.index_to_algebraic(nx, ny))
                break
            else:
                break
            nx += dx
            ny += dy


    def pseudo_legal_moves(self, pos: str) -> List[str]:
        x, y = self.algebraic_to_index(pos)
        p = self.board[y][x]
        if p is None:
            return []
        color = p.color
        moves: List[str] = []

        if p.type == ChessPieceType.KNIGHT:
            for dx, dy in [(-2, -1), (-2, 1), (-1, -2), (-1, 2),
                           (1, -2), (1, 2), (2, -1), (2, 1)]:
                nx, ny = x + dx, y + dy
                if self._inside(nx, ny):
                    tgt = self.board[ny][nx]
                    if tgt is None or tgt.color != color:
                        moves.append(self.index_to_algebraic(nx, ny))

        elif p.type == ChessPieceType.BISHOP:
            for dx, dy in [(-1, -1), (-1, 1), (1, -1), (1, 1)]:
                self._add_ray(x, y, dx, dy, color, moves)

        elif p.type == ChessPieceType.ROOK:
            for dx, dy in [(0, 1), (0, -1), (1, 0), (-1, 0)]:
                self._add_ray(x, y, dx, dy, color, moves)

        elif p.type == ChessPieceType.QUEEN:
            for dx, dy in [(-1, -1), (-1, 1), (1, -1), (1, 1),
                           (0, 1), (0, -1), (1, 0), (-1, 0)]:
                self._add_ray(x, y, dx, dy, color, moves)

        elif p.type == ChessPieceType.KING:
            for dx, dy in [(-1, -1), (-1, 0), (-1, 1),
                           (0, -1),          (0, 1),
                           (1, -1), (1, 0), (1, 1)]:
                nx, ny = x + dx, y + dy
                if self._inside(nx, ny):
                    tgt = self.board[ny][nx]
                    if tgt is None or tgt.color != color:
                        moves.append(self.index_to_algebraic(nx, ny))
            rank = 0 if color == WHITE else 7
            rights = self.castling_rights[color]
            if rights["K"]:
                if (
                    self.board[rank][5] is None
                    and self.board[rank][6] is None
                    and (self.board[rank][7]
                         and self.board[rank][7].color == color)
                ):
                    moves.append(self.index_to_algebraic(6, rank))
            if rights["Q"]:
                if (
                    self.board[rank][1] is None
                    and self.board[rank][2] is None
                    and self.board[rank][3] is None
                    and (self.board[rank][0]
                         and self.board[rank][0].color == color)
                ):
                    moves.append(self.index_to_algebraic(2, rank))

        elif p.type == ChessPieceType.PAWN:
            dir_y = 1 if color == WHITE else -1
            start_rank = 1 if color == WHITE else 6
            # forward push
            if self._inside(x, y + dir_y) and self.board[y + dir_y][x] is None:
                moves.append(self.index_to_algebraic(x, y + dir_y))
                if y == start_rank and self.board[y + 2 * dir_y][x] is None:
                    moves.append(self.index_to_algebraic(x, y + 2 * dir_y))
            # captures
            for dx in (-1, 1):
                nx, ny = x + dx, y + dir_y
                if self._inside(nx, ny):
                    tgt = self.board[ny][nx]
                    if tgt and tgt.color != color:
                        moves.append(self.index_to_algebraic(nx, ny))
                    elif (
                        self.en_passant_target
                        and (nx, ny) == self.en_passant_target
                        and self.board[y][nx]
                        and self.board[y][nx].color != color
                        and self.board[y][nx].type == ChessPieceType.PAWN
                    ):
                        moves.append(self.index_to_algebraic(nx, ny))
        return moves

    # ── Check & legality ────────────────────────────────────────────
    def _king_square(self, color: str) -> Optional[str]:
        for y in range(8):
            for x in range(8):
                p = self.board[y][x]
                if p and p.color == color and p.type == ChessPieceType.KING:
                    return self.index_to_algebraic(x, y)
        return None

    def in_check(self, color: str) -> bool:
        k_sq = self._king_square(color)
        if k_sq is None:
            return False
        enemy = BLACK if color == WHITE else WHITE
        for y in range(8):
            for x in range(8):
                p = self.board[y][x]
                if p and p.color == enemy:
                    if k_sq in self.pseudo_legal_moves(self.index_to_algebraic(x, y)):
                        return True
        return False

    def is_legal(self, start: str, end: str, color: str) -> bool:
        if end not in self.pseudo_legal_moves(start):
            return False
        clone = self.clone()
        clone._apply_move(start, end)
        return not clone.in_check(color)

    def all_legal_moves(self, color: str) -> Dict[str, List[str]]:
        res: Dict[str, List[str]] = {}
        for y in range(8):
            for x in range(8):
                p = self.board[y][x]
                if p and p.color == color:
                    start = self.index_to_algebraic(x, y)
                    ends = [e for e in self.pseudo_legal_moves(start) if self.is_legal(start, e, color)]
                    if ends:
                        res[start] = ends
        return res

    # ── ASCII board ─────────────────────────────────────────────────
    def __repr__(self) -> str:
        rows = []
        for y in reversed(range(8)):
            line = []
            for x in range(8):
                p = self.board[y][x]
                if p is None:
                    line.append(".")
                else:
                    c = p.type.name[0]
                    line.append(c.upper() if p.color == WHITE else c.lower())
            rows.append(f"{y + 1} " + " ".join(line))
        return "\n".join(rows + ["  " + " ".join(FILES)])
