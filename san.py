import re
from typing import Tuple, Optional

from models.game import ChessGame
from models.pieces import WHITE, BLACK, ChessPieceType

SAN_REGEX = re.compile(r'^([NBRQK])?([a-h])?([1-8])?[x-]?([a-h][1-8])(=?[NBRQK])?[+#]?$', re.I)

PIECE_MAP = {
    None: ChessPieceType.PAWN,
    "N": ChessPieceType.KNIGHT,
    "B": ChessPieceType.BISHOP,
    "R": ChessPieceType.ROOK,
    "Q": ChessPieceType.QUEEN,
    "K": ChessPieceType.KING,
}

PROMO_MAP = {
    None: None,
    "N": ChessPieceType.KNIGHT,
    "B": ChessPieceType.BISHOP,
    "R": ChessPieceType.ROOK,
    "Q": ChessPieceType.QUEEN,
    "K": ChessPieceType.KING,
}

def parse_san(game: ChessGame, san: str, color: str) -> Tuple[str, str, Optional[ChessPieceType]]:
    """Parse a simple SAN move and return (start, end, promotion)."""
    san = san.replace("0", "O")
    if san.upper() in {"O-O", "O-O+", "O-O#"}:
        start = "e1" if color == WHITE else "e8"
        end = "g1" if color == WHITE else "g8"
        return start, end, None
    if san.upper() in {"O-O-O", "O-O-O+", "O-O-O#"}:
        start = "e1" if color == WHITE else "e8"
        end = "c1" if color == WHITE else "c8"
        return start, end, None
    m = SAN_REGEX.match(san)
    if not m:
        raise ValueError(f"cannot parse SAN: {san}")
    piece_letter, dfile, drank, dest, promo = m.groups()
    piece_type = PIECE_MAP[piece_letter.upper() if piece_letter else None]
    dis_file = dfile
    dis_rank = drank
    promo_piece = PROMO_MAP[promo.strip("=").upper() if promo else None]
    moves = game.board.all_legal_moves(color)
    candidates = []
    for start, ends in moves.items():
        if dest not in ends:
            continue
        piece = game.board[start]
        if piece.type != piece_type:
            continue
        if dis_file and start[0] != dis_file:
            continue
        if dis_rank and start[1] != dis_rank:
            continue
        candidates.append(start)
    if len(candidates) != 1:
        raise ValueError(f"ambiguous SAN: {san}")
    return candidates[0], dest, promo_piece

