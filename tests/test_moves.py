import os
import sys
import pytest

# Add the models directory to the path so the modules can be imported
sys.path.append(os.path.join(os.path.dirname(__file__), "..", "models"))

from board import ChessBoard
from pieces import ChessPiece, ChessPieceType, WHITE, BLACK


def empty_board():
    board = ChessBoard()
    # clear all squares
    for y in range(8):
        for x in range(8):
            board.board[y][x] = None
    board.en_passant_target = None
    board.castling_rights = {WHITE: {"K": True, "Q": True}, BLACK: {"K": True, "Q": True}}
    return board


def test_knight_moves_blocked_and_capture():
    board = empty_board()
    # White knight on d4
    board["d4"] = ChessPiece(ChessPieceType.KNIGHT, WHITE, (3, 3))
    # Friendly pieces blocking
    board["c6"] = ChessPiece(ChessPieceType.PAWN, WHITE, (2, 5))
    board["e5"] = ChessPiece(ChessPieceType.PAWN, WHITE, (4, 4))
    # Enemy piece to capture
    board["e2"] = ChessPiece(ChessPieceType.BISHOP, BLACK, (4, 1))

    moves = sorted(board.pseudo_legal_moves("d4"))
    assert moves == sorted(["b3", "b5", "c2", "e2", "e6", "f3", "f5"])


def test_bishop_blocked_and_capture():
    board = empty_board()
    # White bishop on c1
    board["c1"] = ChessPiece(ChessPieceType.BISHOP, WHITE, (2, 0))
    # Friendly pawn blocking on d2
    board["d2"] = ChessPiece(ChessPieceType.PAWN, WHITE, (3, 1))
    # Enemy piece on b2
    board["b2"] = ChessPiece(ChessPieceType.ROOK, BLACK, (1, 1))

    moves = sorted(board.pseudo_legal_moves("c1"))
    assert moves == ["b2"]


def test_rook_pinned_by_queen():
    board = empty_board()
    # White king on e1
    board["e1"] = ChessPiece(ChessPieceType.KING, WHITE, (4, 0))
    # White rook on e2 pinned by black rook on e8
    board["e2"] = ChessPiece(ChessPieceType.ROOK, WHITE, (4, 1))
    board["e8"] = ChessPiece(ChessPieceType.ROOK, BLACK, (4, 7))

    moves = board.all_legal_moves(WHITE)
    assert set(moves.get("e2", [])) == set(["e3", "e4", "e5", "e6", "e7", "e8"])


def test_castling_rights_and_blocking():
    board = empty_board()
    # Place kings and rooks for castling
    board["e1"] = ChessPiece(ChessPieceType.KING, WHITE, (4, 0))
    board["h1"] = ChessPiece(ChessPieceType.ROOK, WHITE, (7, 0))
    board["a1"] = ChessPiece(ChessPieceType.ROOK, WHITE, (0, 0))
    board["e8"] = ChessPiece(ChessPieceType.KING, BLACK, (4, 7))
    # Blocks on queenside
    board["b1"] = ChessPiece(ChessPieceType.KNIGHT, WHITE, (1, 0))
    board["c1"] = ChessPiece(ChessPieceType.BISHOP, WHITE, (2, 0))

    moves = board.all_legal_moves(WHITE)
    # Only kingside castling should be available
    assert "e1" in moves and "g1" in moves["e1"]
    assert "c1" not in moves.get("e1", [])


def test_en_passant_capture():
    board = empty_board()
    # White pawn on e5
    board["e5"] = ChessPiece(ChessPieceType.PAWN, WHITE, (4, 4))
    # Black pawn that moved two squares to f5
    board["f5"] = ChessPiece(ChessPieceType.PAWN, BLACK, (5, 4))
    board.en_passant_target = (5, 5)  # f6 square

    moves = board.pseudo_legal_moves("e5")
    assert "f6" in moves
