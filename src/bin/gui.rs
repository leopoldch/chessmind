use chessmind::{game::Game, board::Board, pieces::{Piece, PieceType, Color}};
use eframe::{egui, App, Frame};
use egui::Color32;

pub struct GuiApp {
    game: Game,
    dragging: Option<(usize, usize, Piece)>,
    drag_pos: egui::Pos2,
}

impl GuiApp {
    pub fn new() -> Self {
        Self { game: Game::new(), dragging: None, drag_pos: egui::Pos2::ZERO }
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
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            if ui.button("Restart").clicked() {
                self.game = Game::new();
                self.dragging = None;
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let board_size = ui.available_width().min(ui.available_height());
            let square_size = board_size / 8.0;
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(board_size, board_size),
                egui::Sense::click_and_drag(),
            );

            if let Some(pos) = response.interact_pointer_pos() {
                self.drag_pos = pos;
            }

            if response.drag_stopped() {
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
                    let light = Color32::from_rgb(240,217,181);
                    let dark  = Color32::from_rgb(181,136,99);
                    let color = if (x + y) % 2 == 0 { light } else { dark };
                    painter.rect_filled(sq_rect, 0.0, color);
                }
            }

            for x in 0..8 {
                for y in 0..8 {
                    let piece_opt = self.game.board.get_index(x, y);
                    if let Some(p) = piece_opt {
                        if let Some((dx, dy, _)) = self.dragging {
                            if dx == x && dy == y { continue; }
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
                            if p.color == Color::White { egui::Color32::BLACK } else { egui::Color32::WHITE },
                        );
                    }
                }
            }

            if response.drag_started() {
                if rect.contains(self.drag_pos) {
                    let fx = ((self.drag_pos.x - rect.left()) / square_size).floor() as i32;
                    let fy = 7 - ((self.drag_pos.y - rect.top()) / square_size).floor() as i32;
                    if fx >= 0 && fx < 8 && fy >= 0 && fy < 8 {
                        if let Some(p) = self.game.board.get_index(fx as usize, fy as usize) {
                            self.dragging = Some((fx as usize, fy as usize, p));
                            self.game.board.set_index(fx as usize, fy as usize, None);
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
                    if p.color == Color::White { egui::Color32::BLACK } else { egui::Color32::WHITE },
                );
            }
        });
    }
}

fn main() {
    let options = eframe::NativeOptions::default();
    eframe::run_native("Chessmind", options, Box::new(|_cc| Box::new(GuiApp::new()))).unwrap();
}
