// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for `dav`: the href joining and origin rules, and whole discovery/sync exchanges
//! against the loopback fake. No real server is involved; the fake speaks plain HTTP on
//! 127.0.0.1, which `https` permits for tests alone.
use super::*;
use crate::fake::FakeHttp;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn respond(status: &str, body: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
}

fn respond_etag(status: &str, body: &str, etag: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        .into_bytes()
}

fn redirect_to(location: &str) -> Vec<u8> {
    format!("HTTP/1.1 301 Moved Permanently\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
}

const NS: &str = "xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\" xmlns:card=\"urn:ietf:params:xml:ns:carddav\"";

const PRINCIPAL_SELF: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response><d:href>/dav/user@example.com/</d:href><d:propstat><d:prop>
    <d:current-user-principal><d:href>/dav/user@example.com/principal/</d:href></d:current-user-principal>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;

const PRINCIPAL_DEPTH1: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:response><d:href>/dav/user@example.com/principal/</d:href><d:propstat><d:prop>
    <c:calendar-home-set><d:href>/dav/user@example.com/Calendar/</d:href></c:calendar-home-set>
    <card:addressbook-home-set><d:href>/dav/user@example.com/Contacts/</d:href></card:addressbook-home-set>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;

const CALENDAR_HOME: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response><d:href>/dav/user@example.com/Calendar/personal.ics</d:href><d:propstat><d:prop>
    <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
    <d:displayname>Personal</d:displayname>
    <d:current-user-privilege-set><d:privilege><d:read/></d:privilege><d:privilege><d:write/></d:privilege></d:current-user-privilege-set>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;

const CONTACTS_HOME: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:response><d:href>/dav/user@example.com/Contacts/personal.vcf</d:href><d:propstat><d:prop>
    <d:resourcetype><d:collection/><card:addressbook/></d:resourcetype>
    <d:displayname>Personal</d:displayname>
    <d:current-user-privilege-set><d:privilege><d:read/></d:privilege></d:current-user-privilege-set>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;

fn state(token: &str, sync: bool) -> String {
    let reports = if sync {
        r#"<d:supported-report-set><d:supported-report><d:report-name>sync-collection</d:report-name></d:supported-report></d:supported-report-set>"#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0"?><d:multistatus {NS}><d:response><d:href>/dav/user@example.com/Calendar/personal.ics</d:href><d:propstat><d:prop>
        <d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:sync-token>{token}</d:sync-token>{reports}
        </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#
    )
}

const SYNC_ANSWER: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response><d:href>/dav/user@example.com/Calendar/personal.ics/evt1.ics</d:href><d:propstat><d:prop>
    <d:getetag>&quot;v1&quot;</d:getetag>
    <c:calendar-data>BEGIN:VCALENDAR&#10;END:VCALENDAR</c:calendar-data>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
  <d:response><d:href>/dav/user@example.com/Calendar/personal.ics/gone.ics</d:href><d:propstat><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response>
</d:multistatus>"#;

fn client<'a>(base: &'a str) -> Dav<'a> {
    Dav { base, user: "user@example.com", password: "secret", trusted: "" }
}

#[test]
fn hrefs_join_against_their_base() {
    let b = "https://mail.example.com/SOGo/dav/user@example.com/";
    assert_eq!(resolve(b, "evt.ics").as_deref(), Some("https://mail.example.com/SOGo/dav/user@example.com/evt.ics"));
    assert_eq!(resolve(b, "/abs/x").as_deref(), Some("https://mail.example.com/abs/x"));
    assert_eq!(resolve(b, "https://mail.example.com/other").as_deref(), Some("https://mail.example.com/other"));
    assert_eq!(resolve("https://h/a/b", "../x").as_deref(), Some("https://h/x"));
    assert_eq!(resolve("https://h/a/b/", "./c").as_deref(), Some("https://h/a/b/c"));
    assert_eq!(resolve(b, "sub/").as_deref(), Some("https://mail.example.com/SOGo/dav/user@example.com/sub/"));
    // What must never become a request target.
    assert_eq!(resolve(b, "//evil.example/x"), None);
    assert_eq!(resolve(b, "x?y=1"), None);
    assert_eq!(resolve(b, "x#f"), None);
    assert_eq!(resolve(b, "http://mail.example.com/x"), None, "plain http off loopback is refused");
    assert!(same_origin("https://h.example/a", "https://h.example:443/b"));
    assert!(!same_origin("https://h.example/a", "http://h.example/b"));
    assert!(!same_origin("https://h.example/a", "https://h.example:8443/b"));
}

