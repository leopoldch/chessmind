use chessmind::engine::{Engine, TimeConfig};
use chessmind::eval;
use chessmind::game::Game;
use chessmind::pieces::Color;

fn main() {
    println!("=== Chess Engine Search Test ===\n");

    let mut game = Game::new();
    let mut engine = Engine::new(6); // Fixed depth 6

    // First, verify evaluation works on starting position
    println!("=== EVALUATION TEST ===");
    let eval_white = eval::evaluate(&game.board, Color::White);
    let eval_black = eval::evaluate(&game.board, Color::Black);
    println!("Starting position eval (White perspective): {}", eval_white);
    println!("Starting position eval (Black perspective): {}", eval_black);

    // Make several moves to get out of opening book
    // Playing a common opening that the book might not cover deeply
    game.make_move("e2", "e4"); // 1.e4
    game.make_move("e7", "e5"); // 1...e5  
    game.make_move("g1", "f3"); // 2.Nf3
    game.make_move("b8", "c6"); // 2...Nc6
    game.make_move("f1", "b5"); // 3.Bb5 
    game.make_move("a7", "a6"); // 3...a6
    game.make_move("b5", "a4"); // 4.Ba4
    game.make_move("g8", "f6"); // 4...Nf6
    game.make_move("e1", "g1"); // 5.O-O (castling)
    game.make_move("f8", "e7"); // 5...Be7
    game.make_move("d2", "d4"); // 6.d4
    game.make_move("e5", "d4"); // 6...exd4 (capture, out of most books)

    // Evaluate after moves
    let eval_mid_white = eval::evaluate(&game.board, Color::White);
    let eval_mid_black = eval::evaluate(&game.board, Color::Black);
    println!("Position after 12 plies eval (White): {}", eval_mid_white);
    println!("Position after 12 plies eval (Black): {}", eval_mid_black);

    println!("\nPosition after 6 moves each side (probably out of book)");
    println!("Hash history length: {}", game.hash_history.len());
    println!("Current turn: {:?}\n", game.current_turn);

    // Test with fixed depth
    let config = TimeConfig::fixed_depth(5);
    println!("Searching at fixed depth 5...\n");

    let start = std::time::Instant::now();
    let result = engine.best_move_timed(&mut game, &config);
    let elapsed = start.elapsed();

    match result {
        Some(((from, to), depth)) => {
            println!("\n=== RESULT ===");
            println!("Best move: {} -> {}", from, to);
            println!("Reached depth: {}", depth);
            println!("Time: {:?}", elapsed);
        }
        None => {
            println!("\n=== RESULT ===");
            println!("No move found!");
            println!("Time: {:?}", elapsed);
        }
    }
}
