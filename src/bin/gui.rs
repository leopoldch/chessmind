use chessmind::{
    board::Board,
    engine::{Engine, TimeConfig},
    game::Game,
    pieces::{Color, Piece, PieceType},
};
use eframe::{App, Frame, egui};
use egui::Color32;
use num_cpus;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
enum TimePreset {
    Bullet1,   // 1+0
    Bullet2,   // 2+1
    Blitz3,    // 3+0
    Blitz3Inc, // 3+2
    Blitz5,    // 5+0
    Blitz5Inc, // 5+3
    Rapid10,   // 10+0
    Rapid15,   // 15+10
    Classical, // 30+0
    Custom,    // User-defined
    Unlimited, // No time limit (fixed depth)
}

impl TimePreset {
    fn name(&self) -> &'static str {
        match self {
            TimePreset::Bullet1 => "1+0 (Bullet)",
            TimePreset::Bullet2 => "2+1 (Bullet)",
            TimePreset::Blitz3 => "3+0 (Blitz)",
            TimePreset::Blitz3Inc => "3+2 (Blitz)",
            TimePreset::Blitz5 => "5+0 (Blitz)",
            TimePreset::Blitz5Inc => "5+3 (Blitz)",
            TimePreset::Rapid10 => "10+0 (Rapid)",
            TimePreset::Rapid15 => "15+10 (Rapid)",
            TimePreset::Classical => "30+0 (Classical)",
            TimePreset::Custom => "Custom",
            TimePreset::Unlimited => "Unlimited (Depth 8)",
        }
    }

    fn base_time_secs(&self) -> u32 {
        match self {
            TimePreset::Bullet1 => 60,
            TimePreset::Bullet2 => 120,
            TimePreset::Blitz3 | TimePreset::Blitz3Inc => 180,
            TimePreset::Blitz5 | TimePreset::Blitz5Inc => 300,
            TimePreset::Rapid10 => 600,
            TimePreset::Rapid15 => 900,
            TimePreset::Classical => 1800,
            TimePreset::Custom => 300,
            TimePreset::Unlimited => 0,
        }
    }

    fn increment_secs(&self) -> u32 {
        match self {
            TimePreset::Bullet2 => 1,
            TimePreset::Blitz3Inc => 2,
            TimePreset::Blitz5Inc => 3,
            TimePreset::Rapid15 => 10,
            _ => 0,
        }
    }
}

struct ChessClock {
    white_time_ms: u64,
    black_time_ms: u64,
    increment_ms: u64,
    last_update: Instant,
    running: bool,
    active_color: Color,
}

impl ChessClock {
    fn new(base_time_secs: u32, increment_secs: u32) -> Self {
        Self {
            white_time_ms: base_time_secs as u64 * 1000,
            black_time_ms: base_time_secs as u64 * 1000,
            increment_ms: increment_secs as u64 * 1000,
            last_update: Instant::now(),
            running: false,
            active_color: Color::White,
        }
    }

    fn start(&mut self, color: Color) {
        self.running = true;
        self.active_color = color;
        self.last_update = Instant::now();
    }

    fn stop(&mut self) {
        self.update();
        self.running = false;
    }

    fn switch(&mut self, new_color: Color) {
        self.update();
        if self.active_color == Color::White {
            self.white_time_ms = self.white_time_ms.saturating_add(self.increment_ms);
        } else {
            self.black_time_ms = self.black_time_ms.saturating_add(self.increment_ms);
        }
        self.active_color = new_color;
        self.last_update = Instant::now();
    }

    fn update(&mut self) {
        if !self.running {
            return;
        }
        let elapsed = self.last_update.elapsed().as_millis() as u64;
        self.last_update = Instant::now();

        if self.active_color == Color::White {
            self.white_time_ms = self.white_time_ms.saturating_sub(elapsed);
        } else {
            self.black_time_ms = self.black_time_ms.saturating_sub(elapsed);
        }
    }