#[test]
fn discovery_walks_principal_home_sets_and_collections() {
    let server = FakeHttp::start(|_, req| {
        if req.starts_with("PROPFIND /dav/user@example.com/principal/") {
            respond("207 Multi-Status", PRINCIPAL_DEPTH1)
        } else if req.starts_with("PROPFIND /dav/user@example.com/Calendar/") {
            respond("207 Multi-Status", CALENDAR_HOME)
        } else if req.starts_with("PROPFIND /dav/user@example.com/Contacts/") {
            respond("207 Multi-Status", CONTACTS_HOME)
        } else {
            respond("207 Multi-Status", PRINCIPAL_SELF)
        }
    });
    let base = server.url("/dav/user@example.com/");
    let found = client(&base).discover().unwrap();
    assert_eq!(found.len(), 2);
    let cal = found.iter().find(|c| !c.contacts).unwrap();
    assert_eq!(cal.href, server.url("/dav/user@example.com/Calendar/personal.ics"));
    assert_eq!(cal.name, "Personal");
    assert!(!cal.read_only, "a write privilege means writable");
    let ab = found.iter().find(|c| c.contacts).unwrap();
    assert_eq!(ab.href, server.url("/dav/user@example.com/Contacts/personal.vcf"));
    assert!(ab.read_only, "read alone means read-only");
    // The credentials went to the one server, as Basic.
    assert!(server.request(0).contains("Authorization: Basic "));
}

#[test]
fn sync_reports_changes_and_the_next_token() {
    let counter = Arc::new(AtomicUsize::new(0));
    let states = counter.clone();
    let server = FakeHttp::start(move |_, req| {
        if req.starts_with("REPORT ") {
            return respond("207 Multi-Status", SYNC_ANSWER);
        }
        if req.starts_with("PROPFIND ") {
            let n = states.fetch_add(1, Ordering::SeqCst);
            return respond("207 Multi-Status", &state(if n == 0 { "tok-1" } else { "tok-2" }, true));
        }
        respond("404 Not Found", "")
    });
    let url = server.url("/dav/user@example.com/Calendar/personal.ics");
    let synced = client("http://127.0.0.1/").sync(&url, "tok-1", false).unwrap();
    assert_eq!(synced.token.as_deref(), Some("tok-2"));
    assert_eq!(synced.changes.len(), 2);
    assert_eq!(synced.changes[0].href, server.url("/dav/user@example.com/Calendar/personal.ics/evt1.ics"));
    assert_eq!(synced.changes[0].etag, "\"v1\"");
    assert_eq!(synced.changes[0].data.as_deref(), Some("BEGIN:VCALENDAR\nEND:VCALENDAR"));
    assert_eq!(synced.changes[1].etag, "", "a removal carries no tag");
    // Two state reads (before and after the report) plus the report itself.
    assert_eq!(server.asked(), 3);
    assert!(server.request(1).starts_with("REPORT "), "the report went between the two state reads");
    assert!(server.request(1).contains("<d:sync-token>tok-1</d:sync-token>"));
}

#[test]
fn a_stale_token_restarts_the_sync_once() {
    let counter = Arc::new(AtomicUsize::new(0));
    let reports = counter.clone();
    let server = FakeHttp::start(move |_, req| {
        if req.starts_with("REPORT ") {
            if reports.fetch_add(1, Ordering::SeqCst) == 0 {
                return respond("410 Gone", "");
            }
            return respond("207 Multi-Status", SYNC_ANSWER);
        }
        respond("207 Multi-Status", &state("tok-9", true))
    });
    let url = server.url("/dav/user@example.com/Calendar/personal.ics");
    let synced = client("http://127.0.0.1/").sync(&url, "old-token", false).unwrap();
    assert_eq!(synced.changes.len(), 2);
    assert_eq!(counter.load(Ordering::SeqCst), 2, "the stale token was retried once");
    assert!(server.request(1).contains("old-token"), "the first report carried the stale token");
    assert!(!server.request(2).contains("sync-token"), "the retry asked for everything");
}

