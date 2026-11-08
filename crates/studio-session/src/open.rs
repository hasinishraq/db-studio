//! Opening and closing sessions.

use crate::manager::{SessionError, SessionManager};
use crate::policy::SessionPolicy;
use secrecy::SecretString;
use std::any::Any;
use std::sync::Arc;
use studio_db::{ConnectConfig, Session, TlsConfig};
use studio_types::{ConnectionProfile, Engine, ServerInfo, SessionId, SessionInfo, TxMode, TxState};
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
    pub(crate) database: Mutex<Option<String>>,
    pub(crate) schema: Mutex<Option<String>>,
    /// Keeps a tunnel alive for as long as the session.
    _route: Option<Route>,
}

impl OpenSession {
    /// The engine, which is also the SQL dialect.
    pub fn engine(&self) -> Engine {
        self.profile.engine
    }
}

/// The database a new session starts in.
/// Applies the profile's remembered schema, when it still exists. A failure leaves the server's
/// own default in place; it never stops the connection.
async fn remembered_schema(session: &dyn Session, profile: &ConnectionProfile) -> Option<String> {
    let name = profile.default_schema.as_deref()?;
    let exists = session.list_schemas().await.ok()?.iter().any(|s| s.name == name);
    if !exists {
        tracing::warn!("the remembered schema no longer exists; using the server default");
        return None;
    }
    match session.use_schema(name).await {
        Ok(()) => Some(name.to_owned()),
        Err(error) => {
            tracing::warn!(%error, "could not use the remembered schema");
            None
        }
    }
}

fn starting_database(profile: &ConnectionProfile) -> Option<String> {
    let named = (!profile.database.is_empty()).then(|| profile.database.clone());
    match profile.engine {
        Engine::Sqlite => None,
        Engine::MySql => named,
        Engine::Postgres => named.or_else(|| Some("postgres".to_owned())),
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
        let database = starting_database(&profile);
        let schema = match profile.engine {
            Engine::Postgres => remembered_schema(session.as_ref(), &profile).await.or(session
                .current_schema()
                .await
                .ok()
                .flatten()),
            _ => None,
        };
        let info = SessionInfo {
            session_id: id,
            profile_id: profile.id,
            engine: profile.engine,
            server,
            database: database.clone(),
            schema: schema.clone(),
        };
        let policy = SessionPolicy::of(&profile);
        let open = OpenSession {
            id,
            profile,
            session,
            policy,
            tx: Mutex::new(TxState::default()),
            database: Mutex::new(database),
            schema: Mutex::new(schema),
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

    /// Lists the databases a session can choose from.
    pub async fn list_databases(&self, id: SessionId) -> Result<Vec<String>, SessionError> {
        Ok(self.get(id)?.session.list_databases().await?)
    }

    /// The session's chosen database.
    pub async fn current_database(&self, id: SessionId) -> Option<String> {
        self.get(id).ok()?.database.lock().await.clone()
    }

    /// Chooses the default database. MySQL changes it in place. A PostgreSQL connection is tied to
    /// one database, so that opens a new session first and closes this one only once it works.
    pub async fn use_database(
        &self,
        id: SessionId,
        name: &str,
        password: Option<SecretString>,
        ssh_secret: Option<SecretString>,
    ) -> Result<SessionInfo, SessionError> {
        let open = self.get(id)?;
        let engine = open.engine();
        if engine == Engine::Sqlite {
            return Err(SessionError::Refused(
                "a SQLite connection has one database".to_owned(),
            ));
        }
        let tx = *open.tx.lock().await;
        if tx.dirty || tx.mode == TxMode::Manual {
            return Err(SessionError::DirtyTransaction);
        }
        if !open
            .session
            .list_databases()
            .await?
            .iter()
            .any(|known| known == name)
        {
            return Err(SessionError::Refused(format!(
                "there is no database named {name}"
            )));
        }
        if engine == Engine::Postgres {
            if self.runs.iter().any(|run| run.is_on(id)) {
                return Err(SessionError::Refused(
                    "stop the running query before changing database".to_owned(),
                ));
            }
            let mut profile = open.profile.clone();
            // A remembered schema belongs to the remembered database only.
            if profile.database != name {
                profile.default_schema = None;
            }
            profile.database = name.to_owned();
            let info = self.connect(profile, password, ssh_secret).await?;
            self.disconnect(id, true).await?;
            return Ok(info);
        }
        open.session.use_database(name).await?;
        *open.database.lock().await = Some(name.to_owned());
        Ok(SessionInfo {
            session_id: id,
            profile_id: open.profile.id,
            engine,
            server: open.session.server_info().await?,
            database: Some(name.to_owned()),
            schema: None,
        })
    }

    /// The schema a PostgreSQL session resolves unqualified names in first.
    pub async fn current_schema(&self, id: SessionId) -> Option<String> {
        self.get(id).ok()?.schema.lock().await.clone()
    }

    /// Where unqualified names go: the schema on PostgreSQL, the database on MySQL.
    pub async fn default_namespace(&self, id: SessionId) -> Option<String> {
        let open = self.get(id).ok()?;
        match open.engine() {
            Engine::Postgres => open.schema.lock().await.clone(),
            _ => open.database.lock().await.clone(),
        }
    }

    /// Chooses the schema unqualified names resolve in first. PostgreSQL only.
    pub async fn use_schema(&self, id: SessionId, name: &str) -> Result<SessionInfo, SessionError> {
        let open = self.get(id)?;
        if open.engine() != Engine::Postgres {
            return Err(SessionError::Refused(
                "only PostgreSQL has schemas inside a database".to_owned(),
            ));
        }
        let tx = *open.tx.lock().await;
        if tx.dirty || tx.mode == TxMode::Manual {
            return Err(SessionError::DirtyTransaction);
        }
        if !open.session.list_schemas().await?.iter().any(|s| s.name == name) {
            return Err(SessionError::Refused(format!("there is no schema named {name}")));
        }
        open.session.use_schema(name).await?;
        *open.schema.lock().await = Some(name.to_owned());
        let database = open.database.lock().await.clone();
        Ok(SessionInfo {
            session_id: id,
            profile_id: open.profile.id,
            engine: open.engine(),
            server: open.session.server_info().await?,
            database,
            schema: Some(name.to_owned()),
        })
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
