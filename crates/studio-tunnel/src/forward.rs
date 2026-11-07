//! Copies bytes between local sockets and `direct-tcpip` channels.

use crate::known_hosts::KeyChecker;
use russh::client::Handle;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

/// How often to notice that the SSH session died.
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

async fn relay(
    handle: Arc<Handle<KeyChecker>>,
    mut socket: TcpStream,
    target_host: String,
    target_port: u16,
    shutdown: CancellationToken,
) {
    let origin = socket.peer_addr().ok();
    let origin_host = origin.map_or_else(|| "127.0.0.1".to_owned(), |a| a.ip().to_string());
    let origin_port = origin.map_or(0, |a| a.port());
    let channel = handle
        .channel_open_direct_tcpip(
            target_host,
            u32::from(target_port),
            origin_host,
            u32::from(origin_port),
        )
        .await;
    let channel = match channel {
        Ok(channel) => channel,
        Err(error) => {
            tracing::warn!(%error, "the bastion refused to open a forward");
            return;
        }
    };
    let mut stream = channel.into_stream();
    tokio::select! {
        _ = tokio::io::copy_bidirectional(&mut socket, &mut stream) => {}
        () = shutdown.cancelled() => {}
    }
}

/// Serves the listener until shut down or the SSH session ends.
pub(crate) async fn serve(
    listener: TcpListener,
    handle: Arc<Handle<KeyChecker>>,
    target_host: String,
    target_port: u16,
    shutdown: CancellationToken,
    lost: CancellationToken,
) {
    let mut watch = tokio::time::interval(WATCH_INTERVAL);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = watch.tick() => {
                if handle.is_closed() {
                    lost.cancel();
                    break;
                }
            }
            accepted = listener.accept() => {
                let Ok((socket, _)) = accepted else { continue };
                tokio::spawn(relay(
                    handle.clone(),
                    socket,
                    target_host.clone(),
                    target_port,
                    shutdown.clone(),
                ));
            }
        }
    }
    // Dropping the listener here frees the port.
    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "", "en")
        .await;
}
