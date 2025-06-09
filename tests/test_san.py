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

