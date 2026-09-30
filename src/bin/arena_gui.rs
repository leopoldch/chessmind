use chessmind::{
    engine::Engine,
    game::Game,
    pieces::{Color, Piece, PieceType},
};
use eframe::{App, Frame, egui};
use egui::Color32;
use num_cpus;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

/// Result of a background engine search; the engine travels back with it.
struct SearchDone {
    engine: Engine,
    mv: Option<(String, String)>,
    generation: u64,
}

fn clone_game(game: &Game) -> Game {
    Game {
        board: game.board.clone(),
        current_turn: game.current_turn,
        history: game.history.clone(),
        hash_history: game.hash_history.clone(),
        hash_counts: game.hash_counts.clone(),
        result: game.result,
    }
}

#[derive(PartialEq)]
enum Opponent {
    AiVsAi,
    AiVsRandom,
}

pub struct ArenaApp {
    /// `None` while the engine is lent to the search thread.
    engine: Option<Engine>,
    search_rx: Option<Receiver<SearchDone>>,
    /// Bumped on reset so a search from a previous run is ignored.
    generation: u64,
    game: Game,
    opponent: Opponent,
    num_games: u32,
    games_played: u32,
    wins: u32,
    draws: u32,
    running: bool,
    last_move: Instant,
    move_delay: Duration,
}

impl ArenaApp {
    pub fn new() -> Self {
        Self {
            engine: {
                let mut eng = Engine::from_env(6, num_cpus::get());
                if let Ok(Some(path)) = eng.load_syzygy_from_env() {
                    println!("Loaded Syzygy tablebases from {}", path);
                }
                Some(eng)
            },
            search_rx: None,
            generation: 0,
            game: Game::new(),
            opponent: Opponent::AiVsAi,
            num_games: 10,
            games_played: 0,
            wins: 0,
            draws: 0,
            running: false,
            last_move: Instant::now(),
            move_delay: Duration::from_millis(300),
        }
    }

    fn reset(&mut self) {
        self.generation += 1;
        self.game = Game::new();
        self.games_played = 0;
        self.wins = 0;
        self.draws = 0;
        self.last_move = Instant::now();
    }

    /// Plays the move of a finished background search, if any.
    fn poll_search(&mut self) {
        let Some(rx) = &self.search_rx else {
            return;
        };
        let done = match rx.try_recv() {
            Ok(done) => done,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                eprintln!("Engine search thread died; recreating the engine");
                self.search_rx = None;
                self.engine = Some(Engine::from_env(6, num_cpus::get()));
                return;
            }
        };
        self.search_rx = None;
        self.engine = Some(done.engine);
        if !self.running || done.generation != self.generation {
            return;
        }
        if let Some((s, e)) = done.mv {
            self.game.make_move(&s, &e);
        }
        self.last_move = Instant::now();
    }

    /// Starts the engine search for the side to move on a worker thread.
    fn spawn_search(&mut self, ctx: &egui::Context) {
        let Some(mut engine) = self.engine.take() else {
            return;
        };
        let mut game = clone_game(&self.game);
        let generation = self.generation;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.search_rx = Some(rx);
        thread::spawn(move || {
            let mv = engine.best_move(&mut game);
            let _ = tx.send(SearchDone {
                engine,
                mv,
                generation,
            });
            ctx.request_repaint();
        });
    }

    fn step(&mut self, ctx: &egui::Context) {
        let legal = self.game.legal_moves();
        if legal.is_empty() {
            if self.game.board.in_check(self.game.current_turn) {
                if let Some(winner) = self.game.result {
                    if winner == Color::White {
                        self.wins += 1;
                    }
                }
            } else {
                self.draws += 1;
            }
            self.games_played += 1;
            if self.games_played >= self.num_games {
                self.running = false;
                return;
            }
            self.game = Game::new();
            self.last_move = Instant::now();
            return;
        }

        let engine_to_move = match self.opponent {
            Opponent::AiVsAi => true,
            Opponent::AiVsRandom => self.game.current_turn == Color::White,
        };
        if engine_to_move {
            // The move is played by `poll_search` once the worker returns.
            self.spawn_search(ctx);
            return;
        }

        let mut rng = thread_rng();
        if let Some((s, e)) = legal.choose(&mut rng).cloned() {
            self.game.make_move(&s, &e);
        }
        self.last_move = Instant::now();
    }

    fn piece_char(piece: &Piece) -> char {
        match (piece.piece_type, piece.color) {
            (PieceType::King, Color::White) => '♔',
            (PieceType::Queen, Color::White) => '♕',
            (PieceType::Rook, Color::White) => '♖',
            (PieceType::Bishop, Color::White) => '♗',
            (PieceType::Knight, Color::White) => '♘',
            (PieceType::Pawn, Color::White) => '♙',
            (PieceType::King, Color::Black) => '♚',
            (PieceType::Queen, Color::Black) => '♛',
            (PieceType::Rook, Color::Black) => '♜',
            (PieceType::Bishop, Color::Black) => '♝',
            (PieceType::Knight, Color::Black) => '♞',
            (PieceType::Pawn, Color::Black) => '♟',
        }
    }
}