#[test]
fn a_collection_without_sync_collection_is_listed_whole() {
    let server = FakeHttp::start(|_, req| {
        if req.starts_with("PROPFIND ") && req.contains("Depth: 1") {
            return respond(
                "207 Multi-Status",
                r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
                <d:response><d:href>/dav/user@example.com/Calendar/personal.ics</d:href><d:propstat><d:prop><d:getetag>"self"</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
                <d:response><d:href>/dav/user@example.com/Calendar/personal.ics/a.ics</d:href><d:propstat><d:prop><d:getetag>"a"</d:getetag><c:calendar-data>BEGIN:VCALENDAR</c:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
                </d:multistatus>"#,
            );
        }
        respond("207 Multi-Status", &state("tok", false))
    });
    let url = server.url("/dav/user@example.com/Calendar/personal.ics");
    let synced = client("http://127.0.0.1/").sync(&url, "", false).unwrap();
    assert_eq!(synced.token, None, "a listing keeps no token");
    assert_eq!(synced.changes.len(), 1, "the collection itself is not a member");
    assert_eq!(synced.changes[0].href, server.url("/dav/user@example.com/Calendar/personal.ics/a.ics"));
}

#[test]
fn fetch_put_and_delete_speak_http() {
    let server = FakeHttp::start(|_, req| {
        if req.starts_with("GET ") {
            respond("200 OK", "BEGIN:VCALENDAR\nEND:VCALENDAR")
        } else if req.starts_with("PUT ") {
            respond("201 Created", "")
        } else {
            respond("204 No Content", "")
        }
    });
    let client = client("http://127.0.0.1/");
    let (doc, etag) = client.fetch(&server.url("/dav/c.ics")).unwrap();
    assert_eq!(doc, "BEGIN:VCALENDAR\nEND:VCALENDAR");
    assert_eq!(etag, "");
    client.put(&server.url("/dav/c.ics"), "text/calendar; charset=utf-8", b"BEGIN:VCALENDAR", None).unwrap();
    assert!(server.request(1).contains("If-None-Match: *"));
    client.put(&server.url("/dav/c.ics"), "text/calendar; charset=utf-8", b"BEGIN:VCALENDAR", Some("\"v2\"")).unwrap();
    assert!(server.request(2).contains("If-Match: \"v2\""));
    client.delete(&server.url("/dav/c.ics"), Some("\"v2\"")).unwrap();
    assert!(server.request(3).contains("If-Match: \"v2\""));
}

#[test]
fn fetch_keeps_the_etag() {
    let server = FakeHttp::answering(respond_etag("200 OK", "the document", "\"v1\""));
    let (body, etag) = client("http://127.0.0.1/").fetch(&server.url("/dav/c.ics")).unwrap();
    assert_eq!(body, "the document");
    assert_eq!(etag, "\"v1\"");
}

#[test]
fn redirects_stay_on_their_own_server() {
    // A same-server redirect is followed.
    let same = FakeHttp::start(|n, _| if n == 0 { redirect_to("/new") } else { respond("200 OK", "the document") });
    let url = same.url("/old");
    assert_eq!(client("http://127.0.0.1/").fetch(&url).unwrap().0, "the document");
    assert_eq!(same.asked(), 2);

    // A redirect to another server is not: the 301 comes back as a status, and nothing of the
    // credential went there.
    let other = FakeHttp::answering(respond("200 OK", "not for you"));
    let away = FakeHttp::start({
        let target = other.url("/new");
        move |_, _| redirect_to(&target)
    });
    let url = away.url("/old");
    assert!(matches!(client("http://127.0.0.1/").fetch(&url), Err(DavError::Status(301))));
    assert_eq!(other.asked(), 0, "nothing went to the other server");
}

#[test]
fn refused_credentials_are_a_status_not_a_panic() {
    let server = FakeHttp::answering(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"dav\"\r\nContent-Length: 0\r\n\r\n".to_vec());
    let base = server.url("/dav/user@example.com/");
    let err = client(&base).discover();
    assert!(matches!(err, Err(DavError::Status(401))), "{err:?}");
}
