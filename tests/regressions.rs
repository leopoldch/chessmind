use chessmind::board::Board;
use chessmind::engine::{Engine, TimeConfig};
use chessmind::game::Game;
use chessmind::movegen::generate_moves_fast;
use chessmind::pieces::{Color, Piece, PieceType};
use chessmind::transposition::{Bound, TTEntry, Table};
use chessmind::types::{Move, MoveList};
use std::collections::BTreeSet;

fn piece(piece_type: PieceType, color: Color) -> Piece {
    Piece { piece_type, color }
}

fn put(board: &mut Board, sq: &str, piece_type: PieceType, color: Color) {
    assert!(board.set(sq, Some(piece(piece_type, color))));
}

fn game_from_board(board: Board, turn: Color) -> Game {
    let hash = board.hash(turn);
    let mut hash_counts = std::collections::HashMap::new();
    hash_counts.insert(hash, 1);

    Game {
        board,
        current_turn: turn,
        history: Vec::new(),
        hash_history: vec![hash],
        hash_counts,
        result: None,
    }
}

fn opposite(color: Color) -> Color {
    match color {
        Color::White => Color::Black,
        Color::Black => Color::White,
    }
}

fn move_to_pair(mv: Move) -> (String, String) {
    let from = Board::index_to_algebraic((mv.from_sq() % 8) as usize, (mv.from_sq() / 8) as usize)
        .unwrap();
    let mut to =
        Board::index_to_algebraic((mv.to_sq() % 8) as usize, (mv.to_sq() / 8) as usize).unwrap();
    if mv.is_promotion() {
        if let Some(pt) = mv.promotion_piece() {
            let suffix = match pt {
                PieceType::Knight => 'n',
                PieceType::Bishop => 'b',
                PieceType::Rook => 'r',
                PieceType::Queen => 'q',
                _ => 'q',
            };
            to.push(suffix);
        }
    }
    (from, to)
}

fn legal_moves(board: &mut Board, color: Color) -> Vec<Move> {
    let mut list = MoveList::new();
    generate_moves_fast(board, color, &mut list);
    list.iter().copied().collect()
}

fn move_set(board: &mut Board, color: Color) -> BTreeSet<(String, String)> {
    legal_moves(board, color)
        .into_iter()
        .map(move_to_pair)
        .collect()
}

fn perft(board: &mut Board, color: Color, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }

    let moves = legal_moves(board, color);
    if depth == 1 {
        return moves.len() as u64;
    }

    let mut nodes = 0;
    for mv in moves {
        let undo = board.make_move_fast(mv, color);
        nodes += perft(board, opposite(color), depth - 1);
        board.unmake_move_fast(undo, color);
    }
    nodes
}

fn assert_board_restored(before: &Board, after: &mut Board) {
    after.recompute_hash();
    assert_eq!(after.hash, before.hash, "hash mismatch after restore");
    assert_eq!(
        after.to_fen(Color::White),
        before.to_fen(Color::White),
        "piece placement / castling / ep state changed after restore",
    );
    assert_eq!(after.en_passant, before.en_passant);
    assert_eq!(after.castling, before.castling);
    assert_eq!(after.bitboards, before.bitboards);
    assert_eq!(after.piece_count_all(), before.piece_count_all());
}

fn is_checkmate(board: &mut Board, color: Color) -> bool {
    board.in_check_fast(color) && legal_moves(board, color).is_empty()
}

fn mate_in_two_first_moves(board: &Board, color: Color) -> BTreeSet<(String, String)> {
    let mut winning = BTreeSet::new();
    let mut root = board.clone();
    let root_moves = legal_moves(&mut root, color);

    for mv in root_moves {
        let mut after_root = board.clone();
        after_root.make_move_fast(mv, color);

        let opp = opposite(color);
        let replies = legal_moves(&mut after_root, opp);
        if replies.is_empty() {
            continue;
        }

        let mut forced = true;
        for reply in replies {
            let mut after_reply = after_root.clone();
            after_reply.make_move_fast(reply, opp);

            let white_mates = legal_moves(&mut after_reply, color)
                .into_iter()
                .any(|mate_mv| {
                    let mut after_mate = after_reply.clone();
                    after_mate.make_move_fast(mate_mv, color);
                    is_checkmate(&mut after_mate, opp)
                });

            if !white_mates {
                forced = false;
                break;
            }
        }

        if forced {
            winning.insert(move_to_pair(mv));
        }
    }

    winning
}

