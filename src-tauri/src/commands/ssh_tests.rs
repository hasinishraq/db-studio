//! A real database through a real SSH handshake: the connector, pin store and session manager together.
//! The bastion runs in-process, so only the PostgreSQL test servers are needed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::setup::drivers;
use rand::rngs::OsRng;
use rand::RngCore;
use russh::keys::ssh_key;
use russh::keys::PrivateKey;
use russh::server::{Auth, ChannelOpenHandle, Handler, Msg, Server as _, Session};
use russh::Channel;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use studio_safety::ApprovalBook;
use studio_session::testing::profile;
use studio_session::{NullAuditSink, RunOptions};
use studio_store::MemorySecretStore;
use studio_types::{RunEvent, RunId, SshConfig, StatementOutcome, TlsMode, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

/// `route` plays the bastion's own DNS: every forward goes there, whatever name was asked for.
#[derive(Clone)]
struct Bastion {
    route: Option<String>,
}

impl russh::server::Server for Bastion {
    type Handler = Self;
    fn new_client(&mut self, _: Option<SocketAddr>) -> Self {
        self.clone()
    }
}

impl Handler for Bastion {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if password == "tunnel" {
            Auth::Accept
        } else {
            Auth::reject()
        })
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
        let target = self.route.clone().unwrap_or_else(|| format!("{host}:{port}"));
        tokio::spawn(async move {
            let mut upstream = TcpStream::connect(target).await.unwrap();
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
        });
        Ok(())
    }
}

async fn bastion(route: Option<String>) -> u16 {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let host_key = PrivateKey::from(ssh_key::private::Ed25519Keypair::from_seed(&seed));
    let config = Arc::new(russh::server::Config {
        keys: vec![host_key],
        auth_rejection_time: Duration::from_millis(1),
        ..Default::default()
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = Bastion { route }.run_on_socket(config, &listener).await;
    });
    port
}

/// Parses `postgres://user:pass@host:port/db` into a profile that reaches it through a bastion.
fn tunnelled(url: &str, bastion_port: u16) -> (ConnectionProfile, SecretString) {
    let rest = url.split_once("://").unwrap().1;
    let (credentials, location) = rest.split_once('@').unwrap();
    let (user, password) = credentials.split_once(':').unwrap();
    let (address, database) = location.split_once('/').unwrap();
    let (host, port) = address.split_once(':').unwrap();
    let config = ConnectionProfile {
        host: host.into(),
        port: port.parse().unwrap(),
        database: database.into(),
        username: user.into(),
        tls_mode: TlsMode::Disable,
        ssh: Some(SshConfig {
            host: "127.0.0.1".into(),
            port: bastion_port,
            username: "tunnel".into(),
            auth: SshAuth::Password,
            has_secret: true,
        }),
        ..profile()
    };
    (config, SecretString::from(password.to_owned()))
}

async fn fixture() -> (tempfile::TempDir, Store, Arc<SessionManager>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), Arc::new(MemorySecretStore::new()))
        .await
        .unwrap();
    let connector = Arc::new(TunnelConnector {
        store: store.clone(),
        app: None,
        sessions: OnceLock::new(),
    });
    let manager = SessionManager::new(drivers(), Arc::new(ApprovalBook::new()), Arc::new(NullAuditSink))
        .with_connector(connector);
    (dir, store, Arc::new(manager))
}

fn ssh_secret() -> Option<SecretString> {
    Some(SecretString::from("tunnel".to_owned()))
}

async fn first_cell(manager: &SessionManager, session: SessionId, sql: &str) -> Value {
    let (sink, mut inbox) = mpsc::channel(16);
    let outcomes = tokio::spawn({
        let sql = sql.to_owned();
        async move {
            let mut cell = None;
            while let Some(event) = inbox.recv().await {
                if let RunEvent::Rows { batch, .. } = event {
                    cell = batch
                        .rows
                        .into_iter()
                        .next()
                        .and_then(|row| row.into_iter().next());
                }
            }
            (cell, sql)
        }
    });
    let result = manager
        .run_user(session, sql, None, RunId::new(), RunOptions::default(), sink)
        .await;
    for outcome in result.unwrap() {
        assert!(!matches!(outcome, StatementOutcome::Error { .. }), "{outcome:?}");
    }
    outcomes.await.unwrap().0.expect("a row")
}

