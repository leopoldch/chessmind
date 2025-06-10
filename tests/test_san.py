import os
import sys
sys.path.append(os.path.join(os.path.dirname(__file__), ".."))

from models.game import ChessGame
from models.pieces import WHITE, BLACK
from san import parse_san


def test_parse_basic_san():
    game = ChessGame()
    start, end, promo = parse_san(game, "e4", WHITE)
    assert (start, end) == ("e2", "e4")
    assert promo is None
    game.make_move(start, end)

    start, end, promo = parse_san(game, "e5", BLACK)
    assert (start, end) == ("e7", "e5")
    assert promo is None


def test_parse_castle():
    game = ChessGame()
    moves = ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6", "Ba4", "Nf6", "O-O"]
    color = WHITE
    for m in moves[:-1]:
        s, e, p = parse_san(game, m, color)
        game.make_move(s, e)
        color = BLACK if color == WHITE else WHITE
    s, e, p = parse_san(game, moves[-1], WHITE)
    assert (s, e) == ("e1", "g1")
    assert p is None


def test_parse_pawn_capture_with_file_start():
    game = ChessGame()
    # Clear board and set up custom position
    board = game.board
    for y in range(8):
        for x in range(8):
            board.board[y][x] = None
    from models.pieces import ChessPiece, ChessPieceType
    board.bitboards = {WHITE: {t: 0 for t in ChessPieceType}, BLACK: {t: 0 for t in ChessPieceType}}
    board["b5"] = ChessPiece(ChessPieceType.PAWN, WHITE, (1, 4))
    board["c6"] = ChessPiece(ChessPieceType.KNIGHT, BLACK, (2, 5))
    game.current_turn = WHITE

    start, end, promo = parse_san(game, "bxc6", WHITE)
    assert (start, end) == ("b5", "c6")
    assert promo is None

