use std::env;
use chessmind::{game::Game, engine::Engine};
use futures_util::{StreamExt, SinkExt};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[derive(Deserialize)]
struct MoveMsg {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(rename = "move")]
    mov: Option<String>,
    color: Option<String>,
}

#[tokio::main]
async fn main() {
    let port = env::args().nth(1).unwrap_or_else(|| "8771".into());
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await.expect("bind");
    println!("WebSocket server on ws://{}", addr);
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_conn(stream));
    }
}

async fn handle_conn(stream: tokio::net::TcpStream) {
    let ws_stream = accept_async(stream).await.expect("ws accept");
    let (mut write, mut read) = ws_stream.split();
    let mut game = Game::new();
    let engine = Engine::new(3);
    while let Some(msg) = read.next().await {
        if let Ok(msg) = msg {
            if msg.is_text() {
                let txt = msg.to_text().unwrap();
                if txt.len() == 4 {
                    let start = &txt[0..2];
                    let end = &txt[2..4];
                    game.make_move(start, end);
                } else if let Ok(data) = serde_json::from_str::<MoveMsg>(txt) {
                    if data.msg_type == "move" {
                        if let Some(m) = data.mov {
                            if m.len() == 4 {
                                let s = &m[0..2];
                                let e = &m[2..4];
                                game.make_move(s, e);
                            }
                        }
                    }
                }
                if let Some((s, e)) = engine.best_move(&mut game) {
                    game.make_move(&s, &e);
                    let _ = write.send(Message::Text(format!("{}{}", s, e))).await;
                }
            }
        }
    }
}
