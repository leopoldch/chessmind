import os
import sys

sys.path.append(os.path.join(os.path.dirname(__file__), ".."))

from engine import Engine
from models.game import ChessGame
from models.pieces import WHITE, BLACK


def test_eval_cache_hit_and_eviction():
    engine = Engine()
    engine._eval_cache_size = 2
    game = ChessGame()

    key1 = (engine._board_hash(game.board), WHITE)
    engine.evaluate(game.board, WHITE)
    assert key1 in engine.eval_cache

    size1 = len(engine.eval_cache)
    engine.evaluate(game.board, WHITE)
    assert len(engine.eval_cache) == size1

    game.make_move("e2", "e4")
    key2 = (engine._board_hash(game.board), BLACK)
    engine.evaluate(game.board, BLACK)
    assert key2 in engine.eval_cache

    game.make_move("e7", "e5")
    key3 = (engine._board_hash(game.board), WHITE)
    engine.evaluate(game.board, WHITE)
    assert key3 in engine.eval_cache

    assert key1 not in engine.eval_cache

