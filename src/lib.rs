pub mod pieces;
pub mod board;
pub mod game;
pub mod engine;
pub mod san;
pub mod transposition;
pub mod movegen;

#[cfg(test)]
mod tests {
    use crate::{game::Game, engine::Engine};
    use num_cpus;

    #[test]
    fn engine_returns_move() {
        let mut game = Game::new();
        let mut engine = Engine::with_threads(2, num_cpus::get());
        let mv = engine.best_move(&mut game);
        assert!(mv.is_some());
    }
}
