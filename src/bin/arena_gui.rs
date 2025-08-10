use chessmind::{
    engine::Engine,
    game::Game,
    pieces::{Color, Piece, PieceType},
};
use eframe::{egui, App, Frame};
use egui::Color32;
use num_cpus;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::time::{Duration, Instant};

#[derive(PartialEq)]
enum Opponent {
    AiVsAi,
    AiVsRandom,
}

pub struct ArenaApp {
    engine: Engine,
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
            engine: Engine::with_threads(6, num_cpus::get()),
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
        self.game = Game::new();
        self.games_played = 0;
        self.wins = 0;
        self.draws = 0;
        self.last_move = Instant::now();
    }

    fn step(&mut self) {
        // Check end of game
        let legal = self.game.legal_moves();
        if legal.is_empty() {
            if self.game.board.in_check(self.game.current_turn) {
                // checkmate
                if let Some(winner) = self.game.result {
                    if winner == Color::White {
                        self.wins += 1;
                    }
                }
            } else {
                // stalemate
                self.draws += 1;
            }
            self.games_played += 1;
            if self.games_played >= self.num_games {
                self.running = false;
                return;
            }
            self.game = Game::new();
            return;
        }

        let mv = match self.opponent {
            Opponent::AiVsAi => self.engine.best_move(&mut self.game),
            Opponent::AiVsRandom => {
                if self.game.current_turn == Color::White {
                    self.engine.best_move(&mut self.game)
                } else {
                    let mut rng = thread_rng();
                    legal.choose(&mut rng).cloned()
                }
            }
        };

        if let Some((s, e)) = mv {
            self.game.make_move(&s, &e);
        }
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
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(board_size, board_size),
                egui::Sense::hover(),
            );

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

        if self.running && self.last_move.elapsed() >= self.move_delay {
            self.step();
            self.last_move = Instant::now();
        }

        ctx.request_repaint();
    }
}

fn main() {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Arena",
        options,
        Box::new(|_| Box::new(ArenaApp::new())),
    )
    .unwrap();
}

