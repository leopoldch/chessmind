# ======================== file: chess_board.py ==========================
"""ChessBoard: 8×8 board with legal‑move logic including castling and
en-passant."""
from __future__ import annotations

from dataclasses import dataclass
from typing import List, Optional, Tuple, Dict

# Bitboard helpers -----------------------------------------------------
def _bit(x: int, y: int) -> int:
    return 1 << (y * 8 + x)

def _bit_to_coords(bb: int) -> Tuple[int, int]:
    idx = (bb.bit_length() - 1)
    return idx % 8, idx // 8

from models.pieces import ChessPiece, ChessPieceType, WHITE, BLACK

FILES = "abcdefgh"
RANKS = "12345678"
FILE_TO_IDX = {c: i for i, c in enumerate(FILES)}
RANK_TO_IDX = {c: i for i, c in enumerate(RANKS)}


@dataclass
class MoveState:
    start: str
    end: str
    captured: Optional[ChessPiece]
    prev_en_passant: Optional[Tuple[int, int]]
    prev_castling_rights: Dict[str, Dict[str, bool]]
    rook_start: Optional[str] = None
    rook_end: Optional[str] = None
    ep_capture_pos: Optional[Tuple[int, int]] = None


@dataclass
class NullMoveState:
    prev_en_passant: Optional[Tuple[int, int]]