    fn get_time(&mut self, color: Color) -> u64 {
        self.update();
        if color == Color::White {
            self.white_time_ms
        } else {
            self.black_time_ms
        }
    }

    fn is_flagged(&mut self, color: Color) -> bool {
        self.get_time(color) == 0
    }

    fn format_time(ms: u64) -> String {
        let total_secs = ms / 1000;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        let tenths = (ms % 1000) / 100;

        if total_secs < 10 {
            format!("{}:{:02}.{}", mins, secs, tenths)
        } else {
            format!("{}:{:02}", mins, secs)
        }
    }

    fn reset(&mut self, base_time_secs: u32, increment_secs: u32) {
        self.white_time_ms = base_time_secs as u64 * 1000;
        self.black_time_ms = base_time_secs as u64 * 1000;
        self.increment_ms = increment_secs as u64 * 1000;
        self.running = false;
        self.active_color = Color::White;
        self.last_update = Instant::now();
    }
}

pub struct GuiApp {
    game: Game,
    engine: Engine,
    vs_ai: bool,
    ai_color: Color,
    dragging: Option<(usize, usize, Piece)>,
    drag_pos: egui::Pos2,
    last_ai_time: Option<Duration>,

    clock: ChessClock,
    time_preset: TimePreset,
    custom_base_mins: u32,
    custom_increment_secs: u32,
    use_clock: bool,
    game_started: bool,

    flag_winner: Option<Color>,
}

impl GuiApp {
    pub fn new() -> Self {
        Self {
            game: Game::new(),
            engine: {
                let mut eng = Engine::from_env(8, num_cpus::get());
                if let Ok(Some(path)) = eng.load_syzygy_from_env() {
                    println!("Loaded Syzygy tablebases from {}", path);
                }
                eng
            },
            vs_ai: false,
            ai_color: Color::Black,
            dragging: None,
            drag_pos: egui::Pos2::ZERO,
            last_ai_time: None,

            clock: ChessClock::new(300, 0), // 5+0 default
            time_preset: TimePreset::Blitz5,
            custom_base_mins: 5,
            custom_increment_secs: 0,
            use_clock: true,
            game_started: false,
            flag_winner: None,
        }
    }

    fn get_time_config(&mut self) -> TimeConfig {
        if !self.use_clock || self.time_preset == TimePreset::Unlimited {
            return TimeConfig::fixed_depth(8);
        }

        self.clock.update();
        TimeConfig {
            wtime: Some(self.clock.white_time_ms),
            btime: Some(self.clock.black_time_ms),
            winc: Some(self.clock.increment_ms),
            binc: Some(self.clock.increment_ms),
            movestogo: None,
            depth: None,
            movetime: None,
            infinite: false,
        }
    }

    fn check_ai_move(&mut self) {
        if self.flag_winner.is_some() {
            return;
        }

        if self.use_clock && self.game_started && self.time_preset != TimePreset::Unlimited {
            self.clock.update();
            if self.clock.is_flagged(Color::White) {
                self.flag_winner = Some(Color::Black);
                self.clock.stop();
                return;
            }
            if self.clock.is_flagged(Color::Black) {
                self.flag_winner = Some(Color::White);
                self.clock.stop();
                return;
            }
        }

        if self.vs_ai && self.game.result.is_none() && self.game.current_turn == self.ai_color {
            let time_config = self.get_time_config();
            let start = Instant::now();

            if let Some(((s, e), depth)) = self.engine.best_move_timed(&mut self.game, &time_config)
            {
                let duration = start.elapsed();
                self.game.make_move(&s, &e);
                self.last_ai_time = Some(duration);

                if self.use_clock && self.game_started {
                    let next_color = if self.ai_color == Color::White {
                        Color::Black
                    } else {
                        Color::White
                    };
                    self.clock.switch(next_color);
                }

                println!("AI move {s}{e} in {:?} (depth {})", duration, depth);
            }
        }
    }

