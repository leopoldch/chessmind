# cython: language_level=3
import cython
from models.pieces import WHITE, BLACK

@cython.boundscheck(False)
@cython.wraparound(False)
cpdef int quiescence_cython(object engine, object board, int alpha, int beta, str color):
    cdef int stand = engine.evaluate(board, color)
    cdef object moves
    cdef object start
    cdef object ends
    cdef object piece
    cdef object target
    cdef object state
    cdef str next_color
    cdef int score
    if stand >= beta:
        return beta
    if alpha < stand:
        alpha = stand
    moves = board.all_legal_moves(color)
    for start, ends in moves.items():
        piece = board[start]
        for end in ends:
            target = board[end]
            if target is None or target.color == piece.color:
                continue
            state = board.make_move_state(start, end)
            next_color = BLACK if color == WHITE else WHITE
            score = -quiescence_cython(engine, board, -beta, -alpha, next_color)
            board.unmake_move(state)
            if score >= beta:
                return beta
            if score > alpha:
                alpha = score
    return alpha

@cython.boundscheck(False)
@cython.wraparound(False)
cpdef tuple negamax_cython(object engine, object board, str color, int depth, int alpha, int beta, int ply):
    cdef unsigned long long key = engine._hash(board, color)
    cdef object entry = engine.tt.get(key)
    cdef str next_color
    cdef int score
    if entry is not None and entry.depth >= depth:
        return entry.score, entry.move

    if depth == 0:
        return quiescence_cython(engine, board, alpha, beta, color), None

    if depth >= 3 and not board.in_check(color):
        next_color = BLACK if color == WHITE else WHITE
        nm_state = board.make_null_move_state()
        score, _ = negamax_cython(engine, board, next_color, depth - 3, -beta, -beta + 1, ply + 1)
        board.unmake_null_move(nm_state)
        score = -score
        if score >= beta:
            return score, None

    cdef int best_score = -10000
    cdef object best_move = None
    cdef object moves = board.all_legal_moves(color)
    if not moves:
        if board.in_check(color):
            return -9999 + (engine.depth - depth), None
        return 0, None
    cdef object tt_move = entry.move if entry is not None else None
    cdef list ordered_moves = engine._order_moves(board, moves, tt_move, ply)
    cdef int i = 0
    cdef object start
    cdef object end
    cdef bint is_capture
    cdef object state
    cdef bint gives_check
    cdef int reduction
    cdef int ext_depth
    for start, end in ordered_moves:
        is_capture = board[end] is not None
        state = board.make_move_state(start, end)
        next_color = BLACK if color == WHITE else WHITE
        gives_check = board.in_check(next_color)
        reduction = 0
        if depth > 2 and i >= 3 and not is_capture:
            reduction = 1
        ext_depth = depth - 1 + (1 if gives_check else 0)
        if reduction:
            score, _ = negamax_cython(engine, board, next_color, ext_depth - reduction, -alpha - 1, -alpha, ply + 1)
            score = -score
            if score > alpha:
                score, _ = negamax_cython(engine, board, next_color, ext_depth, -beta, -alpha, ply + 1)
                score = -score
        else:
            score, _ = negamax_cython(engine, board, next_color, ext_depth, -beta, -alpha, ply + 1)
            score = -score
        if score > best_score:
            best_score = score
            best_move = (start, end)
        if best_score > alpha:
            alpha = best_score
        if alpha >= beta:
            if not is_capture:
                if ply >= len(engine.killer_moves):
                    engine.killer_moves.extend([[None, None] for _ in range(ply - len(engine.killer_moves) + 1)])
                km = engine.killer_moves[ply]
                if (start, end) not in km:
                    km.pop()
                    km.insert(0, (start, end))
            engine.history_table[(start, end)] += depth * depth
            board.unmake_move(state)
            break
        board.unmake_move(state)
        i += 1

    engine.tt[key] = engine.TransEntry(depth, best_score, best_move)
    engine.tt.move_to_end(key)
    if len(engine.tt) > engine._tt_size:
        engine.tt.popitem(last=False)
    return best_score, best_move