#[test]
fn start_position_perft_is_stable() {
    let mut board = Board::new();
    board.setup_standard();

    assert_eq!(perft(&mut board, Color::White, 1), 20);
    assert_eq!(perft(&mut board, Color::White, 2), 400);
}

#[test]
fn castling_is_rejected_through_attack() {
    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "h1", PieceType::Rook, Color::White);
    put(&mut board, "c4", PieceType::Bishop, Color::Black);
    put(&mut board, "a8", PieceType::King, Color::Black);

    assert!(!board.is_legal("e1", "g1", Color::White));

    let moves = move_set(&mut board, Color::White);
    assert!(!moves.contains(&("e1".to_string(), "g1".to_string())));
}

#[test]
fn en_passant_is_rejected_if_it_exposes_the_king() {
    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "e5", PieceType::Pawn, Color::White);
    put(&mut board, "d5", PieceType::Pawn, Color::Black);
    put(&mut board, "e8", PieceType::Rook, Color::Black);
    put(&mut board, "a8", PieceType::King, Color::Black);
    board.en_passant = Some((3, 5));

    assert!(!board.is_legal("e5", "d6", Color::White));

    let moves = move_set(&mut board, Color::White);
    assert!(!moves.contains(&("e5".to_string(), "d6".to_string())));
}

#[test]
fn promotion_generation_includes_all_underpromotions() {
    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "h8", PieceType::King, Color::Black);
    put(&mut board, "e7", PieceType::Pawn, Color::White);
    put(&mut board, "d8", PieceType::Rook, Color::Black);
    put(&mut board, "f8", PieceType::Rook, Color::Black);

    let mut list = MoveList::new();
    generate_moves_fast(&mut board, Color::White, &mut list);

    let promos: Vec<_> = list.iter().copied().filter(|m| m.is_promotion()).collect();
    assert_eq!(promos.len(), 12);

    let promo_targets: BTreeSet<_> = promos
        .into_iter()
        .map(|m| {
            let (_, to) = move_to_pair(m);
            let suffix = to.chars().last().unwrap();
            (to, suffix)
        })
        .collect();
    assert!(promo_targets.contains(&(String::from("d8q"), 'q')));
    assert!(promo_targets.contains(&(String::from("d8r"), 'r')));
    assert!(promo_targets.contains(&(String::from("d8b"), 'b')));
    assert!(promo_targets.contains(&(String::from("d8n"), 'n')));
    assert!(promo_targets.contains(&(String::from("e8q"), 'q')));
    assert!(promo_targets.contains(&(String::from("f8q"), 'q')));
}

#[test]
fn check_evasions_are_complete_and_legal() {
    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "e8", PieceType::Rook, Color::Black);
    put(&mut board, "h8", PieceType::King, Color::Black);

    let moves = move_set(&mut board, Color::White);
    let expected: BTreeSet<(String, String)> = [
        ("e1".to_string(), "d1".to_string()),
        ("e1".to_string(), "f1".to_string()),
        ("e1".to_string(), "d2".to_string()),
        ("e1".to_string(), "f2".to_string()),
    ]
    .into_iter()
    .collect();

    assert_eq!(moves, expected);
}