    fn on_player_move(&mut self) {
        if !self.game_started {
            self.game_started = true;
            if self.use_clock && self.time_preset != TimePreset::Unlimited {
                self.clock.start(self.game.current_turn);
            }
        } else if self.use_clock && self.time_preset != TimePreset::Unlimited {
            self.clock.switch(self.game.current_turn);
        }
    }

    fn restart_game(&mut self) {
        self.game = Game::new();
        self.dragging = None;
        self.game_started = false;
        self.flag_winner = None;

        if self.time_preset == TimePreset::Custom {
            self.clock
                .reset(self.custom_base_mins * 60, self.custom_increment_secs);
        } else {
            self.clock.reset(
                self.time_preset.base_time_secs(),
                self.time_preset.increment_secs(),
            );
        }

        self.check_ai_move();
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
        if self.use_clock && self.game_started && self.clock.running {
            ctx.request_repaint();
        }

        self.check_ai_move();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("🔄 Restart").clicked() {
                    self.restart_game();
                }

                ui.separator();

                ui.checkbox(&mut self.vs_ai, "Play vs AI");

                if self.vs_ai {
                    ui.label("AI plays:");
                    ui.radio_value(&mut self.ai_color, Color::White, "White");
                    ui.radio_value(&mut self.ai_color, Color::Black, "Black");
                }
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.use_clock, "Use Clock");

