use chessmind::{
    board::Board,
    engine::Engine,
    game::Game,
    pieces::{Color, Piece, PieceType},
};
use eframe::{App, Frame, egui};
use egui::Color32;
use num_cpus;
use std::time::{Duration, Instant};

pub struct GuiApp {
    game: Game,
    engine: Engine,
    vs_ai: bool,
    ai_color: Color,
    dragging: Option<(usize, usize, Piece)>,
    drag_pos: egui::Pos2,
    last_ai_time: Option<Duration>,
}

impl GuiApp {
    pub fn new() -> Self {
        Self {
            game: Game::new(),
            engine: Engine::with_threads(3, num_cpus::get()),
            vs_ai: false,
            ai_color: Color::Black,
            dragging: None,
            drag_pos: egui::Pos2::ZERO,
            last_ai_time: None,
        }
    }

    fn check_ai_move(&mut self) {
        if self.vs_ai && self.game.result.is_none() && self.game.current_turn == self.ai_color {
            let start = Instant::now();
            if let Some((s, e)) = self.engine.best_move(&mut self.game) {
                let duration = start.elapsed();
                self.game.make_move(&s, &e);
                self.last_ai_time = Some(duration);
                println!(
                    "AI move {s}{e} in {:?} (depth {})",
                    duration, self.engine.depth
                );
            }
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

impl App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.check_ai_move();
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            if ui.button("Restart").clicked() {
                self.game = Game::new();
                self.dragging = None;
                self.check_ai_move();
            }
            ui.separator();
            ui.checkbox(&mut self.vs_ai, "Play vs AI");
            if self.vs_ai {
                ui.label("AI plays:");
                ui.radio_value(&mut self.ai_color, Color::White, "White");
                ui.radio_value(&mut self.ai_color, Color::Black, "Black");
                if let Some(t) = self.last_ai_time {
                    ui.label(format!(
                        "Last AI move: {:.2?} (depth {})",
                        t, self.engine.depth
                    ));
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let board_size = ui.available_width().min(ui.available_height());
            let square_size = board_size / 8.0;
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(board_size, board_size),
                egui::Sense::click_and_drag(),
            );

            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                self.drag_pos = pos;
            }

            let drag_released = response.drag_released()
                || (self.dragging.is_some() && !ctx.input(|i| i.pointer.any_down()));
            if drag_released {
                if let Some((sx, sy, piece)) = self.dragging.take() {
                    if rect.contains(self.drag_pos) {
                        let fx = ((self.drag_pos.x - rect.left()) / square_size).floor() as i32;
                        let fy = 7 - ((self.drag_pos.y - rect.top()) / square_size).floor() as i32;
                        if fx >= 0 && fx < 8 && fy >= 0 && fy < 8 {
                            if let (Some(start), Some(end)) = (
                                Board::index_to_algebraic(sx, sy),
                                Board::index_to_algebraic(fx as usize, fy as usize),
                            ) {
                                if !self.game.make_move(&start, &end) {
                                    self.game.board.set_index(sx, sy, Some(piece));
                                } else {
                                    self.check_ai_move();
                                }
                            }
                        } else {
                            self.game.board.set_index(sx, sy, Some(piece));
                        }
                    } else {
                        self.game.board.set_index(sx, sy, Some(piece));
                    }
                }
            }

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
                    let piece_opt = self.game.board.get_index(x, y);
                    if let Some(p) = piece_opt {
                        if let Some((dx, dy, _)) = self.dragging {
                            if dx == x && dy == y {
                                continue;
                            }
                        }
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
                                egui::Color32::BLACK
                            } else {
                                egui::Color32::WHITE
                            },
                        );
                    }
                }
            }

            if response.drag_started() {
                if rect.contains(self.drag_pos) {
                    // on convertit tout de suite en usize, c’est plus simple
                    let fx = ((self.drag_pos.x - rect.left()) / square_size).floor() as usize;
                    let fy = 7 - ((self.drag_pos.y - rect.top()) / square_size).floor() as usize;

                    if fx < 8 && fy < 8 {
                        // plus besoin de vérifier < 0
                        if let Some(p) = self.game.board.get_index(fx, fy) {
                            self.dragging = Some((fx, fy, p)); // on mémorise la pièce
                            // plus de self.game.board.set_index(...) ici !
                        }
                    }
                }
            }

            if let Some((_sx, _sy, p)) = self.dragging {
                painter.text(
                    self.drag_pos,
                    egui::Align2::CENTER_CENTER,
                    Self::piece_char(&p),
                    egui::FontId::proportional(square_size * 0.8),
                    if p.color == Color::White {
                        egui::Color32::BLACK
                    } else {
                        egui::Color32::WHITE
                    },
                );
            }
        });
    }
}

fn main() {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Chessmind",
        options,
        Box::new(|_cc| Box::new(GuiApp::new())),
    )
    .unwrap();
}