#[test]
fn make_unmake_restores_state_across_special_moves() {
    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "e8", PieceType::King, Color::Black);
    put(&mut board, "e2", PieceType::Pawn, Color::White);
    let before_normal = board.clone();
    let undo = board.make_move_fast(Move::new(12, 28, Move::FLAG_DOUBLE_PUSH), Color::White);
    board.unmake_move_fast(undo, Color::White);
    assert_board_restored(&before_normal, &mut board);

    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "e8", PieceType::King, Color::Black);
    put(&mut board, "e4", PieceType::Pawn, Color::White);
    put(&mut board, "d5", PieceType::Pawn, Color::Black);
    let before_capture = board.clone();
    let undo = board.make_move_fast(Move::capture(28, 35), Color::White);
    board.unmake_move_fast(undo, Color::White);
    assert_board_restored(&before_capture, &mut board);

    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "h8", PieceType::King, Color::Black);
    put(&mut board, "e7", PieceType::Pawn, Color::White);
    let before_promo = board.clone();
    let undo = board.make_move_fast(
        Move::promotion(52, 60, PieceType::Queen, false),
        Color::White,
    );
    board.unmake_move_fast(undo, Color::White);
    assert_board_restored(&before_promo, &mut board);

    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "h8", PieceType::King, Color::Black);
    put(&mut board, "e5", PieceType::Pawn, Color::White);
    put(&mut board, "d5", PieceType::Pawn, Color::Black);
    put(&mut board, "e8", PieceType::Rook, Color::Black);
    board.en_passant = Some((3, 5));
    let before_ep = board.clone();
    let undo = board.make_move_fast(Move::new(36, 43, Move::FLAG_EP_CAPTURE), Color::White);
    board.unmake_move_fast(undo, Color::White);
    assert_board_restored(&before_ep, &mut board);

    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "h8", PieceType::King, Color::Black);
    put(&mut board, "h1", PieceType::Rook, Color::White);
    let before_castle = board.clone();
    let undo = board.make_move_fast(Move::new(4, 6, Move::FLAG_KING_CASTLE), Color::White);
    board.unmake_move_fast(undo, Color::White);
    assert_board_restored(&before_castle, &mut board);
}

#[test]
fn transposition_table_round_trip_is_consistent() {
    let table = Table::new(8);
    let entry = TTEntry {
        depth: 6,
        value: 1234,
        bound: Bound::Exact,
        best: Some(Move::new(12, 28, Move::FLAG_DOUBLE_PUSH)),
    };
    table.store(0xdead_beef, entry);

    let got = table.get(0xdead_beef).expect("missing TT entry");
    assert_eq!(got.depth, entry.depth);
    assert_eq!(got.value, entry.value);
    assert!(matches!(got.bound, Bound::Exact));
    assert_eq!(got.best, entry.best);

    let replacement = TTEntry {
        depth: 8,
        value: -55,
        bound: Bound::Lower,
        best: Some(Move::new(4, 6, Move::FLAG_KING_CASTLE)),
    };
    table.store(0xdead_beef, replacement);
    let got = table
        .get(0xdead_beef)
        .expect("missing replacement TT entry");
    assert_eq!(got.depth, replacement.depth);
    assert_eq!(got.value, replacement.value);
    assert!(matches!(got.bound, Bound::Lower));
    assert_eq!(got.best, replacement.best);
}

#[test]
fn repetition_tracking_counts_repeat_positions() {
    let mut board = Board::new();
    put(&mut board, "e1", PieceType::King, Color::White);
    put(&mut board, "e8", PieceType::King, Color::Black);
    board.castling = [[false, false], [false, false]];
    let mut game = game_from_board(board, Color::White);
    let initial = game.hash_history[0];

    assert!(game.make_move("e1", "f1"));
    assert!(game.make_move("e8", "f8"));
    assert!(game.make_move("f1", "e1"));
    assert!(game.make_move("f8", "e8"));
    assert_eq!(game.repetition_count(initial), 2);

    assert!(game.make_move("e1", "f1"));
    assert!(game.make_move("e8", "f8"));
    assert!(game.make_move("f1", "e1"));
    assert!(game.make_move("f8", "e8"));
    assert_eq!(game.repetition_count(initial), 3);
}