                if self.use_clock {
                    ui.separator();

                    egui::ComboBox::from_label("Time Control")
                        .selected_text(self.time_preset.name())
                        .show_ui(ui, |ui| {
                            let presets = [
                                TimePreset::Bullet1,
                                TimePreset::Bullet2,
                                TimePreset::Blitz3,
                                TimePreset::Blitz3Inc,
                                TimePreset::Blitz5,
                                TimePreset::Blitz5Inc,
                                TimePreset::Rapid10,
                                TimePreset::Rapid15,
                                TimePreset::Classical,
                                TimePreset::Custom,
                                TimePreset::Unlimited,
                            ];
                            for preset in presets {
                                if ui
                                    .selectable_value(&mut self.time_preset, preset, preset.name())
                                    .changed()
                                {
                                    if preset != TimePreset::Custom {
                                        self.clock.reset(
                                            preset.base_time_secs(),
                                            preset.increment_secs(),
                                        );
                                    }
                                }
                            }
                        });

                    if self.time_preset == TimePreset::Custom {
                        ui.label("Base (min):");
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.custom_base_mins)
                                    .clamp_range(1..=120),
                            )
                            .changed()
                        {
                            self.clock
                                .reset(self.custom_base_mins * 60, self.custom_increment_secs);
                        }
                        ui.label("Inc (sec):");
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.custom_increment_secs)
                                    .clamp_range(0..=60),
                            )
                            .changed()
                        {
                            self.clock
                                .reset(self.custom_base_mins * 60, self.custom_increment_secs);
                        }
                    }
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                if let Some(winner) = self.flag_winner {
                    let winner_str = if winner == Color::White {
                        "White"
                    } else {
                        "Black"
                    };
                    ui.label(
                        egui::RichText::new(format!("⏱ {} wins on time!", winner_str))
                            .color(Color32::RED)
                            .strong(),
                    );
                } else if let Some(res) = self.game.result {
                    let winner_str = if res == Color::White {
                        "White"
                    } else {
                        "Black"
                    };
                    ui.label(
                        egui::RichText::new(format!("♚ {} wins by checkmate!", winner_str))
                            .color(Color32::GOLD)
                            .strong(),
                    );
                } else {
                    let turn_str = if self.game.current_turn == Color::White {
                        "White"
                    } else {
                        "Black"
                    };
                    ui.label(format!("Turn: {}", turn_str));

                    if let Some(t) = self.last_ai_time {
                        ui.separator();
                        ui.label(format!("AI: {:.2?}", t));
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let available_height = ui.available_height();
                let board_size = ui.available_width().min(available_height) * 0.85;
                let square_size = board_size / 8.0;

                ui.vertical(|ui| {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(board_size, board_size),
                        egui::Sense::click_and_drag(),
                    );

                    if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                        self.drag_pos = pos;
                    }

                    let drag_released = response.drag_stopped()
                        || (self.dragging.is_some() && !ctx.input(|i| i.pointer.any_down()));

                    if drag_released {
                        if let Some((sx, sy, piece)) = self.dragging.take() {
                            if rect.contains(self.drag_pos) {
                                let fx =
                                    ((self.drag_pos.x - rect.left()) / square_size).floor() as i32;
                                let fy = 7
                                    - ((self.drag_pos.y - rect.top()) / square_size).floor() as i32;
                                if fx >= 0 && fx < 8 && fy >= 0 && fy < 8 {
                                    if let (Some(start), Some(end)) = (
                                        Board::index_to_algebraic(sx, sy),
                                        Board::index_to_algebraic(fx as usize, fy as usize),
                                    ) {
                                        if !self.game.make_move(&start, &end) {
                                            self.game.board.set_index(sx, sy, Some(piece));
                                        } else {
                                            self.on_player_move();
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
                                        egui::Color32::WHITE
                                    } else {
                                        egui::Color32::BLACK
                                    },
                                );
                            }
                        }
                    }

                    if response.drag_started() {
                        if rect.contains(self.drag_pos) {
                            let fx =
                                ((self.drag_pos.x - rect.left()) / square_size).floor() as usize;
                            let fy =
                                7 - ((self.drag_pos.y - rect.top()) / square_size).floor() as usize;

                            if fx < 8 && fy < 8 {
                                if let Some(p) = self.game.board.get_index(fx, fy) {
                                    self.dragging = Some((fx, fy, p));
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
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::BLACK
                            },
                        );
                    }
                });

                ui.separator();

                ui.vertical(|ui| {
                    ui.set_min_width(120.0);

                    self.clock.update();

                    let black_time = self.clock.black_time_ms;
                    let black_active =
                        self.clock.running && self.clock.active_color == Color::Black;

                    ui.group(|ui| {
                        ui.set_min_height(80.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("Black").size(14.0));
                            let time_text = ChessClock::format_time(black_time);
                            let color = if black_time == 0 {
                                Color32::RED
                            } else if black_active {
                                Color32::GREEN
                            } else {
                                Color32::GRAY
                            };
                            ui.label(
                                egui::RichText::new(time_text)
                                    .size(28.0)
                                    .color(color)
                                    .monospace(),
                            );
                            if black_active {
                                ui.label(egui::RichText::new("●").color(Color32::GREEN));
                            }
                        });
                    });

                    ui.add_space(20.0);

                    ui.group(|ui| {
                        ui.vertical_centered(|ui| {
                            let move_num = (self.game.history.len() / 2) + 1;
                            ui.label(format!("Move {}", move_num));
                        });
                    });

                    ui.add_space(20.0);

                    let white_time = self.clock.white_time_ms;
                    let white_active =
                        self.clock.running && self.clock.active_color == Color::White;

                    ui.group(|ui| {
                        ui.set_min_height(80.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("White").size(14.0));
                            let time_text = ChessClock::format_time(white_time);
                            let color = if white_time == 0 {
                                Color32::RED
                            } else if white_active {
                                Color32::GREEN
                            } else {
                                Color32::GRAY
                            };
                            ui.label(
                                egui::RichText::new(time_text)
                                    .size(28.0)
                                    .color(color)
                                    .monospace(),
                            );
                            if white_active {
                                ui.label(egui::RichText::new("●").color(Color32::GREEN));
                            }
                        });
                    });
                });
            });
        });
    }
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 550.0])
            .with_min_inner_size([500.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Chessmind",
        options,
        Box::new(|_cc| Box::new(GuiApp::new())),
    )
    .unwrap();
}
