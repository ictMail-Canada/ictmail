// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! DAV (CalDAV/CardDAV) on an IMAP account: `account.add`/`account.edit` take `dav` — an object
//! of server settings, `null` to turn it off, or nothing to keep what it has — and
//! `account.list` says where it starts from.

use mailcraft_model::{AccountKind, DavServer, Id};
use serde_json::{Map, Value, json};

use crate::Session;

fn add(s: &mut Session, params: Value) -> Id {
    s.run("account.add", &params).unwrap()["id"].as_u64().unwrap()
}

/// An IMAP account's params with `dav` (or `extra`) set on them.
fn params(dav: Value, extra: &[(&str, Value)]) -> Value {
    let mut v = Map::from_iter([
        ("email".to_string(), json!("sam@example.org")),
        ("incoming".to_string(), json!({"host": "mail.example.org", "port": 993, "security": "tls"})),
    ]);
    v.insert("dav".to_string(), dav);
    for (key, value) in extra {
        v.insert(key.to_string(), value.clone());
    }
    Value::Object(v)
}

fn dav_of(s: &Session, id: Id) -> Option<DavServer> {
    s.mailbox.account(id).unwrap().dav.clone()
}

/// `dav` on an IMAP account: an empty object turns it on with the address the incoming server
/// suggests; fields set it; `account.list` says the origin without the rest of the address.
#[test]
fn dav_turns_on_with_the_suggested_address() {
    let mut s = Session::empty();
    let a = add(&mut s, params(json!({}), &[]));
    let account = s.mailbox.account(a).unwrap();
    assert_eq!(account.kind, AccountKind::Imap);
    let dav = account.dav.clone().unwrap();
    // No user name of its own: the account's address is the user in the path, as SOGo has it.
    assert_eq!(dav.effective(account).as_deref(), Some("https://mail.example.org/SOGo/dav/sam@example.org/"));
    let list = s.run("account.list", &json!({})).unwrap();
    let shown = &list["accounts"].as_array().unwrap().iter().find(|x| x["id"] == json!(a)).unwrap()["dav"];
    assert_eq!(shown["server"], json!("https://mail.example.org"));
    assert_eq!(shown["username"], json!(""));
    assert_eq!(shown["trustsCertificate"], json!(false));
}

/// What is given is kept; what isn't keeps what the account had; `null` turns it off.
#[test]
fn dav_settings_are_kept_edited_and_cleared() {
    let mut s = Session::empty();
    let a = add(&mut s, params(json!({"url": "https://dav.example.org/", "username": "svc", "trusted": "a".repeat(64)}), &[]));
    let dav = dav_of(&s, a).unwrap();
    assert_eq!(dav.url, "https://dav.example.org/");
    assert_eq!(dav.username, "svc");
    assert_eq!(dav.trusted, "a".repeat(64));
    // An edit that says nothing of dav keeps it.
    s.run("account.edit", &json!({"id": a, "displayName": "Sam"})).unwrap();
    assert!(dav_of(&s, a).is_some());
    // One field changes, the rest stay.
    s.run("account.edit", &json!({"id": a, "dav": {"username": "other"}})).unwrap();
    let dav = dav_of(&s, a).unwrap();
    assert_eq!(dav.username, "other");
    assert_eq!(dav.url, "https://dav.example.org/");
    // null turns it off.
    s.run("account.edit", &json!({"id": a, "dav": null})).unwrap();
    assert!(dav_of(&s, a).is_none());
}

/// What the model refuses is refused whole: a bad address, a bad name, a bad fingerprint.
#[test]
fn a_bad_dav_setting_is_refused() {
    let mut s = Session::empty();
    for dav in [
        json!({"url": "http://dav.example.org/"}),
        json!({"url": "https://sam:pw@dav.example.org/"}),
        json!({"username": "a\nb"}),
        json!({"trusted": "not-hex"}),
        json!("dav.example.org"),
    ] {
        let before = s.mailbox.clone();
        assert!(s.run("account.add", &params(dav.clone(), &[])).is_err(), "{dav}");
        assert_eq!(s.mailbox, before);
    }
}

/// DAV belongs to an IMAP account: another kind has none, and saying so changes nothing of it.
#[test]
fn only_an_imap_account_has_dav() {
    let mut s = Session::empty();
    // A local account (no servers) can't have one.
    let a = add(&mut s, json!({"email": "local@example.org", "dav": {}}));
    assert_eq!(s.mailbox.account(a).unwrap().kind, AccountKind::Local);
    assert!(dav_of(&s, a).is_none());
    // An IMAP account that loses its servers is no longer one that can have DAV.
    let b = add(&mut s, params(json!({}), &[("email", json!("b@example.org"))]));
    s.run("account.edit", &json!({"id": b, "incoming": null, "outgoing": null})).unwrap();
    assert_eq!(s.mailbox.account(b).unwrap().kind, AccountKind::Local);
    assert!(dav_of(&s, b).is_none());
}