#[test]
fn tactical_capture_and_mate_regressions_remain_stable() {
    let mut capture_game = game_from_board(
        {
            let mut board = Board::new();
            put(&mut board, "e1", PieceType::King, Color::White);
            put(&mut board, "d1", PieceType::Queen, Color::White);
            put(&mut board, "h8", PieceType::King, Color::Black);
            put(&mut board, "d8", PieceType::Rook, Color::Black);
            board
        },
        Color::White,
    );

    let mut engine = Engine::new(3);
    let config = TimeConfig::fixed_depth(3);
    let result = engine
        .best_move_timed(&mut capture_game, &config)
        .expect("missing move");
    assert_eq!(result.0, ("d1".to_string(), "d8".to_string()));

    let mut mate_one_game = game_from_board(
        {
            let mut board = Board::new();
            put(&mut board, "e1", PieceType::King, Color::White);
            put(&mut board, "a1", PieceType::Rook, Color::White);
            put(&mut board, "h8", PieceType::King, Color::Black);
            put(&mut board, "g7", PieceType::Pawn, Color::Black);
            put(&mut board, "h7", PieceType::Pawn, Color::Black);
            board
        },
        Color::White,
    );

    let mut engine = Engine::new(2);
    let config = TimeConfig::fixed_depth(2);
    let result = engine
        .best_move_timed(&mut mate_one_game, &config)
        .expect("missing move");
    assert_eq!(result.0, ("a1".to_string(), "a8".to_string()));

    let mut mate_two_board = Board::new();
    put(&mut mate_two_board, "g1", PieceType::King, Color::White);
    put(&mut mate_two_board, "d1", PieceType::Queen, Color::White);
    put(&mut mate_two_board, "a1", PieceType::Rook, Color::White);
    put(&mut mate_two_board, "h8", PieceType::King, Color::Black);
    put(&mut mate_two_board, "g7", PieceType::Pawn, Color::Black);
    let mate_two_moves = mate_in_two_first_moves(&mate_two_board, Color::White);
    let expected: BTreeSet<(String, String)> = [
        ("a1".to_string(), "a8".to_string()),
        ("d1".to_string(), "h5".to_string()),
    ]
    .into_iter()
    .collect();

    assert_eq!(mate_two_moves, expected);
}

#[test]
fn transposition_table_keeps_promotion_piece() {
    let table = Table::new(64);
    for (key, piece_type) in [
        (1u64, PieceType::Knight),
        (2, PieceType::Bishop),
        (3, PieceType::Rook),
        (4, PieceType::Queen),
    ] {
        for capture in [false, true] {
            let mv = Move::promotion(50, if capture { 57 } else { 58 }, piece_type, capture);
            let key = key * 1000 + capture as u64;
            table.store(
                key,
                TTEntry {
                    depth: 5,
                    value: -321,
                    bound: Bound::Exact,
                    best: Some(mv),
                },
            );
            let got = table.get(key).expect("missing TT entry");
            assert_eq!(got.best, Some(mv));
            assert_eq!(got.value, -321);
        }
    }
}

/// White: Kc6, Pc7. Black: Ka7. c8=Q is stalemate, c8=R wins. The TT used to
/// store only (from, to), so the search returned c8=Q.
fn underpromotion_game() -> Game {
    let mut board = Board::new();
    put(&mut board, "c6", PieceType::King, Color::White);
    put(&mut board, "c7", PieceType::Pawn, Color::White);
    put(&mut board, "a7", PieceType::King, Color::Black);
    board.castling = [[false, false], [false, false]];
    game_from_board(board, Color::White)
}

#[test]
fn search_underpromotes_to_avoid_stalemate() {
    for depth in 2..=10 {
        let mut game = underpromotion_game();
        let mut engine = Engine::new(depth);
        let result = engine
            .best_move_timed(&mut game, &TimeConfig::fixed_depth(depth))
            .expect("missing move");
        assert_eq!(
            result.0,
            ("c7".to_string(), "c8r".to_string()),
            "depth {depth}"
        );
    }

    // Parallel root search (used from depth 9 with several threads).
    let mut game = underpromotion_game();
    let mut engine = Engine::with_threads(10, 3);
    let result = engine
        .best_move_timed(&mut game, &TimeConfig::fixed_depth(10))
        .expect("missing move");
    assert_eq!(result.0, ("c7".to_string(), "c8r".to_string()));
}

fn check_has_legal_move(board: &mut Board, color: Color, depth: u32, positions: &mut usize) {
    let moves = legal_moves(board, color);
    assert_eq!(
        board.has_legal_move(color),
        !moves.is_empty(),
        "has_legal_move disagrees with the generator in {}",
        board.to_fen(color)
    );
    *positions += 1;
    if depth == 0 {
        return;
    }
    for mv in moves {
        let undo = board.make_move_fast(mv, color);
        check_has_legal_move(board, opposite(color), depth - 1, positions);
        board.unmake_move_fast(undo, color);
    }
}

