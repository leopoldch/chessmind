use chessmind::{
    engine::{Engine, TimeConfig},
    game::Game,
    pieces::Color,
    san::parse_san,
};
use futures_util::{SinkExt, StreamExt};
use num_cpus;
use serde::Deserialize;
use serde_json;
use std::env;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn is_coordinate(mv: &str) -> bool {
    mv.len() >= 4
        && mv.as_bytes()[0].is_ascii_lowercase()
        && mv.as_bytes()[1].is_ascii_digit()
        && mv.as_bytes()[2].is_ascii_lowercase()
        && mv.as_bytes()[3].is_ascii_digit()
}

#[derive(Deserialize)]
struct MoveEntry {
    #[serde(rename = "move")]
    mov: String,
    color: String,
}

#[derive(Deserialize, Default, Clone, Debug)]
struct TimeControl {
    #[serde(default)]
    wtime: Option<u64>,
    #[serde(default)]
    btime: Option<u64>,
    #[serde(default)]
    winc: Option<u64>,
    #[serde(default)]
    binc: Option<u64>,
    #[serde(default)]
    movestogo: Option<u32>,
    #[serde(default)]
    depth: Option<u32>,
    #[serde(default)]
    movetime: Option<u64>,
}

impl TimeControl {
    fn to_time_config(&self) -> TimeConfig {
        if self.wtime.is_some() || self.btime.is_some() {
            TimeConfig {
                wtime: self.wtime,
                btime: self.btime,
                winc: self.winc,
                binc: self.binc,
                movestogo: self.movestogo,
                depth: self.depth,
                movetime: self.movetime,
                infinite: false,
            }
        } else if let Some(depth) = self.depth {
            TimeConfig::fixed_depth(depth)
        } else if let Some(movetime) = self.movetime {
            TimeConfig::fixed_time(movetime)
        } else {
            TimeConfig {
                wtime: Some(300_000), // 5 minutes
                btime: Some(300_000),
                winc: None,
                binc: None,
                movestogo: None,
                depth: None,
                movetime: None,
                infinite: false,
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClientMsg {
    #[serde(rename = "color")]
    Color { color: String },

    #[serde(rename = "move")]
    Move {
        #[serde(rename = "move")]
        mov: String,
        #[serde(flatten)]
        time: Option<TimeControl>,
    },

    #[serde(rename = "moves")]
    Moves {
        moves: Vec<MoveEntry>,
        #[serde(flatten)]
        time: Option<TimeControl>,
    },

    #[serde(rename = "go")]
    Go {
        #[serde(flatten)]
        time: Option<TimeControl>,
    },

    #[serde(rename = "time")]
    Time {
        #[serde(flatten)]
        time: TimeControl,
    },

    #[serde(rename = "stop")]
    Stop,

    #[serde(rename = "newgame")]
    NewGame,
}

#[tokio::main]
async fn main() {
    let port = env::args().nth(1).unwrap_or_else(|| "8771".into());
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await.expect("bind");
    println!("WebSocket server on ws://{}", addr);
    println!("Supports time control: wtime, btime, winc, binc, movestogo, depth, movetime");
    while let Ok((stream, addr)) = listener.accept().await {
        println!("Client connected: {}", addr);
        tokio::spawn(handle_conn(stream, addr));
    }
}

async fn handle_conn(stream: tokio::net::TcpStream, addr: std::net::SocketAddr) {
    let ws_stream = accept_async(stream).await.expect("ws accept");
    let (mut write, mut read) = ws_stream.split();

    let mut game = Game::new();
    let mut engine = Engine::from_env(6, num_cpus::get());
    if let Ok(Some(path)) = engine.load_syzygy_from_env() {
        println!("Loaded Syzygy tablebases from {}", path);
    }

    let mut my_color: Option<Color> = None;
    let mut last_len: usize = 0;
    let mut current_time_control = TimeControl::default();

    while let Some(msg) = read.next().await {
        if let Ok(msg) = msg {
            if !msg.is_text() {
                continue;
            }
            let txt = msg.to_text().unwrap();
            println!("Received from {}: {}", addr, txt);

            if let Ok(data) = serde_json::from_str::<ClientMsg>(txt) {
                match data {
                    ClientMsg::Color { color } => {
                        my_color = match color.as_str() {
                            "white" => Some(Color::White),
                            _ => Some(Color::Black),
                        };
                        println!(
                            "AI colour set to: {}",
                            if my_color == Some(Color::White) {
                                "White"
                            } else {
                                "Black"
                            }
                        );
                        game = Game::new();
                        last_len = 0;

                        if my_color == Some(Color::White) && game.current_turn == Color::White {
                            let time_config = current_time_control.to_time_config();
                            let time_config = current_time_control.to_time_config();
                            if let Some(((s, e), depth)) =
                                engine.best_move_timed(&mut game, &time_config)
                            {
                                println!("AI calculated depth: {}", depth);
                                game.make_move(&s, &e);
                                last_len = 1;
                                let _ = write.send(Message::Text(format!("{}{}", s, e))).await;
                            }
                        }
                        continue;
                    }

                    ClientMsg::Move { mov, time } => {
                        if let Some(tc) = time {
                            current_time_control = tc;
                        }

                        let mov = mov.replace('+', "").replace('#', "");
                        if is_coordinate(&mov) {
                            game.make_move(&mov[0..2], &mov[2..4]);
                            last_len += 1;
                        } else {
                            let color = game.current_turn;
                            if let Some((s, e)) = parse_san(&mut game, &mov, color) {
                                game.make_move(&s, &e);
                                last_len += 1;
                            }
                        }
                    }

                    ClientMsg::Moves { moves, time } => {
                        if let Some(tc) = time {
                            current_time_control = tc;
                        }

                        if moves.len() == last_len {
                            continue;
                        }

                        game = Game::new();
                        for entry in &moves {
                            let mv = entry.mov.replace('+', "").replace('#', "");
                            let color = if entry.color.to_lowercase().starts_with('w') {
                                Color::White
                            } else {
                                Color::Black
                            };
                            if is_coordinate(&mv) {
                                game.make_move(&mv[0..2], &mv[2..4]);
                            } else if let Some((s, e)) = parse_san(&mut game, &mv, color) {
                                game.make_move(&s, &e);
                            }
                        }
                        last_len = moves.len();
                    }

                    ClientMsg::Go { time } => {
                        if let Some(tc) = time {
                            current_time_control = tc;
                        }

                        let time_config = current_time_control.to_time_config();
                        let start_time = Instant::now();
                        if let Some(((s, e), depth)) =
                            engine.best_move_timed(&mut game, &time_config)
                        {
                            println!(
                                "AI calculation took {:?} (depth: {})",
                                start_time.elapsed(),
                                depth
                            );
                            game.make_move(&s, &e);
                            last_len += 1;
                            let msg = serde_json::json!({
                                "next_move": format!("{}{}", s, e),
                                "time_ms": start_time.elapsed().as_millis()
                            })
                            .to_string();
                            let _ = write.send(Message::Text(msg)).await;
                        }
                        continue;
                    }

                    ClientMsg::Time { time } => {
                        current_time_control = time;
                        println!("Time control updated: {:?}", current_time_control);
                        continue;
                    }

                    ClientMsg::Stop => {
                        engine.stop();
                        println!("Search stopped");
                        continue;
                    }

                    ClientMsg::NewGame => {
                        game = Game::new();
                        last_len = 0;
                        current_time_control = TimeControl::default();
                        println!("New game started");
                        continue;
                    }
                }
            } else if is_coordinate(txt) {
                game.make_move(&txt[0..2], &txt[2..4]);
                last_len += 1;
            } else {
                let color = game.current_turn;
                if let Some((s, e)) = parse_san(&mut game, txt, color) {
                    game.make_move(&s, &e);
                    last_len += 1;
                }
            }

            if let Some(color) = my_color {
                if let Some(res) = game.result {
                    let result = if res == Color::White {
                        "white"
                    } else {
                        "black"
                    };
                    let _ = write
                        .send(Message::Text(format!("{{\"result\":\"{}\"}}", result)))
                        .await;
                    game = Game::new();
                    last_len = 0;
                    my_color = None;
                    continue;
                }

                if game.current_turn != color {
                    continue;
                }

                let time_config = current_time_control.to_time_config();
                let start_time = Instant::now();

                let next = if color == Color::White && last_len == 0 {
                    Some((("d2".to_string(), "d4".to_string()), 0))
                } else {
                    engine.best_move_timed(&mut game, &time_config)
                };

                println!("AI calculation took {:?}", start_time.elapsed());

                if let Some(((s, e), depth)) = next {
                    if depth > 0 {
                        println!("AI calculated depth: {}", depth);
                    }
                    game.make_move(&s, &e);
                    last_len += 1;
                    let msg = serde_json::json!({
                        "next_move": format!("{}{}", s, e),
                        "time_ms": start_time.elapsed().as_millis()
                    })
                    .to_string();
                    let _ = write.send(Message::Text(msg)).await;
                }
            }
        }
    }
    println!("Client disconnected: {}", addr);
}
