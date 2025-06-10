# cython: language_level=3
import cython
cimport cython
from models.pieces import ChessPieceType, WHITE, BLACK

cdef inline bint _in_center(int x, int y):
    return (x == 3 or x == 4) and (y == 3 or y == 4)

@cython.boundscheck(False)
@cython.wraparound(False)
cpdef int evaluate_board_cython(object board, str color, dict piece_values):
    cdef int value = 0
    cdef int x, y
    cdef object p
    cdef int mul
    for y in range(8):
        for x in range(8):
            p = board.board[y][x]
            if p is not None:
                if p.color == color:
                    mul = 1
                else:
                    mul = -1
                value += piece_values[p.type] * mul
                if _in_center(x, y):
                    value += mul
                if p.type == ChessPieceType.PAWN:
                    if p.color == WHITE:
                        advance = y
                    else:
                        advance = 7 - y
                    value += (advance // 2) * mul
                    if (p.color == WHITE and y == 6) or (p.color == BLACK and y == 1):
                        value += 3 * mul
    cdef object wk = board._king_square(WHITE)
    cdef object bk = board._king_square(BLACK)
    if wk in ("g1", "c1"):
        if color == WHITE:
            value += 1
        else:
            value -= 1
    if bk in ("g8", "c8"):
        if color == BLACK:
            value += 1
        else:
            value -= 1
    if board.in_check(BLACK if color == WHITE else WHITE):
        value += 1
    if board.in_check(color):
        value -= 1
    return value