#[test]
fn has_legal_move_matches_full_generation() {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        // Stalemates, pins and en passant corner cases.
        "k7/8/1Q6/8/8/8/8/7K b - - 0 1",
        "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1",
        "8/8/8/KPp4r/8/8/8/7k w - c6 0 1",
        "k7/P7/1K6/8/8/8/8/8 b - - 0 1",
        "5k2/5P2/5K2/8/8/8/8/3r4 w - - 0 1",
        "K7/1r6/2k5/8/8/8/8/8 w - - 0 1",
    ];
    let mut positions = 0;
    for fen in fens {
        let game = chessmind::fen::game_from_fen(fen).expect("valid FEN");
        let mut board = game.board.clone();
        check_has_legal_move(&mut board, game.current_turn, 3, &mut positions);
    }
    assert!(positions > 10_000);
}

/// White: Kh1, Qe3. Black: Ka8, Pb6. Qxb6 wins a pawn but stalemates; the
/// quiescence search at the horizon must see the stalemate (depth 1).
#[test]
fn search_sees_stalemate_at_the_horizon() {
    for depth in 1..=4 {
        let mut game =
            chessmind::fen::game_from_fen("k7/8/1p6/8/8/4Q3/8/7K w - - 0 1").expect("valid FEN");
        let mut engine = Engine::new(depth);
        let ((from, to), _) = engine
            .best_move_timed(&mut game, &TimeConfig::fixed_depth(depth))
            .expect("missing move");
        assert_ne!(
            (from.as_str(), to.as_str()),
            ("e3", "b6"),
            "depth {depth}: Qxb6 is stalemate"
        );
    }
}

#[test]
fn halfmove_clock_follows_the_game_and_is_restored_by_unmake() {
    let mut game = Game::new();
    assert_eq!(game.board.halfmove, 0);
    for (mv, expected) in [
        ("g1f3", 1),
        ("g8f6", 2),
        ("f3g1", 3),
        ("f6g8", 4),
        ("e2e4", 0),
    ] {
        let (from, to) = mv.split_at(2);
        assert!(game.make_move(from, to));
        assert_eq!(game.board.halfmove, expected, "after {mv}");
    }

    let mut board = game.board.clone();
    board.halfmove = 57;
    for mv in legal_moves(&mut board, Color::Black) {
        let undo = board.make_move_fast(mv, Color::Black);
        let zeroing = mv.is_capture() || board.piece_type_idx_at(mv.to_sq()) == 0;
        assert_eq!(board.halfmove, if zeroing { 0 } else { 58 });
        board.unmake_move_fast(undo, Color::Black);
        assert_eq!(board.halfmove, 57);
    }
}

/// White (Kc3, Qh1, Pa2) against a bare king, with the halfmove clock at 99:
/// every move except a pawn push draws by the fifty-move rule.
#[test]
fn search_respects_the_fifty_move_rule() {
    for depth in 1..=6 {
        let mut game =
            chessmind::fen::game_from_fen("8/8/8/4k3/8/2K5/P7/7Q w - - 0 1").expect("valid FEN");
        game.board.halfmove = 99;
        let mut engine = Engine::new(depth);
        let ((from, _), _) = engine
            .best_move_timed(&mut game, &TimeConfig::fixed_depth(depth))
            .expect("missing move");
        assert_eq!(
            from, "a2",
            "depth {depth}: only a pawn move avoids the draw"
        );
    }

    // Checkmate on the hundredth ply still wins: White Kg6, Qb1 v Kh8 (Qb8 mates).
    let mut game =
        chessmind::fen::game_from_fen("7k/8/6K1/8/8/8/8/1Q6 w - - 0 1").expect("valid FEN");
    game.board.halfmove = 99;
    let mut engine = Engine::new(3);
    let ((from, to), _) = engine
        .best_move_timed(&mut game, &TimeConfig::fixed_depth(3))
        .expect("missing move");
    assert_eq!((from.as_str(), to.as_str()), ("b1", "b8"));
}
