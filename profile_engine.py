import cProfile
from engine import Engine
from models.game import ChessGame


def main() -> None:
    game = ChessGame()
    engine = Engine(depth=3)
    cProfile.run('engine.best_move(game)')


if __name__ == '__main__':
    main()
