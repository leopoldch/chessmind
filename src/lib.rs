pub mod board;
pub mod engine;
pub mod game;
pub mod movegen;
pub mod opening;
pub mod pieces;
pub mod san;
pub mod transposition;

#[cfg(test)]
mod tests {
    use crate::{engine::Engine, game::Game};
    use num_cpus;

    #[test]
    fn engine_returns_move() {
        let mut game = Game::new();
        let mut engine = Engine::with_threads(2, num_cpus::get());
        let mv = engine.best_move(&mut game);
        assert!(mv.is_some());
    }
}