class ChessBoard:
    def __init__(self) -> None:
        # 2‑D array: board[y][x]
        self.board: List[List[Optional[ChessPiece]]] = [[None for _ in range(8)] for _ in range(8)]
        # bitboards[color][piece_type]
        self.bitboards: Dict[str, Dict[ChessPieceType, int]] = {
            WHITE: {t: 0 for t in ChessPieceType},
            BLACK: {t: 0 for t in ChessPieceType},
        }
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
        for c in (WHITE, BLACK):
            for t in ChessPieceType:
                self.bitboards[c][t] = 0

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
        return FILE_TO_IDX[pos[0]], RANK_TO_IDX[pos[1]]

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
        old = self.board[y][x]
        if old:
            self.bitboards[old.color][old.type] &= ~_bit(x, y)
        self.board[y][x] = piece
        if piece:
            piece.position = (x, y)
            self.bitboards[piece.color][piece.type] |= _bit(x, y)

    def move_piece_unchecked(self, start: str, end: str) -> None:
        piece = self[start]
        if piece is None:
            raise ValueError(f"No piece on {start}")
        self[end] = piece
        self[start] = None

    def make_move_state(self, start: str, end: str) -> MoveState:
        piece = self[start]
        if piece is None:
            raise ValueError(f"No piece on {start}")
        captured = self[end]
        prev_ep = self.en_passant_target
        prev_rights = {
            WHITE: dict(self.castling_rights[WHITE]),
            BLACK: dict(self.castling_rights[BLACK]),
        }

        sx, sy = self.algebraic_to_index(start)
        ex, ey = self.algebraic_to_index(end)
        color = piece.color
        dir_y = 1 if color == WHITE else -1
        rook_start = rook_end = None
        ep_capture_pos = None

        # Castling
        if piece.type == ChessPieceType.KING and abs(ex - sx) == 2:
            if ex > sx:
                rook_start = self.index_to_algebraic(7, sy)
                rook_end = self.index_to_algebraic(ex - 1, sy)
            else:
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

        # En-passant capture
        if piece.type == ChessPieceType.PAWN:
            if self.en_passant_target and (ex, ey) == self.en_passant_target and self.board[ey][ex] is None:
                ep_capture_pos = (ex, ey - dir_y)
                captured = self.board[ep_capture_pos[1]][ep_capture_pos[0]]
                self[ChessBoard.index_to_algebraic(*ep_capture_pos)] = None
            if abs(ey - sy) == 2:
                self.en_passant_target = (sx, sy + dir_y)
            else:
                self.en_passant_target = None
        else:
            self.en_passant_target = None

        self.move_piece_unchecked(start, end)

        return MoveState(
            start,
            end,
            captured,
            prev_ep,
            prev_rights,
            rook_start,
            rook_end,
            ep_capture_pos,
        )

    def unmake_move(self, state: MoveState) -> None:
        # move piece back
        self.move_piece_unchecked(state.end, state.start)
        # restore captured piece
        if state.ep_capture_pos:
            self[state.end] = None
            self[ChessBoard.index_to_algebraic(*state.ep_capture_pos)] = state.captured
        else:
            self[state.end] = state.captured

        # restore rook if castling
        if state.rook_start and state.rook_end:
            self.move_piece_unchecked(state.rook_end, state.rook_start)

        # restore previous state
        self.en_passant_target = state.prev_en_passant
        self.castling_rights = {
            WHITE: dict(state.prev_castling_rights[WHITE]),
            BLACK: dict(state.prev_castling_rights[BLACK]),
        }

    def make_null_move_state(self) -> NullMoveState:
        state = NullMoveState(self.en_passant_target)
        self.en_passant_target = None
        return state

    def unmake_null_move(self, state: NullMoveState) -> None:
        self.en_passant_target = state.prev_en_passant

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
                self[ChessBoard.index_to_algebraic(ex, ey - dir_y)] = None
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
        new = ChessBoard()
        for y in range(8):
            for x in range(8):
                p = self.board[y][x]
                if p is None:
                    new.board[y][x] = None
                else:
                    # create a new ChessPiece with same attributes
                    new.board[y][x] = ChessPiece(p.type, p.color, p.position)
        for c in (WHITE, BLACK):
            for t in ChessPieceType:
                new.bitboards[c][t] = self.bitboards[c][t]
        new.en_passant_target = (
            (self.en_passant_target[0], self.en_passant_target[1])
            if self.en_passant_target is not None
            else None
        )
        new.castling_rights = {
            WHITE: dict(self.castling_rights[WHITE]),
            BLACK: dict(self.castling_rights[BLACK]),
        }
        return new

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
        bb = self.bitboards[color][ChessPieceType.KING]
        if bb:
            x, y = _bit_to_coords(bb)
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

    def is_legal(self, start: str, end: str, color: str, *, assume_pseudo: bool = False) -> bool:
        """Return True if the move is legal.

        The ``assume_pseudo`` flag skips generation of pseudo legal moves and
        assumes the caller already checked that ``end`` is a pseudo legal move
        for ``start``.  ``all_legal_moves`` uses this to avoid redundant work
        when iterating over every pseudo legal move on the board.
        """
        if not assume_pseudo and end not in self.pseudo_legal_moves(start):
            return False

        piece = self[start]
        if piece and piece.type == ChessPieceType.KING:
            sx, sy = self.algebraic_to_index(start)
            ex, ey = self.algebraic_to_index(end)
            if abs(ex - sx) == 2:
                # Can't castle while in check
                if self.in_check(color):
                    return False
                # Squares the king passes through must not be under attack
                step = 1 if ex > sx else -1
                interm = self.index_to_algebraic(sx + step, sy)
                state = self.make_move_state(start, interm)
                in_check = self.in_check(color)
                self.unmake_move(state)
                if in_check:
                    return False

        state = self.make_move_state(start, end)
        illegal = self.in_check(color)
        self.unmake_move(state)
        return not illegal

    def all_legal_moves(self, color: str) -> Dict[str, List[str]]:
        res: Dict[str, List[str]] = {}
        for t in ChessPieceType:
            bb = self.bitboards[color][t]
            while bb:
                lsb = bb & -bb
                x, y = _bit_to_coords(lsb)
                start = self.index_to_algebraic(x, y)
                pseudo = self.pseudo_legal_moves(start)
                ends = [e for e in pseudo if self.is_legal(start, e, color, assume_pseudo=True)]
                if ends:
                    res[start] = ends
                bb &= bb - 1
        return res

    def piece_count(self, piece_type: ChessPieceType, color: str | None = None) -> int:
        """Return the number of pieces of the given type on the board.

        If ``color`` is provided, only pieces of that color are counted.
        """
        if color is not None:
            return self.bitboards[color][piece_type].bit_count()
        return (
            self.bitboards[WHITE][piece_type].bit_count()
            + self.bitboards[BLACK][piece_type].bit_count()
        )

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
