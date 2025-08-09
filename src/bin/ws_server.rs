use std::env;
use std::time::Instant;
use chessmind::{game::Game, engine::Engine, pieces::Color, san::parse_san};
use futures_util::{StreamExt, SinkExt};
use serde::Deserialize;
use serde_json;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use num_cpus;

fn is_coordinate(mv: &str) -> bool {
    mv.len() == 4
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

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClientMsg {
    #[serde(rename = "color")]
    Color { color: String },
    #[serde(rename = "move")]
    Move { #[serde(rename = "move")] mov: String },
    #[serde(rename = "moves")]
    Moves { moves: Vec<MoveEntry> },
}

#[tokio::main]
async fn main() {
    let port = env::args().nth(1).unwrap_or_else(|| "8771".into());
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await.expect("bind");
    println!("WebSocket server on ws://{}", addr);
    while let Ok((stream, addr)) = listener.accept().await {
        println!("Client connected: {}", addr);
        tokio::spawn(handle_conn(stream, addr));
    }
}

async fn handle_conn(stream: tokio::net::TcpStream, addr: std::net::SocketAddr) {
    let ws_stream = accept_async(stream).await.expect("ws accept");
    let (mut write, mut read) = ws_stream.split();
    let mut game = Game::new();
    let mut engine = Engine::with_threads(6, num_cpus::get());
    let mut my_color: Option<Color> = None;
    let mut last_len: usize = 0;
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
                            if my_color == Some(Color::White) { "White" } else { "Black" }
                        );
                        game = Game::new();
                        last_len = 0;
                        if my_color == Some(Color::White) && game.current_turn == Color::White {
                            game.make_move("d2", "d4");
                            last_len = 1;
                            let _ = write.send(Message::Text("d2d4".into())).await;
                        }
                        continue;
                    }
                    ClientMsg::Move { mov } => {
                        let mov = mov.replace('+', "");
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
                    ClientMsg::Moves { moves } => {
                        if moves.len() == last_len {
                            continue;
                        }
                        game = Game::new();
                        for entry in &moves {
                            let mv = entry.mov.replace('+', "");
                            let color = if entry.color.to_lowercase().starts_with("w") { Color::White } else { Color::Black };
                            if is_coordinate(&mv) {
                                game.make_move(&mv[0..2], &mv[2..4]);
                            } else if let Some((s, e)) = parse_san(&mut game, &mv, color) {
                                game.make_move(&s, &e);
                            }
                        }
                        last_len = moves.len();
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
                    let result = if res == Color::White { "white" } else { "black" };
                    let _ = write.send(Message::Text(format!("{{\"result\":\"{}\"}}", result))).await;
                    break;
                }
                if game.current_turn != color {
                    continue;
                }
                let start_time = Instant::now();
                let next = if color == Color::White && last_len == 0 {
                    Some(("d2".to_string(), "d4".to_string()))
                } else {
                    engine.best_move(&mut game)
                };
                println!("AI calculation took {:?}", start_time.elapsed());
                if let Some((s, e)) = next {
                    game.make_move(&s, &e);
                    last_len += 1;
                    let msg = serde_json::json!({"next_move": format!("{}{}", s, e)}).to_string();
                    let _ = write.send(Message::Text(msg)).await;
                }
            }
        }
    }
    println!("Client disconnected: {}", addr);
}
