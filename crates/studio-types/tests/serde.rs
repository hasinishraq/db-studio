//! Serde round trips and the JSON shapes the frontend relies on.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use studio_types::*;

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T) {
    let json = serde_json::to_string(&value).unwrap();
    let back: T = serde_json::from_str(&json).unwrap();
    assert_eq!(value, back, "round trip changed {json}");
}

fn profile() -> ConnectionProfile {
    ConnectionProfile {
        id: ProfileId::new(),
        name: "main".into(),
        engine: Engine::Postgres,
        host: "db".into(),
        port: 5432,
        database: "app".into(),
        username: "me".into(),
        tls_mode: TlsMode::default(),
        ca_cert_path: Some("/ca.pem".into()),
        client_cert_path: None,
        client_key_path: None,
        ssh: Some(SshConfig {
            host: "bastion".into(),
            port: 22,
            username: "me".into(),
            auth: SshAuth::PrivateKey { path: "/k".into() },
            has_secret: true,
        }),
        environment: Environment::Production,
        read_only: true,
        ai_policy: AiPolicy::LocalOnly,
        color: None,
        has_password: true,
    }
}

#[test]
fn profile_has_no_secret_keys() {
    let json = serde_json::to_string(&profile()).unwrap().to_lowercase();
    let without_flags = json.replace("haspassword", "").replace("hassecret", "");
    assert!(!without_flags.contains("password"), "{json}");
    assert!(!without_flags.contains("passphrase"), "{json}");
    assert!(!without_flags.contains("secret"), "{json}");
}

#[test]
fn enums_and_structs_round_trip() {
    round_trip(profile());
    for mode in [
        TlsMode::Disable,
        TlsMode::Require,
        TlsMode::VerifyCa,
        TlsMode::VerifyFull,
    ] {
        round_trip(mode);
    }
    for e in [Engine::Postgres, Engine::MySql, Engine::Sqlite] {
        round_trip(e);
    }
    for e in [
        Environment::Development,
        Environment::Staging,
        Environment::Production,
    ] {
        round_trip(e);
    }
    for p in [
        AiPolicy::Default,
        AiPolicy::LocalOnly,
        AiPolicy::NoRowSharing,
        AiPolicy::Disabled,
    ] {
        round_trip(p);
    }
    for a in [
        SshAuth::Password,
        SshAuth::Agent,
        SshAuth::PrivateKey { path: "k".into() },
    ] {
        round_trip(a);
    }
    round_trip(Settings::default());
    round_trip(SessionId::new());
    round_trip(StatementOutcome::Command {
        rows_affected: 2,
        elapsed_ms: 1,
    });
    round_trip(StatementOutcome::Error {
        error: AppError::invalid("bad"),
    });
    round_trip(RunEvent::Rows {
        index: 0,
        batch: RowBatch {
            seq: 1,
            rows: vec![vec![Value::Null, Value::Int(1)]],
        },
    });
    round_trip(FilterRule {
        column: "a".into(),
        operator: FilterOperator::IsNull,
        value: None,
    });
    round_trip(RowChange::Delete {
        keys: vec![vec![CellValue {
            column: "id".into(),
            value: Some("1".into()),
        }]],
    });
    round_trip(AiEvent::QueryStart {
        id: QueryId::new(),
        sql: "select 1".into(),
        read_only: true,
        reason: None,
        needs_review: false,
        risk_reasons: vec![],
    });
    round_trip(AiEvent::Done { error: None });
}

#[test]
fn ids_are_strings_and_reject_garbage() {
    let id = RunId::new();
    assert_eq!(
        serde_json::to_value(id).unwrap(),
        serde_json::json!(id.to_string())
    );
    assert!(serde_json::from_str::<RunId>("\"not-a-uuid\"").is_err());
}

#[test]
fn missing_settings_fall_back_to_defaults() {
    let s: Settings = serde_json::from_str(r#"{"historyLimit": 5}"#).unwrap();
    assert_eq!(s.history_limit, 5);
    assert!(!s.ai_share_rows, "row sharing must default to off");
    assert!(!Settings::default().encrypt_store);
}

#[test]
fn tls_defaults_to_verify_full() {
    assert_eq!(TlsMode::default(), TlsMode::VerifyFull);
}

#[test]
fn approval_token_debug_is_redacted() {
    assert!(!format!("{:?}", ApprovalToken("abc123".into())).contains("abc123"));
}

#[test]
fn bytes_keep_only_a_preview() {
    let Value::Bytes { len, preview_hex } = Value::bytes(&[0xab; 100]) else {
        panic!("not bytes")
    };
    assert_eq!(len, 100);
    assert_eq!(preview_hex.len(), 128);
    assert!(preview_hex.starts_with("abab"));
}

#[test]
fn value_json_shape() {
    let values = vec![
        Value::Null,
        Value::Bool(true),
        Value::Int(7),
        Value::BigInt("9007199254740993".into()),
        Value::Float(1.5),
        Value::Decimal("1.10".into()),
        Value::Text("hi".into()),
        Value::Json("{}".into()),
        Value::bytes(&[1, 255]),
        Value::Temporal("2026-11-07".into()),
        Value::Unsupported("tsvector".into()),
    ];
    insta::assert_json_snapshot!(values, @r#"
    [
      {
        "t": "Null"
      },
      {
        "t": "Bool",
        "v": true
      },
      {
        "t": "Int",
        "v": 7
      },
      {
        "t": "BigInt",
        "v": "9007199254740993"
      },
      {
        "t": "Float",
        "v": 1.5
      },
      {
        "t": "Decimal",
        "v": "1.10"
      },
      {
        "t": "Text",
        "v": "hi"
      },
      {
        "t": "Json",
        "v": "{}"
      },
      {
        "t": "Bytes",
        "v": {
          "len": 2,
          "preview_hex": "01ff"
        }
      },
      {
        "t": "Temporal",
        "v": "2026-11-07"
      },
      {
        "t": "Unsupported",
        "v": "tsvector"
      }
    ]
    "#);
}
