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
        best: Some((12, 28)),
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
        best: Some((4, 6)),
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
