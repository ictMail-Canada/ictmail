// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the CalDAV/CardDAV protocol documents in `protocol.rs`: request bodies and the
//! multistatus readers. Answers are synthetic; no server is involved.
use super::*;
use crate::xml;

fn root(body: &str) -> xml::Node {
    xml::parse(body.as_bytes()).expect("test multistatus parses")
}

const MULTI: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:response>
    <d:href>/SOGo/dav/user@example.com/</d:href>
    <d:propstat><d:prop>
      <d:current-user-principal><d:href>/SOGo/dav/user@example.com/</d:href></d:current-user-principal>
      <c:calendar-home-set><d:href>/SOGo/dav/user@example.com/Calendar/</d:href></c:calendar-home-set>
      <card:addressbook-home-set><d:href>/SOGo/dav/user@example.com/Contacts/</d:href></card:addressbook-home-set>
    </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
  <d:response>
    <d:href>/SOGo/dav/user@example.com/Calendar/</d:href>
    <d:propstat><d:prop>
      <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
      <d:displayname>Calendar</d:displayname>
      <d:supported-report-set>
        <d:supported-report><d:report-name>sync-collection</d:report-name></d:supported-report>
        <d:supported-report><d:report-name>calendar-query</d:report-name></d:supported-report>
      </d:supported-report-set>
    </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
  <d:response>
    <d:href>/SOGo/dav/user@example.com/Contacts/</d:href>
    <d:propstat><d:prop>
      <d:resourcetype><d:collection/><card:addressbook/></d:resourcetype>
      <d:displayname>Contacts</d:displayname>
      <d:supported-report-set><d:supported-report><d:report-name>sync-collection</d:report-name></d:supported-report></d:supported-report-set>
    </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
</d:multistatus>"#;

#[test]
fn propfind_bodies_ask_for_their_properties() {
    let self_ = current_user_principal();
    assert!(self_.contains("<d:current-user-principal/>"), "{self_}");
    let principal = principal_collections();
    for want in ["<c:calendar-home-set/>", "<card:addressbook-home-set/>", "<d:resourcetype/>"] {
        assert!(principal.contains(want), "{want} missing from {principal}");
    }
    assert!(collections_in_home().contains("<d:supported-report-set/>"));
    assert!(collection_state().contains("<d:sync-token/>"));
}

#[test]
fn answers_carry_href_and_successful_properties_only() {
    let r = root(MULTI);
    let answers = answers(&r);
    assert_eq!(answers.len(), 3);
    assert_eq!(answers[0].href, "/SOGo/dav/user@example.com/");
    // The principal's home sets come through structured, not as text.
    assert_eq!(home_sets(&answers[0], xml::CALDAV, "calendar-home-set"), vec!["/SOGo/dav/user@example.com/Calendar/"]);
    assert_eq!(home_sets(&answers[0], xml::CARDDAV, "addressbook-home-set"), vec!["/SOGo/dav/user@example.com/Contacts/"]);
    let calendar = kind(&answers[1]);
    assert!(calendar.collection && calendar.calendar && !calendar.addressbook && !calendar.principal && !calendar.directory);
    let addressbook = kind(&answers[2]);
    assert!(addressbook.collection && addressbook.addressbook && !addressbook.calendar && !addressbook.directory);
    assert!(supports_sync(&answers[1]));
    assert!(supports_sync(&answers[2]));
}

#[test]
fn kind_marks_a_directory_of_users_apart_from_a_book() {
    // SOGo's all-accounts list carries `addressbook` and `directory` together: it is a
    // lookup listing, not a book the owner keeps in step.
    let body = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:response><d:href>/SOGo/dav/user@example.com/Contacts/mail_users/</d:href><d:propstat><d:prop>
    <d:resourcetype><d:collection/><card:addressbook/><card:directory/></d:resourcetype>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
    let r = root(body);
    let k = kind(&answers(&r)[0]);
    assert!(k.addressbook && k.directory && k.collection);
}

const SYNC: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/SOGo/dav/user@example.com/Calendar/personal/evt1.ics</d:href>
    <d:propstat><d:prop>
      <d:getetag>&quot;abc123&quot;</d:getetag>
      <c:calendar-data>BEGIN:VCALENDAR&#10;END:VCALENDAR</c:calendar-data>
    </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
  <d:response>
    <d:href>/SOGo/dav/user@example.com/Calendar/personal/gone.ics</d:href>
    <d:propstat><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
  </d:response>
</d:multistatus>"#;

#[test]
fn changes_report_documents_and_deletions() {
    let r = root(SYNC);
    let changes = changes(&r, xml::CALDAV, "calendar-data");
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].href, "/SOGo/dav/user@example.com/Calendar/personal/evt1.ics");
    assert_eq!(changes[0].etag, Some("\"abc123\""));
    assert_eq!(changes[0].data, Some("BEGIN:VCALENDAR\nEND:VCALENDAR"));
    // A resource with no successful getetag and no document is gone.
    assert_eq!(changes[1].etag, None);
    assert_eq!(changes[1].data, None);
}

#[test]
fn sync_collection_body_carries_token_and_data_property() {
    let body = sync_collection("tok-9", (xml::CALDAV, "calendar-data"));
    assert!(body.contains("<d:sync-token>tok-9</d:sync-token>"), "{body}");
    assert!(body.contains("<d:sync-level>1</d:sync-level>"));
    assert!(body.contains("<c:calendar-data/>"));
    let first = sync_collection("", (xml::CARDDAV, "card-data"));
    assert!(!first.contains("sync-token"), "{first}");
    assert!(first.contains("<card:card-data/>"));
    // A token with markup is escaped, never injected.
    let hostile = sync_collection("<x>", (xml::CALDAV, "calendar-data"));
    assert!(hostile.contains("&lt;x&gt;"), "{hostile}");
    assert!(!hostile.contains("<x>"));
}

#[test]
fn collection_state_reads_the_next_sync_token() {
    let body = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/SOGo/dav/user@example.com/Calendar/personal/</d:href>
    <d:propstat><d:prop>
      <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
      <d:sync-token>https://example.com/token-42</d:sync-token>
    </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
</d:multistatus>"#;
    let r = root(body);
    assert_eq!(sync_token(&r).as_deref(), Some("https://example.com/token-42"));
}

#[test]
fn a_failed_propstat_contributes_no_properties() {
    let body = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/x/</d:href>
    <d:propstat><d:prop><d:displayname>Real</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
    <d:propstat><d:prop><d:getetag>fake</d:getetag></d:prop><d:status>HTTP/1.1 403 Forbidden</d:status></d:propstat>
  </d:response>
</d:multistatus>"#;
    let r = root(body);
    let a = &answers(&r)[0];
    assert!(prop(a, xml::DAV, "displayname").is_some());
    assert!(prop(a, xml::DAV, "getetag").is_none());
}
