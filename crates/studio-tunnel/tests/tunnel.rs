//! Real SSH handshakes against an in-process server, so no container is needed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rand::rngs::OsRng;
use rand::RngCore;
use russh::keys::ssh_key::{self, LineEnding};
use russh::keys::{HashAlg, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Handler, Msg, Server as _, Session, VerifiedPublicKey};
use russh::Channel;
use secrecy::SecretString;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use studio_tunnel::{HostKeyStore, PinnedKey, Tunnel, TunnelAuth, TunnelConfig, TunnelError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone)]
struct Bastion {
    client_key: Option<ssh_key::PublicKey>,
    /// Whether any credential reached the server.
    auth_attempts: Arc<Mutex<u32>>,
}

impl russh::server::Server for Bastion {
    type Handler = Self;
    fn new_client(&mut self, _: Option<SocketAddr>) -> Self {
        self.clone()
    }
}

impl Handler for Bastion {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        *self.auth_attempts.lock().unwrap() += 1;
        if user == "tunnel" && password == "tunnel" {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_publickey(&mut self, _user: &str, key: VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
        *self.auth_attempts.lock().unwrap() += 1;
        if self.client_key.as_ref() == Some(key.public_key) {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _origin: &str,
        _origin_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        let target = format!("{host}:{port}");
        tokio::spawn(async move {
            let mut upstream = TcpStream::connect(target).await.unwrap();
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
        });
        Ok(())
    }
}

struct Pins(Vec<PinnedKey>);

#[async_trait::async_trait]
impl HostKeyStore for Pins {
    async fn pinned(&self, _host: &str, _port: u16) -> Result<Vec<PinnedKey>, TunnelError> {
        Ok(self.0.clone())
    }
}

fn pin(key: &PrivateKey) -> PinnedKey {
    PinnedKey {
        key_type: key.algorithm().to_string(),
        fingerprint: key.public_key().fingerprint(HashAlg::Sha256).to_string(),
    }
}

fn new_key() -> PrivateKey {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let pair = ssh_key::private::Ed25519Keypair::from_seed(&seed);
    PrivateKey::from(pair)
}

struct Fixture {
    port: u16,
    host_key: PrivateKey,
    client_key: PrivateKey,
    target_port: u16,
    attempts: Arc<Mutex<u32>>,
}

/// An echo service standing in for a database.
async fn echo() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buffer = [0u8; 1024];
                while let Ok(read) = socket.read(&mut buffer).await {
                    if read == 0 || socket.write_all(&buffer[..read]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

async fn fixture() -> Fixture {
    let (host_key, client_key) = (new_key(), new_key());
    let attempts = Arc::new(Mutex::new(0));
    let mut server = Bastion {
        client_key: Some(client_key.public_key().clone()),
        auth_attempts: attempts.clone(),
    };
    let config = Arc::new(russh::server::Config {
        keys: vec![host_key.clone()],
        auth_rejection_time: Duration::from_millis(1),
        ..Default::default()
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = server.run_on_socket(config, &listener).await;
    });
    Fixture {
        port,
        host_key,
        client_key,
        target_port: echo().await,
        attempts,
    }
}

impl Fixture {
    fn config(&self, auth: TunnelAuth) -> TunnelConfig {
        TunnelConfig {
            host: "127.0.0.1".into(),
            port: self.port,
            username: "tunnel".into(),
            auth,
            target_host: "127.0.0.1".into(),
            target_port: self.target_port,
        }
    }

    fn password(&self) -> TunnelConfig {
        self.config(TunnelAuth::Password(SecretString::from("tunnel".to_owned())))
    }

    fn trusted(&self) -> Pins {
        Pins(vec![pin(&self.host_key)])
    }
}

async fn round_trip(port: u16) {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    socket.write_all(b"ping through ssh").await.unwrap();
    let mut buffer = [0u8; 16];
    socket.read_exact(&mut buffer).await.unwrap();
    assert_eq!(&buffer, b"ping through ssh");
}

#[tokio::test]
async fn password_auth_forwards_traffic() {
    let f = fixture().await;
    let tunnel = Tunnel::open(f.password(), &f.trusted()).await.unwrap();
    round_trip(tunnel.local_port).await;
    round_trip(tunnel.local_port).await;
}

#[tokio::test]
async fn key_auth_forwards_traffic() {
    let f = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id");
    std::fs::write(&path, f.client_key.to_openssh(LineEnding::LF).unwrap().as_bytes()).unwrap();
    let auth = TunnelAuth::Key {
        path: path.to_string_lossy().into_owned(),
        passphrase: None,
    };
    let tunnel = Tunnel::open(f.config(auth), &f.trusted()).await.unwrap();
    round_trip(tunnel.local_port).await;
}

#[tokio::test]
async fn an_unknown_host_key_stops_before_any_credential_is_sent() {
    let f = fixture().await;
    let error = Tunnel::open(f.password(), &Pins(vec![]))
        .await
        .err()
        .expect("must refuse");
    let TunnelError::HostKeyUnknown {
        fingerprint_sha256, ..
    } = error
    else {
        panic!("got {error:?}")
    };
    assert_eq!(fingerprint_sha256, pin(&f.host_key).fingerprint);
    assert_eq!(
        *f.attempts.lock().unwrap(),
        0,
        "the password must not be sent to an unverified server"
    );
}

#[tokio::test]
async fn a_changed_host_key_is_a_hard_error() {
    let f = fixture().await;
    let other = pin(&new_key());
    let error = Tunnel::open(f.password(), &Pins(vec![other.clone()]))
        .await
        .err()
        .expect("must refuse");
    let TunnelError::HostKeyChanged { expected, actual, .. } = error else {
        panic!("got {error:?}")
    };
    assert_eq!(expected, other.fingerprint);
    assert_eq!(actual, pin(&f.host_key).fingerprint);
    assert_eq!(*f.attempts.lock().unwrap(), 0);
}

#[tokio::test]
async fn a_wrong_password_is_an_auth_error() {
    let f = fixture().await;
    let auth = TunnelAuth::Password(SecretString::from("nope".to_owned()));
    let error = Tunnel::open(f.config(auth), &f.trusted())
        .await
        .err()
        .expect("must refuse");
    assert!(matches!(error, TunnelError::Auth(_)), "got {error:?}");
    assert!(
        !error.to_string().contains("nope"),
        "a password must never appear in an error"
    );
}

#[tokio::test]
async fn dropping_the_tunnel_frees_the_port() {
    let f = fixture().await;
    let tunnel = Tunnel::open(f.password(), &f.trusted()).await.unwrap();
    let port = tunnel.local_port;
    round_trip(port).await;
    drop(tunnel);
    for _ in 0..100 {
        if TcpListener::bind(("127.0.0.1", port)).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("port {port} was never released");
}

#[tokio::test]
async fn close_waits_for_the_port() {
    let f = fixture().await;
    let tunnel = Tunnel::open(f.password(), &f.trusted()).await.unwrap();
    let port = tunnel.local_port;
    tunnel.close().await;
    TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("the port must be free at once");
}
