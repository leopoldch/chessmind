pub mod pieces;
pub mod board;
pub mod game;
pub mod engine;
pub mod san;
pub mod transposition;

#[cfg(test)]
mod tests {
    use crate::{game::Game, engine::Engine};

    #[test]
    fn engine_returns_move() {
        let mut game = Game::new();
        let mut engine = Engine::new(2);
        let mv = engine.best_move(&mut game);
        assert!(mv.is_some());
    }
}
