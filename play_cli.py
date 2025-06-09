from __future__ import annotations

from models.game import ChessGame
from models.board import ChessBoard
from models.pieces import WHITE, BLACK
from engine import Engine


def print_board(game: ChessGame) -> None:
    print(game.board)


def main() -> None:
    game = ChessGame()
    engine = Engine()
    color_choice = input("Play as (w/b)? ").strip().lower()
    human_color = WHITE if color_choice != "b" else BLACK
    print("Enter moves in algebraic format like e2e4")
    while not game.result:
        print_board(game)
        if game.current_turn == human_color:
            move = input(f"{game.current_turn}> ")
            if len(move) < 4:
                print("Invalid move format")
                continue
            start, end = move[:2], move[2:4]
            if not game.make_move(start, end):
                print("Illegal move")
                continue
        else:
            start, end = engine.best_move(game)
            print(f"AI plays {start}{end}")
            game.make_move(start, end)
    print_board(game)
    if game.result == "draw":
        print("Game drawn")
    else:
        print(f"{game.result.capitalize()} wins")


if __name__ == "__main__":
    main()
