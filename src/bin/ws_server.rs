use std::env;
use std::time::Instant;
use chessmind::{game::Game, engine::Engine, pieces::Color};
use futures_util::{StreamExt, SinkExt};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

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
    let engine = Engine::new(3);
    let mut my_color: Option<Color> = None;
    while let Some(msg) = read.next().await {
        if let Ok(msg) = msg {
            if msg.is_text() {
                let txt = msg.to_text().unwrap();
                println!("Received from {}: {}", addr, txt);
                if let Ok(data) = serde_json::from_str::<ClientMsg>(txt) {
                    match data {
                        ClientMsg::Color { color } => {
                            my_color = match color.as_str() {
                                "white" => Some(Color::White),
                                "black" => Some(Color::Black),
                                _ => None,
                            };
                        }
                        ClientMsg::Move { mov } => {
                            if mov.len() == 4 {
                                let s = &mov[0..2];
                                let e = &mov[2..4];
                                game.make_move(s, e);
                            }
                        }
                        ClientMsg::Moves { moves } => {
                            game = Game::new();
                            for m in moves {
                                if m.mov.len() == 4 {
                                    let s = &m.mov[0..2];
                                    let e = &m.mov[2..4];
                                    game.make_move(s, e);
                                }
                            }
                        }
                    }
                } else if txt.len() == 4 {
                    let start = &txt[0..2];
                    let end = &txt[2..4];
                    game.make_move(start, end);
                }

                if let Some(color) = my_color {
                    if color == game.current_turn {
                        let start_time = Instant::now();
                        let next = if color == Color::White && game.history.is_empty() {
                            Some(("d2".to_string(), "d4".to_string()))
                        } else {
                            engine.best_move(&mut game)
                        };
                        println!("AI calculation took {:?}", start_time.elapsed());
                        if let Some((s, e)) = next {
                            game.make_move(&s, &e);
                            let _ = write.send(Message::Text(format!("{}{}", s, e))).await;
                        }
                    }
                }
            }
        }
    }
    println!("Client disconnected: {}", addr);
}