/// Connects, which must first stop at the unknown host key, then trusts it as the UI would.
async fn connect_after_trust(
    store: &Store,
    manager: &SessionManager,
    config: ConnectionProfile,
    password: SecretString,
) -> SessionId {
    store.upsert_profile(&config).await.unwrap();
    let first = manager
        .connect(config.clone(), Some(password.clone()), ssh_secret())
        .await;
    let Err(SessionError::HostKey {
        changed: false,
        detail,
        ..
    }) = first
    else {
        panic!("an unknown key must stop the connection");
    };
    trust(
        store,
        config.id,
        detail.key_type.clone().unwrap(),
        detail.fingerprint.clone(),
        false,
    )
    .await
    .unwrap();
    manager
        .connect(config, Some(password), ssh_secret())
        .await
        .unwrap()
        .session_id
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_database_answers_through_the_tunnel() {
    let Ok(url) = std::env::var("TEST_PG_URL") else {
        println!("skipped: TEST_PG_URL not set");
        return;
    };
    let (_dir, store, manager) = fixture().await;
    let (config, password) = tunnelled(&url, bastion(None).await);
    let session = connect_after_trust(&store, &manager, config, password).await;
    let value = first_cell(&manager, session, "SELECT 40 + 2").await;
    assert_eq!(value, Value::Int(42));
    manager.disconnect(session, false).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn tls_is_verified_end_to_end_through_the_tunnel() {
    let Ok(url) = std::env::var("TEST_PG_TLS_URL") else {
        println!("skipped: TEST_PG_TLS_URL not set");
        return;
    };
    let ca = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docker/tls/ca.crt");
    let (_dir, store, manager) = fixture().await;
    let (mut config, password) = tunnelled(&url, bastion(None).await);
    config.tls_mode = TlsMode::VerifyFull;
    config.ca_cert_path = Some(ca.to_string_lossy().into_owned());
    let session = connect_after_trust(&store, &manager, config, password).await;
    let sql = "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()";
    assert_eq!(first_cell(&manager, session, sql).await, Value::Bool(true));
    let info = manager.get(session).unwrap().session.server_info().await.unwrap();
    assert!(
        info.tls_verified,
        "the certificate was checked against the real host name"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_name_the_certificate_does_not_cover_fails_through_the_tunnel() {
    let Ok(url) = std::env::var("TEST_PG_TLS_URL") else {
        println!("skipped: TEST_PG_TLS_URL not set");
        return;
    };
    let ca = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docker/tls/ca.crt");
    let (_dir, store, manager) = fixture().await;
    let real = url
        .rsplit('@')
        .next()
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .replace("localhost", "127.0.0.1");
    let (mut config, password) = tunnelled(&url, bastion(Some(real)).await);
    // The bastion reaches the server, but the certificate only covers localhost.
    config.host = "db.internal.example".into();
    config.tls_mode = TlsMode::VerifyFull;
    config.ca_cert_path = Some(ca.to_string_lossy().into_owned());
    store.upsert_profile(&config).await.unwrap();
    let first = manager
        .connect(config.clone(), Some(password.clone()), ssh_secret())
        .await;
    let Err(SessionError::HostKey { detail, .. }) = first else {
        panic!("expected the host key stop")
    };
    trust(
        &store,
        config.id,
        detail.key_type.clone().unwrap(),
        detail.fingerprint.clone(),
        false,
    )
    .await
    .unwrap();
    let second = manager.connect(config, Some(password), ssh_secret()).await;
    assert!(
        matches!(second, Err(SessionError::Db(studio_db::DbError::Tls(_)))),
        "the certificate does not cover this name: {:?}",
        second.err()
    );
}
