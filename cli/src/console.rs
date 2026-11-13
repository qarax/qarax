use std::mem::DropGuard;

use anyhow::Context;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

/// Convert an HTTP(S) base URL to a WebSocket URL and append the console
/// attach path for the given VM.
fn build_ws_url(base_url: &str, vm_id: Uuid) -> String {
    let ws_base = if base_url.starts_with("https://") {
        base_url.replacen("https://", "wss://", 1)
    } else {
        base_url.replacen("http://", "ws://", 1)
    };
    format!(
        "{}/vms/{}/console/attach",
        ws_base.trim_end_matches('/'),
        vm_id
    )
}

/// Attach an interactive WebSocket console to a running VM.
///
/// - Stdin bytes are forwarded to the server as `Binary` WebSocket frames.
/// - Output from the server is written directly to stdout.
/// - Press **Ctrl+]** (byte `0x1D`) to disconnect.
pub async fn attach(base_url: &str, vm_id: Uuid) -> anyhow::Result<()> {
    let ws_url = build_ws_url(base_url, vm_id);

    eprintln!("[Connecting to console for VM {vm_id} ...]");
    eprintln!("[Press Ctrl+] to disconnect]");

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .with_context(|| format!("Failed to connect to WebSocket at {ws_url}"))?;

    eprintln!("[Connected]");

    let (mut ws_write, mut ws_read) = ws_stream.split();

    // Enter terminal raw mode for the duration of the session.
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let _raw = DropGuard::new((), |()| {
        let _ = disable_raw_mode();
    });

    // stdin → WebSocket
    let stdin_to_ws = async move {
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 1024];
        loop {
            match stdin.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    // Ctrl+] (0x1D) is the escape sequence to detach.
                    if buf[..n].contains(&0x1D) {
                        break;
                    }
                    if ws_write
                        .send(Message::Binary(buf[..n].to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    };

    // WebSocket → stdout
    let ws_to_stdout = async move {
        let mut stdout = tokio::io::stdout();
        loop {
            match ws_read.next().await {
                Some(Ok(Message::Binary(data))) => {
                    let _ = stdout.write_all(&data).await;
                    let _ = stdout.flush().await;
                }
                Some(Ok(Message::Text(text))) => {
                    let _ = stdout.write_all(text.as_bytes()).await;
                    let _ = stdout.flush().await;
                }
                None | Some(Ok(Message::Close(_))) | Some(Err(_)) => break,
                _ => {}
            }
        }
    };

    // Run both halves concurrently; stop when either finishes.
    tokio::select! {
        _ = stdin_to_ws => {},
        _ = ws_to_stdout => {},
    }

    // _raw dropped here → raw mode disabled.
    eprintln!("\r\n[Disconnected]");
    Ok(())
}
