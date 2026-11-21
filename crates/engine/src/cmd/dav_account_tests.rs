// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! DAV (CalDAV/CardDAV) on an IMAP account: `account.add`/`account.edit` take `dav` — an object
//! of server settings, `null` to turn it off, or nothing to keep what it has — and
//! `account.list` says where it starts from.

use mailcraft_model::{AccountKind, Id, dav::DavServer};
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

//! An account's calendar and contacts servers (#209): the switch, the settings, the refusals.

use mailcraft_model::{AccountKind, DavCollectionState, Id};
use serde_json::json;

use crate::agent::{Access, Class, classify};
use crate::{CmdError, Session};

const SYNC: [&str; 4] = ["sync.davRequest", "sync.applyDav", "sync.davChanges", "sync.applyDavUploaded"];

/// A session with an IMAP account and the feature switched on.
fn session() -> (Session, Id) {
    let mut s = Session::empty();
    s.dav_enabled = true;
    let id = s
        .run("account.add", &json!({"email": "ada@example.org", "incoming": {"host": "imap.example.org"}, "outgoing": {"host": "smtp.example.org"}}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    (s, id)
}

fn set(s: &mut Session, id: Id) {
    let said = s
        .run(
            "account.davSet",
            &json!({"id": id, "calendarHomes": ["https://dav.example.org/cal/ada/"], "contactHomes": ["https://cards.example.org/ada/"], "username": " ada "}),
        )
        .unwrap();
    assert_eq!((said["origins"].clone(), said["changedServer"].clone()), (json!(2), json!(false)));
}

#[test]
fn the_commands_are_there_and_switched_off_until_the_host_says() {
    let mut s = Session::empty();
    assert!(!s.dav_enabled, "it ships off");
    let id = s.run("account.add", &json!({"email": "ada@example.org", "incoming": {"host": "imap.example.org"}})).unwrap()["id"].as_u64().unwrap();
    for command in ["account.davSet", "sync.davRequest", "sync.applyDav", "sync.davChanges", "sync.applyDavUploaded"] {
        assert!(s.registry.get(command).is_some(), "{command}");
        assert!(!s.enabled(command), "{command}");
        let said = s.run(command, &json!({"id": id, "calendarHomes": ["https://dav.example.org/"]}));
        assert!(matches!(said, Err(CmdError::Disabled(_))), "{command}: {said:?}");
    }
    assert!(s.mailbox.account(id).unwrap().dav_server.is_none(), "nothing was kept");
    // Turning it off is always allowed: settings a file brought can be put away.
    assert!(s.enabled("account.davOff"));
    assert_eq!(s.run("account.davOff", &json!({"id": id})).unwrap()["off"], false);
    s.dav_enabled = true;
    assert!(s.enabled("account.davSet"));
}

#[test]
fn settings_are_kept_changed_and_put_away() {
    let (mut s, id) = session();
    set(&mut s, id);
    let dav = s.mailbox.account(id).unwrap().dav_server.clone().unwrap();
    assert_eq!((dav.calendar_homes.len(), dav.contact_homes.len(), dav.username.as_str()), (1, 1, "ada"));
    // What isn't named stays; the password flag is no other server.
    s.mailbox
        .account_mut(id)
        .unwrap()
        .dav_server
        .as_mut()
        .unwrap()
        .collections
        .push(DavCollectionState { href: "https://dav.example.org/cal/ada/w/".into(), ..Default::default() });
    let said = s.run("account.davSet", &json!({"id": id, "separatePassword": true})).unwrap();
    assert_eq!(said["changedServer"], false);
    let dav = s.mailbox.account(id).unwrap().dav_server.clone().unwrap();
    assert!(dav.separate_password && dav.collections.len() == 1 && dav.calendar_homes.len() == 1);
    // Another home is another server: what was known of the old one's collections is forgotten.
    let said = s.run("account.davSet", &json!({"id": id, "calendarHomes": ["https://dav2.example.org/cal/"]})).unwrap();
    assert_eq!(said["changedServer"], true);
    let dav = s.mailbox.account(id).unwrap().dav_server.clone().unwrap();
    assert!(dav.collections.is_empty() && dav.contact_homes.len() == 1);
    // Away it goes, and it was one.
    assert_eq!(s.run("account.davOff", &json!({"id": id})).unwrap()["off"], true);
    assert!(s.mailbox.account(id).unwrap().dav_server.is_none());
    assert_eq!(s.run("account.davOff", &json!({"id": id})).unwrap()["off"], false);
}

#[test]
fn what_cannot_be_used_is_refused_and_changes_nothing() {
    let (mut s, id) = session();
    set(&mut s, id);
    let kept = s.mailbox.account(id).unwrap().dav_server.clone();
    for bad in [
        json!({"id": id, "calendarHomes": ["http://dav.example.org/"]}),
        json!({"id": id, "calendarHomes": ["https://user:pw@dav.example.org/"]}),
        json!({"id": id, "calendarHomes": ["https://outlook.office365.com/dav/"]}),
        json!({"id": id, "contactHomes": ["https://www.googleapis.com/carddav/v1/"]}),
        json!({"id": id, "calendarHomes": [], "contactHomes": []}),
        json!({"id": id, "calendarHomes": (0..9).map(|n| format!("https://d.example.org/{n}/")).collect::<Vec<_>>()}),
        json!({"id": id, "calendarHomes": "https://dav.example.org/"}),
        json!({"id": id, "calendarHomes": [5]}),
        json!({"id": id, "username": "a\r\nb"}),
        json!({"id": id, "username": 5}),
        json!({"id": id, "trusted": "nope"}),
        json!({"id": id, "separatePassword": "yes"}),
        json!({"id": 9999}),
        json!({"id": "x"}),
        json!({}),
    ] {
        assert!(s.run("account.davSet", &bad).is_err(), "{bad}");
        assert_eq!(s.mailbox.account(id).unwrap().dav_server, kept, "{bad}");
    }
    // No address is said back in an error: it may name an organisation's servers.
    let said = s.run("account.davSet", &json!({"id": id, "calendarHomes": ["https://secret-host.example.org/x y"]})).unwrap_err().to_string();
    assert!(!said.contains("secret-host"), "{said}");
}

#[test]
fn only_an_account_with_a_server_of_its_own_may_have_one() {
    let (mut s, imap) = session();
    let local = s.run("account.add", &json!({"email": "grace@example.org", "kind": "local"})).unwrap()["id"].as_u64().unwrap();
    let graph = s.run("account.add", &json!({"email": "linus@example.org", "kind": "microsoft"})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(s.mailbox.account(graph).unwrap().kind, AccountKind::Microsoft);
    let shared = s.mailbox.account(imap).unwrap().clone();
    let shared = mailcraft_model::Account { id: 77, email: "team@example.org".into(), shared_of: Some(imap), ..shared };
    s.mailbox.accounts.push(shared);
    for id in [local, graph, 77] {
        let said = s.run("account.davSet", &json!({"id": id, "calendarHomes": ["https://dav.example.org/"]}));
        assert!(matches!(said, Err(CmdError::Failed(_))), "{id}: {said:?}");
        assert!(s.mailbox.account(id).unwrap().dav_server.is_none());
    }
    let jmap = s
        .run("account.add", &json!({"email": "jo@example.org", "kind": "jmap", "jmap": {"session": "https://jmap.example.org/session"}}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    assert!(s.run("account.davSet", &json!({"id": jmap, "calendarHomes": ["https://dav.example.org/"]})).is_ok());
}

#[test]
fn the_host_side_refuses_politely_until_it_is_built() {
    let (mut s, id) = session();
    // Switched on, but no account has a server yet: out of reach, with the reason.
    for command in ["sync.davRequest", "sync.davChanges"] {
        assert!(matches!(s.run(command, &json!({})), Err(CmdError::Disabled(_))), "{command}");
    }
    set(&mut s, id);
    for command in SYNC {
        assert!(s.enabled(command), "{command}");
        let said = s.run(command, &json!({"account": id, "result": {}}));
        assert!(matches!(said, Err(CmdError::Failed(_))), "{command}: {said:?}");
    }
    // Junk of every shape is refused and changes nothing.
    let kept = s.mailbox.account(id).unwrap().dav_server.clone();
    for junk in [json!(null), json!([]), json!("x"), json!({"account": -1}), json!({"result": {"deep": [[[]]]}}), json!({"account": 1e300})] {
        for command in SYNC.into_iter().chain(["account.davSet", "account.davOff"]) {
            let _ = s.run(command, &junk);
        }
    }
    assert_eq!(s.mailbox.account(id).unwrap().dav_server, kept);
}

#[test]
fn an_agent_may_not_choose_where_a_password_goes() {
    let (s, id) = session();
    for command in ["account.davSet", "account.davOff"] {
        assert_eq!(classify(&s, command, &json!({"id": id})), Access::Owner(Class::Accounts), "{command}");
    }
    for command in SYNC {
        assert_eq!(classify(&s, command, &json!({})), Access::Never, "{command}");
    }
}

#[test]
fn the_settings_survive_the_mailbox_file() {
    let (mut s, id) = session();
    set(&mut s, id);
    s.run("account.davSet", &json!({"id": id, "trusted": "AB".repeat(32), "separatePassword": true})).unwrap();
    s.mailbox.account_mut(id).unwrap().dav_server.as_mut().unwrap().collections.push(DavCollectionState {
        href: "https://dav.example.org/cal/ada/w/".into(),
        sync_token: "T-1".into(),
        ..Default::default()
    });
    let text = serde_json::to_string(&s.mailbox).unwrap();
    assert!(!text.contains("password\":\""), "no password is among it");
    let back: mailcraft_model::Mailbox = serde_json::from_str(&text).unwrap();
    assert_eq!(back.account(id).unwrap().dav_server, s.mailbox.account(id).unwrap().dav);
    assert_eq!(back.account(id).unwrap().dav_server.as_ref().unwrap().collections[0].sync_token, "T-1");
}

#[test]
fn an_account_that_only_lives_here_has_no_server_to_keep() {
    let (mut s, id) = session();
    set(&mut s, id);
    s.run("account.edit", &json!({"id": id, "incoming": null, "outgoing": null})).unwrap();
    let a = s.mailbox.account(id).unwrap();
    assert_eq!(a.kind, AccountKind::Local);
    assert!(a.dav_server.is_none());
}
