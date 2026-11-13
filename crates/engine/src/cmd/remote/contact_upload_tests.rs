// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Contacts of a DAV account's address books going back to its server: the changes `people`
//! notes, the upload `sync.contactChanges` builds, what `sync.applyContactChanges` makes of
//! the server's answer, and a download leaving a change that hasn't gone yet alone.

use mailcraft_model::remote::{ContactChangeKind, ContactOp, ContactOutcome, ContactResult, ContactsDownload, ContactsUploaded, RemoteContact};
use mailcraft_model::{AccountChoice, Contact, Id, Labeled, Origin};
use serde_json::json;

use super::{apply_contact_changes, contact_upload, merge_contacts};
use crate::Session;

/// An IMAP account with DAV on, in an empty mailbox.
fn session() -> (Session, Id) {
    let mut s = Session::empty();
    let id = s
        .run("account.add", &json!({"email": "sam@example.org", "incoming": {"host": "mail.example.org", "port": 993, "security": "tls"}, "dav": {}}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    (s, id)
}

/// Show `account`'s contacts in People, so a contact made is kept for it.
fn show(s: &mut Session, account: Id) {
    s.view.people_account = AccountChoice::Account(account);
}

/// A contact the server has, as a download brings it: its id is its resource's address.
fn downloaded(s: &mut Session, account: Id, remote: &str, etag: &str, first: &str) -> Id {
    let mut contact =
        Contact { first: first.into(), last: "Side".into(), emails: vec![Labeled::new("work", "side@example.org")], ..Default::default() };
    contact.origin = Origin { account, id: remote.into(), change_key: etag.into(), ..Default::default() };
    s.mailbox.add_contact(contact)
}

/// The server's answer to `op`, as a host brings it back.
fn answered(op: ContactOp, result: ContactResult, remote: &str, change_key: &str) -> ContactsUploaded {
    ContactsUploaded {
        outcomes: vec![ContactOutcome { op, result, remote: remote.into(), change_key: change_key.into(), ..Default::default() }],
        ..Default::default()
    }
}

/// A contact made while one account's contacts are shown is that server's from the start: the
/// upload carries the whole card, and the server's "done" names it and gives it an ETag.
#[test]
fn a_contact_made_for_a_dav_account_is_created_on_its_server() {
    let (mut s, account) = session();
    show(&mut s, account);
    let id = s.run("people.new", &json!({"first": "Test", "last": "Person", "email": "test.person@example.org"})).unwrap()["id"].as_u64().unwrap();
    let contact = s.mailbox.contact(id).unwrap();
    assert_eq!(contact.origin.account, account);
    assert!(contact.origin.id.is_empty());
    assert_eq!(s.mailbox.contact_changes.len(), 1);
    assert_eq!(s.mailbox.contact_changes[0].kind, ContactChangeKind::Create);

    let upload = contact_upload(&s, account).unwrap();
    assert_eq!(upload.account, account);
    assert_eq!(upload.ops.len(), 1);
    let op = upload.ops[0].clone();
    assert_eq!(op.kind, ContactChangeKind::Create);
    assert_eq!(op.card.first, "Test");
    assert!(op.remote.is_empty());

    let remote = "https://mail.example.org/SOGo/dav/sam@example.org/Contacts/personal/test-person.vcf";
    let mut uploaded = answered(op, ContactResult::Done, remote, "etag-1");
    uploaded.account = account;
    apply_contact_changes(&mut s, &uploaded).unwrap();
    assert!(s.mailbox.contact_changes.is_empty());
    let contact = s.mailbox.contact(id).unwrap();
    assert_eq!(contact.origin.id, remote);
    assert_eq!(contact.origin.change_key, "etag-1");
}

/// A change to a contact the server has goes as the whole card, under the ETag it was
/// downloaded with; the server's "done" gives it the new ETag.
#[test]
fn a_change_to_a_downloaded_contact_is_sent_under_its_etag() {
    let (mut s, account) = session();
    let remote = "https://mail.example.org/SOGo/dav/sam@example.org/Contacts/personal/side.vcf";
    let id = downloaded(&mut s, account, remote, "etag-1", "Server");
    s.run("people.edit", &json!({"id": id, "first": "Changed"})).unwrap();

    let upload = contact_upload(&s, account).unwrap();
    assert_eq!(upload.ops.len(), 1);
    let op = upload.ops[0].clone();
    assert_eq!(op.kind, ContactChangeKind::Update);
    assert_eq!(op.remote, remote);
    assert_eq!(op.etag, "etag-1");
    assert_eq!(op.card.first, "Changed");

    let mut uploaded = answered(op, ContactResult::Done, remote, "etag-2");
    uploaded.account = account;
    apply_contact_changes(&mut s, &uploaded).unwrap();
    assert!(s.mailbox.contact_changes.is_empty());
    assert_eq!(s.mailbox.contact(id).unwrap().origin.change_key, "etag-2");
}

/// A deletion of a contact the server has goes as a delete of its resource; of one whose create
/// was still waiting, it goes as nothing at all.
#[test]
fn a_delete_goes_for_a_downloaded_contact_and_away_for_a_waiting_one() {
    let (mut s, account) = session();
    show(&mut s, account);
    let remote = "https://mail.example.org/SOGo/dav/sam@example.org/Contacts/personal/side.vcf";
    let kept = downloaded(&mut s, account, remote, "etag-1", "Server");
    let made = s.run("people.new", &json!({"first": "New", "email": "new@example.org"})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(s.mailbox.contact_changes.len(), 1);

    s.run("people.delete", &json!({"id": kept})).unwrap();
    s.run("people.delete", &json!({"id": made})).unwrap();
    let upload = contact_upload(&s, account).unwrap();
    assert_eq!(upload.ops.len(), 1);
    let op = upload.ops[0].clone();
    assert_eq!(op.kind, ContactChangeKind::Delete);
    assert_eq!(op.remote, remote);
    assert_eq!(op.etag, "etag-1");
    assert!(op.card.first.is_empty());

    let mut uploaded = answered(op, ContactResult::Done, remote, "");
    uploaded.account = account;
    apply_contact_changes(&mut s, &uploaded).unwrap();
    assert!(s.mailbox.contact_changes.is_empty());
}

/// A change the server couldn't take waits to be tried again; one it took is let go of.
#[test]
fn a_failed_change_waits_and_a_taken_one_is_let_go() {
    let (mut s, account) = session();
    let remote = "https://mail.example.org/SOGo/dav/sam@example.org/Contacts/personal/side.vcf";
    let id = downloaded(&mut s, account, remote, "etag-1", "Server");
    s.run("people.edit", &json!({"id": id, "first": "Changed"})).unwrap();
    let op = contact_upload(&s, account).unwrap().ops[0].clone();

    let mut uploaded = answered(op.clone(), ContactResult::Failed, "", "");
    uploaded.account = account;
    uploaded.outcomes[0].error = "no connection".into();
    apply_contact_changes(&mut s, &uploaded).unwrap();
    assert_eq!(s.mailbox.contact_changes.len(), 1);
    let change = &s.mailbox.contact_changes[0];
    assert_eq!(change.failures, 1);
    assert!(change.retry_at > 0);
    assert_eq!(change.error, "no connection");
    // And it is not due yet: the next upload is empty.
    assert!(contact_upload(&s, account).unwrap().ops.is_empty());
}

/// A change the server says it no longer has is sent again as a create: the card whole, under
/// a name of its own.
#[test]
fn a_change_the_server_lost_is_sent_again_as_a_create() {
    let (mut s, account) = session();
    let remote = "https://mail.example.org/SOGo/dav/sam@example.org/Contacts/personal/side.vcf";
    let id = downloaded(&mut s, account, remote, "etag-1", "Server");
    s.run("people.edit", &json!({"id": id, "first": "Changed"})).unwrap();
    let op = contact_upload(&s, account).unwrap().ops[0].clone();

    let mut uploaded = answered(op, ContactResult::Gone, remote, "");
    uploaded.account = account;
    apply_contact_changes(&mut s, &uploaded).unwrap();
    let upload = contact_upload(&s, account).unwrap();
    assert_eq!(upload.ops.len(), 1);
    assert_eq!(upload.ops[0].kind, ContactChangeKind::Create);
    assert!(upload.ops[0].remote.is_empty());
    assert_eq!(upload.ops[0].card.first, "Changed");
}

/// A download of the server's copy doesn't take back a change that hasn't gone up yet.
#[test]
fn a_download_leaves_a_change_that_hasnt_gone_up_alone() {
    let (mut s, account) = session();
    let remote = "https://mail.example.org/SOGo/dav/sam@example.org/Contacts/personal/side.vcf";
    let id = downloaded(&mut s, account, remote, "etag-1", "Server");
    s.run("people.edit", &json!({"id": id, "first": "Changed"})).unwrap();
    // The server still has the old one: its download would undo the change, were it not noted.
    let download = ContactsDownload {
        account,
        complete: true,
        contacts: vec![RemoteContact {
            id: remote.into(),
            change_key: "etag-1".into(),
            first: "Server".into(),
            emails: vec![Labeled::new("work", "side@example.org")],
            ..Default::default()
        }],
        ..Default::default()
    };
    merge_contacts(&mut s.mailbox, &download).unwrap();
    assert_eq!(s.mailbox.contact(id).unwrap().first, "Changed");
    assert_eq!(s.mailbox.contact_changes.len(), 1);
}

/// A contact of an account whose contacts aren't kept on a DAV server is still refused: a
/// change made here would not reach it.
#[test]
fn a_contact_of_another_server_is_still_refused() {
    let mut s = Session::empty();
    let account = s.run("account.add", &json!({"email": "ada@example.org", "incoming": {"host": "outlook.example.org"}, "auth": "oauth2"})).unwrap()
        ["id"]
        .as_u64()
        .unwrap();
    let remote = "https://graph.example.org/contacts/1";
    let id = downloaded(&mut s, account, remote, "etag-1", "Server");
    assert!(s.run("people.edit", &json!({"id": id, "first": "Changed"})).is_err());
    assert!(s.mailbox.contact_changes.is_empty());
}

/// A contact made while "This computer" is shown stays on this computer: no account's server
/// is told of it.
#[test]
fn a_contact_made_with_no_account_shown_stays_here() {
    let (mut s, _account) = session();
    s.view.people_account = AccountChoice::Here;
    s.run("people.new", &json!({"first": "Here", "email": "here@example.org"})).unwrap();
    assert!(s.mailbox.contact_changes.is_empty());
    assert!(s.mailbox.contacts.values().all(|c| c.origin.is_local()));
}

/// With all accounts shown and only one whose contacts a DAV server keeps, a contact made is
/// that server's from the start.
#[test]
fn a_contact_made_with_all_shown_goes_to_the_only_dav_account() {
    let (mut s, account) = session();
    let id = s.run("people.new", &json!({"first": "All", "email": "all@example.org"})).unwrap()["id"].as_u64().unwrap();
    let contact = s.mailbox.contact(id).unwrap();
    assert_eq!(contact.origin.account, account);
    assert_eq!(s.mailbox.contact_changes.len(), 1);
}

/// A contact kept on this computer before there was a server to tell is created on the only
/// DAV account there is, without being noted first: the upload takes it along, and the
/// server's "done" makes it the account's.
#[test]
fn a_contact_kept_here_is_created_on_the_only_dav_server() {
    let (mut s, account) = session();
    let local = Contact { first: "Old".into(), last: "Local".into(), emails: vec![Labeled::new("work", "old@example.org")], ..Default::default() };
    let id = s.mailbox.add_contact(local);
    assert!(s.mailbox.contact(id).unwrap().origin.is_local());

    let upload = contact_upload(&s, account).unwrap();
    assert_eq!(upload.ops.len(), 1);
    let op = upload.ops[0].clone();
    assert_eq!(op.kind, ContactChangeKind::Create);
    assert_eq!(op.contact, id);
    assert_eq!(op.card.first, "Old");

    let mut uploaded = answered(op, ContactResult::Done, "https://dav.example.org/old.vcf", "etag-9");
    uploaded.account = account;
    apply_contact_changes(&mut s, &uploaded).unwrap();
    let contact = s.mailbox.contact(id).unwrap();
    assert_eq!(contact.origin.account, account);
    assert_eq!(contact.origin.id, "https://dav.example.org/old.vcf");
    assert_eq!(contact.origin.change_key, "etag-9");
}

/// With two accounts whose contacts a DAV server keeps, a contact kept on this computer is
/// nobody's to send: it stays here.
#[test]
fn a_contact_kept_here_stays_here_with_two_dav_accounts() {
    let (mut s, account) = session();
    let second = s
        .run(
            "account.add",
            &json!({"email": "sam2@example.org", "incoming": {"host": "mail.example.org", "port": 993, "security": "tls"}, "dav": {}}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let local = Contact { first: "Old".into(), emails: vec![Labeled::new("work", "old@example.org")], ..Default::default() };
    s.mailbox.add_contact(local);
    for a in [account, second] {
        assert!(contact_upload(&s, a).unwrap().ops.is_empty());
    }
}

/// A contact an import brought is an archive, not the owner's to publish: it is never sent.
#[test]
fn an_imported_contact_is_not_published() {
    let (mut s, account) = session();
    let mut local = Contact { first: "Imported".into(), emails: vec![Labeled::new("work", "imp@example.org")], ..Default::default() };
    local.imported_from = "old.pst".into();
    s.mailbox.add_contact(local);
    assert!(contact_upload(&s, account).unwrap().ops.is_empty());
}

/// A contact kept on this computer that is changed while all accounts are shown becomes the
/// only DAV account's: it is created there.
#[test]
fn a_change_to_a_contact_kept_here_makes_it_the_dav_accounts() {
    let (mut s, account) = session();
    let local = Contact { first: "Old".into(), emails: vec![Labeled::new("work", "old@example.org")], ..Default::default() };
    let id = s.mailbox.add_contact(local);
    s.run("people.edit", &json!({"id": id, "first": "Changed"})).unwrap();
    assert_eq!(s.mailbox.contact(id).unwrap().origin.account, account);
    let upload = contact_upload(&s, account).unwrap();
    assert_eq!(upload.ops.len(), 1);
    assert_eq!(upload.ops[0].kind, ContactChangeKind::Create);
    assert_eq!(upload.ops[0].card.first, "Changed");
}