impl App for ArenaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Wins: {}", self.wins));
                ui.label(format!("Draws: {}", self.draws));
                ui.label(format!("Total: {}", self.games_played));
                let wr = if self.games_played > 0 {
                    self.wins as f32 / self.games_played as f32 * 100.0
                } else {
                    0.0
                };
                ui.label(format!("Winrate: {:.1}%", wr));
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Games:");
                ui.add(egui::DragValue::new(&mut self.num_games).clamp_range(1..=1000));
                ui.radio_value(&mut self.opponent, Opponent::AiVsAi, "AI vs AI");
                ui.radio_value(&mut self.opponent, Opponent::AiVsRandom, "AI vs Random");
                let button = if self.running { "Stop" } else { "Start" };
                if ui.button(button).clicked() {
                    if self.running {
                        self.running = false;
                    } else {
                        self.reset();
                        self.running = true;
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let board_size = ui.available_width().min(ui.available_height());
            let square_size = board_size / 8.0;
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(board_size, board_size), egui::Sense::hover());

            let painter = ui.painter();
            for x in 0..8 {
                for y in 0..8 {
                    let sq_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            rect.left() + x as f32 * square_size,
                            rect.top() + (7 - y) as f32 * square_size,
                        ),
                        egui::vec2(square_size, square_size),
                    );
                    let light = Color32::from_rgb(240, 217, 181);
                    let dark = Color32::from_rgb(181, 136, 99);
                    let color = if (x + y) % 2 == 0 { light } else { dark };
                    painter.rect_filled(sq_rect, 0.0, color);
                }
            }

            for x in 0..8 {
                for y in 0..8 {
                    if let Some(p) = self.game.board.get_index(x, y) {
                        let sq_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                rect.left() + x as f32 * square_size,
                                rect.top() + (7 - y) as f32 * square_size,
                            ),
                            egui::vec2(square_size, square_size),
                        );
                        painter.text(
                            sq_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            Self::piece_char(&p),
                            egui::FontId::proportional(square_size * 0.8),
                            if p.color == Color::White {
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::BLACK
                            },
                        );
                    }
                }
            }
        });

        self.poll_search();

        // Idle or waiting on a search: no repaint needed (the search thread
        // wakes the UI when it is done).
        if self.running && self.search_rx.is_none() {
            let elapsed = self.last_move.elapsed();
            if elapsed >= self.move_delay {
                self.step(ctx);
                if self.search_rx.is_none() {
                    ctx.request_repaint_after(self.move_delay);
                }
            } else {
                ctx.request_repaint_after(self.move_delay - elapsed);
            }
        }
    }
}

fn main() {
    let options = eframe::NativeOptions::default();
    eframe::run_native("Arena", options, Box::new(|_| Box::new(ArenaApp::new()))).unwrap();
}
