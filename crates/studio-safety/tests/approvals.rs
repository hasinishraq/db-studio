//! Approval token rules.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;
use studio_safety::{ApprovalBook, SafetyError};
use studio_types::{ApprovalToken, Origin, SessionId};

const SQL: &str = "DELETE FROM users WHERE id = 5";

#[test]
fn a_token_works_once() {
    let book = ApprovalBook::new();
    let session = SessionId::new();
    let token = book.issue(session, SQL, Origin::Editor);
    assert_eq!(book.consume(&token, session, SQL), Ok(Origin::Editor));
    assert_eq!(book.consume(&token, session, SQL), Err(SafetyError::NoApproval));
}

#[test]
fn an_expired_token_fails() {
    let book = ApprovalBook::with_ttl(Duration::from_millis(1));
    let session = SessionId::new();
    let token = book.issue(session, SQL, Origin::Editor);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(book.consume(&token, session, SQL), Err(SafetyError::Expired));
}

#[test]
fn one_changed_character_fails() {
    let book = ApprovalBook::new();
    let session = SessionId::new();
    let token = book.issue(session, SQL, Origin::Editor);
    let other = "DELETE FROM users WHERE id = 6";
    assert_eq!(book.consume(&token, session, other), Err(SafetyError::Mismatch));
}

#[test]
fn another_session_fails() {
    let book = ApprovalBook::new();
    let token = book.issue(SessionId::new(), SQL, Origin::Editor);
    assert_eq!(
        book.consume(&token, SessionId::new(), SQL),
        Err(SafetyError::Mismatch)
    );
}

#[test]
fn a_mismatch_still_burns_the_token() {
    let book = ApprovalBook::new();
    let session = SessionId::new();
    let token = book.issue(session, SQL, Origin::Editor);
    assert_eq!(
        book.consume(&token, session, "SELECT 1"),
        Err(SafetyError::Mismatch)
    );
    assert_eq!(book.consume(&token, session, SQL), Err(SafetyError::NoApproval));
}

#[test]
fn an_expired_token_is_also_burned() {
    let book = ApprovalBook::with_ttl(Duration::from_millis(1));
    let session = SessionId::new();
    let token = book.issue(session, SQL, Origin::Editor);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(book.consume(&token, session, SQL), Err(SafetyError::Expired));
    assert_eq!(book.consume(&token, session, SQL), Err(SafetyError::NoApproval));
}

#[test]
fn an_unknown_token_fails() {
    let book = ApprovalBook::new();
    let forged = ApprovalToken("00".repeat(32));
    assert_eq!(
        book.consume(&forged, SessionId::new(), SQL),
        Err(SafetyError::NoApproval)
    );
}

#[test]
fn the_origin_is_reported() {
    let book = ApprovalBook::new();
    let session = SessionId::new();
    let token = book.issue(session, SQL, Origin::Ai);
    assert_eq!(book.consume(&token, session, SQL), Ok(Origin::Ai));
}

#[test]
fn tokens_are_long_and_unique() {
    let book = ApprovalBook::new();
    let session = SessionId::new();
    let a = book.issue(session, SQL, Origin::Editor);
    let b = book.issue(session, SQL, Origin::Editor);
    assert_eq!(a.0.len(), 64);
    assert_ne!(a, b);
}

#[test]
fn issuing_sweeps_expired_entries() {
    let book = ApprovalBook::with_ttl(Duration::from_millis(1));
    let session = SessionId::new();
    for _ in 0..5 {
        book.issue(session, SQL, Origin::Editor);
    }
    std::thread::sleep(Duration::from_millis(20));
    book.issue(session, SQL, Origin::Editor);
    assert_eq!(book.outstanding(), 1);
}
