# cython: language_level=3
import numpy as np
cimport cython

@cython.boundscheck(False)
@cython.wraparound(False)
cpdef list order_moves_cython(object board, dict moves_dict, list killer_moves,
                              dict history_table, dict piece_values,
                              object tt_move=None, int ply=0):
    cdef list ordered = []
    cdef int score
    cdef object start
    cdef object end
    cdef object target
    for start, ends in moves_dict.items():
        p = board[start]
        for end in ends:
            score = history_table.get((start, end), 0)
            if tt_move is not None and (start, end) == tt_move:
                score += 10000
            target = board[end]
            if target is not None:
                score += 5000
                score += piece_values[target.type] * 100 - piece_values[p.type]
            if ply < len(killer_moves) and (start, end) in killer_moves[ply]:
                score += 7000
            ordered.append((score, start, end))
    ordered.sort(key=_score_key, reverse=True)
    return [(s, e) for _, s, e in ordered]

cdef inline int _score_key(tuple item):
    return <int>item[0]
