import os
import sys

sys.path.append(os.path.join(os.path.dirname(__file__), ".."))

from engine import Engine, PIECE_VALUES, CENTER_SQUARES
from models.game import ChessGame
from models.pieces import ChessPieceType, WHITE, BLACK


def eval_python(board, color):
    value = 0
    for y in range(8):
        for x in range(8):
            p = board.board[y][x]
            if p:
                mul = 1 if p.color == color else -1
                value += PIECE_VALUES[p.type] * mul
                if (x, y) in CENTER_SQUARES:
                    value += mul
                if p.type == ChessPieceType.PAWN:
                    advance = y if p.color == WHITE else 7 - y
                    value += (advance // 2) * mul
                    if (p.color == WHITE and y == 6) or (
                        p.color == BLACK and y == 1
                    ):
                        value += 3 * mul
    wk = board._king_square(WHITE)
    bk = board._king_square(BLACK)
    if wk in ("g1", "c1"):
        value += 1 if color == WHITE else -1
    if bk in ("g8", "c8"):
        value += 1 if color == BLACK else -1
    if board.in_check(BLACK if color == WHITE else WHITE):
        value += 1
    if board.in_check(color):
        value -= 1
    return value


def test_eval_speedup_matches_python():
    try:
        from engine_cython.eval_speedups import evaluate_board_cython
    except Exception:
        evaluate_board_cython = None
    if evaluate_board_cython is None:
        return
    game = ChessGame()
    eng = Engine()
    assert evaluate_board_cython(game.board, WHITE, PIECE_VALUES) == eval_python(game.board, WHITE)
    game.make_move("e2", "e4")
    game.make_move("e7", "e5")
    assert evaluate_board_cython(game.board, WHITE, PIECE_VALUES) == eval_python(game.board, WHITE)
