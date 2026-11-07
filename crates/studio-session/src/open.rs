//! Opening and closing sessions.

use crate::manager::{SessionError, SessionManager};
use crate::policy::SessionPolicy;
use secrecy::SecretString;
use std::any::Any;
use std::sync::Arc;
use studio_db::{ConnectConfig, Session, TlsConfig};
use studio_types::{ConnectionProfile, Engine, ServerInfo, SessionId, SessionInfo, TxState};
use tokio::sync::Mutex;

/// An established route to a database, such as an SSH tunnel.
pub struct Route {
    /// Local port to dial.
    pub local_port: u16,
    /// Dropped when the session closes, which closes the route.
    pub keep_alive: Box<dyn Any + Send + Sync>,
}

/// Opens the route for a profile with an SSH bastion. Implemented by the app.
#[async_trait::async_trait]
pub trait Connector: Send + Sync {
    /// Opens a route to the profile's database through its bastion.
    async fn open_route(
        &self,
        profile: &ConnectionProfile,
        ssh_secret: Option<SecretString>,
        session: Option<SessionId>,
    ) -> Result<Route, SessionError>;
}

/// One open session.
pub struct OpenSession {
    /// Its id.
    pub id: SessionId,
    /// The profile it came from. Holds no secrets.
    pub profile: ConnectionProfile,
    /// The driver session.
    pub session: Box<dyn Session>,
    /// What the profile allows.
    pub policy: SessionPolicy,
    pub(crate) tx: Mutex<TxState>,
    /// Keeps a tunnel alive for as long as the session.
    _route: Option<Route>,
}

impl OpenSession {
    /// The engine, which is also the SQL dialect.
    pub fn engine(&self) -> Engine {
        self.profile.engine
    }
}

fn config(
    profile: &ConnectionProfile,
    password: Option<SecretString>,
    route: Option<&Route>,
) -> ConnectConfig {
    let (host, port) = match route {
        Some(route) => ("127.0.0.1".to_owned(), route.local_port),
        None => (profile.host.clone(), profile.port),
    };
    let mut cfg = ConnectConfig::new(host, port, profile.database.clone());
    cfg.username = profile.username.clone();
    cfg.password = password;
    cfg.read_only = profile.read_only;
    cfg.tls = TlsConfig {
        mode: profile.tls_mode,
        ca_path: profile.ca_cert_path.clone(),
        client_cert_path: profile.client_cert_path.clone(),
        client_key_path: profile.client_key_path.clone(),
        // TLS runs end to end through a tunnel, so the real host name is what gets verified.
        server_name: route.map(|_| profile.host.clone()),
    };
    cfg
}

impl SessionManager {
    async fn open(
        &self,
        profile: &ConnectionProfile,
        password: Option<SecretString>,
        ssh_secret: Option<SecretString>,
        id: SessionId,
    ) -> Result<(Box<dyn Session>, Option<Route>), SessionError> {
        let driver = self.drivers.get(&profile.engine).ok_or(SessionError::NoDriver)?;
        let route = match (&profile.ssh, &self.connector) {
            (Some(_), Some(connector)) => Some(connector.open_route(profile, ssh_secret, Some(id)).await?),
            (Some(_), None) => {
                return Err(SessionError::Tunnel("SSH tunnels are not available".to_owned()));
            }
            (None, _) => None,
        };
        let session = driver.connect(&config(profile, password, route.as_ref())).await?;
        Ok((session, route))
    }

    /// Connects and registers a session.
    pub async fn connect(
        &self,
        profile: ConnectionProfile,
        password: Option<SecretString>,
        ssh_secret: Option<SecretString>,
    ) -> Result<SessionInfo, SessionError> {
        let id = SessionId::new();
        let (session, route) = self.open(&profile, password, ssh_secret, id).await?;
        let server = session.server_info().await?;
        let info = SessionInfo {
            session_id: id,
            profile_id: profile.id,
            engine: profile.engine,
            server,
        };
        let policy = SessionPolicy::of(&profile);
        let open = OpenSession {
            id,
            profile,
            session,
            policy,
            tx: Mutex::new(TxState::default()),
            _route: route,
        };
        self.sessions.insert(id, Arc::new(open));
        Ok(info)
    }

    /// Connects, reports the server and disconnects.
    pub async fn test_connection(
        &self,
        profile: &ConnectionProfile,
        password: Option<SecretString>,
        ssh_secret: Option<SecretString>,
    ) -> Result<ServerInfo, SessionError> {
        let (session, route) = self.open(profile, password, ssh_secret, SessionId::new()).await?;
        let info = session.server_info().await;
        session.close().await;
        drop(route);
        Ok(info?)
    }

    /// Closes a session. With uncommitted work this fails unless `force`, which rolls it back.
    pub async fn disconnect(&self, id: SessionId, force: bool) -> Result<(), SessionError> {
        let open = self.get(id)?;
        let tx = *open.tx.lock().await;
        if tx.dirty && !force {
            return Err(SessionError::DirtyTransaction);
        }
        if tx.mode == studio_types::TxMode::Manual {
            let _ = open.session.rollback().await;
        }
        self.sessions.remove(&id);
        open.session.close().await;
        Ok(())
    }

    /// Marks a session lost after its tunnel dropped.
    pub fn forget(&self, id: SessionId) {
        self.sessions.remove(&id);
    }
}
