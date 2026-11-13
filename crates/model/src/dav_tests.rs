// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
use super::*;
use crate::{Freq, Recurrence};

#[test]
fn unknown_permissions_never_imply_write_access() {
    let c: Collection = serde_json::from_str(r#"{"account":7,"href":"/calendar/"}"#).unwrap();
    assert!(c.read_only);
    assert_eq!(c.account, 7);
}

#[test]
fn a_snapshot_round_trips_the_opaque_document_and_etag() {
    let r =
        Resource { account: 7, href: "/calendar/item.ics".into(), etag: "\"opaque\"".into(), raw: "X-PRIVATE:kept\r\n".into(), ..Default::default() };
    let encoded = serde_json::to_string(&r).unwrap();
    let got: Resource = serde_json::from_str(&encoded).unwrap();
    assert_eq!(got.raw, r.raw);
    assert_eq!(got.etag, r.etag);
    assert_eq!(got.account, r.account);
    assert!(!encoded.contains("password"));
}

fn imap_account() -> Account {
    Account {
        kind: AccountKind::Imap,
        email: "sam@example.org".into(),
        username: "sam".into(),
        incoming: Some(crate::Server { host: "mail.example.org".into(), port: 993, security: crate::Security::Tls }),
        ..Default::default()
    }
}

#[test]
fn a_dav_address_follows_the_incoming_servers_rules() {
    assert!(DavServer::check_url("https://mail.example.org/SOGo/dav/sam/").is_ok());
    assert!(DavServer::check_url("http://127.0.0.1:8080/dav").is_ok());
    // Plain http anywhere but this computer, a user name in the address, and junk all fail.
    assert!(DavServer::check_url("http://mail.example.org/dav").is_err());
    assert!(DavServer::check_url("https://sam:secret@mail.example.org/dav").is_err());
    assert!(DavServer::check_url("not an address").is_err());
}

#[test]
fn an_empty_dav_url_means_the_sogo_layout_of_the_mail_host() {
    let account = imap_account();
    let dav = DavServer::default();
    assert_eq!(dav.effective(&account).as_deref(), Some("https://mail.example.org/SOGo/dav/sam/"));
    // A sign-in name of its own goes in the path instead.
    let dav = DavServer { username: "sam@example.org".into(), ..Default::default() };
    assert_eq!(dav.effective(&account).as_deref(), Some("https://mail.example.org/SOGo/dav/sam@example.org/"));
    // An address of its own wins over the suggestion.
    let dav = DavServer { url: "https://dav.example.net/".into(), ..Default::default() };
    assert_eq!(dav.effective(&account).as_deref(), Some("https://dav.example.net"));
    // With nothing to derive from, there is no address: no incoming server, or another kind.
    let local = Account { kind: AccountKind::Local, ..Default::default() };
    assert_eq!(DavServer::default().effective(&local), None);
    let no_incoming = Account { kind: AccountKind::Imap, email: "sam@example.org".into(), ..Default::default() };
    assert_eq!(DavServer::default().effective(&no_incoming), None);
}

#[test]
fn the_sign_in_name_is_the_setting_then_the_account() {
    let account = imap_account();
    let dav = DavServer::default();
    assert_eq!(dav_user(&dav, &account), "sam");
    let named = DavServer { username: "svc-dav".into(), ..Default::default() };
    assert_eq!(dav_user(&named, &account), "svc-dav");
    let no_username = Account { username: String::new(), ..imap_account() };
    assert_eq!(dav_user(&dav, &no_username), "sam@example.org");
}

#[test]
fn a_bad_setting_is_named_by_its_field() {
    let bad_url = DavServer { url: "http://example.org/".into(), ..Default::default() };
    assert!(bad_url.problem().is_some());
    let bad_user = DavServer { username: "a\nb".into(), ..Default::default() };
    assert!(bad_user.problem().is_some());
    let bad_pin = DavServer { trusted: "xyz".into(), ..Default::default() };
    assert!(bad_pin.problem().is_some());
    let pin = "a".repeat(64);
    assert!(DavServer { trusted: pin, ..Default::default() }.problem().is_none());
}

#[test]
fn instance_stamps_parse_as_the_day_they_name() {
    assert_eq!(instance_of("abc-123#20261014").map(|(u, d)| (u.to_string(), d)), Some(("abc-123".to_string(), Date::new(2026, 10, 14).unwrap())));
    assert_eq!(instance_of("abc-123#20261014T093000").map(|(u, _)| u), Some("abc-123"));
    assert_eq!(instance_of("abc-123#20261014T093000Z").map(|(u, _)| u), Some("abc-123"));
    // A uid with a # that isn't a stamp, an empty base, or no # at all is no instance.
    assert_eq!(instance_of("abc#def"), None);
    assert_eq!(instance_of("#20261014"), None);
    assert_eq!(instance_of("abc-123"), None);
    assert_eq!(instance_of("abc#20261340"), None);
}

fn timed_event(uid: &str) -> Event {
    Event {
        uid: uid.into(),
        title: "Standup".into(),
        start: DateTime::new(Date::new(2026, 10, 14).unwrap(), 9, 0, 0).unwrap(),
        end: DateTime::new(Date::new(2026, 10, 14).unwrap(), 9, 30, 0).unwrap(),
        ..Default::default()
    }
}

#[test]
fn a_single_event_keeps_its_href_and_tag() {
    let remote = event_to_remote(&timed_event("evt-1"), "/calendar/evt-1.ics", "\"e1\"", Zone::UTC, "sam@example.org", None);
    assert_eq!(remote.kind, RemoteEventKind::Single);
    assert_eq!(remote.id, "/calendar/evt-1.ics");
    assert_eq!(remote.change_key, "\"e1\"");
    assert_eq!(remote.uid, "evt-1");
    assert_eq!(remote.series, "");
    assert_eq!(remote.start, DateTime::new(Date::new(2026, 10, 14).unwrap(), 9, 0, 0).unwrap().to_timestamp(0));
}

#[test]
fn a_recurring_event_is_a_series_master_with_its_skipped_days() {
    let mut event = timed_event("series-1");
    event.recurrence = Some(Recurrence { freq: Freq::Daily, ..Default::default() });
    event.exceptions = vec![Date::new(2026, 10, 15).unwrap(), Date::new(2026, 10, 16).unwrap()];
    let remote = event_to_remote(&event, "/calendar/series-1.ics", "\"e2\"", Zone::UTC, "sam@example.org", None);
    assert_eq!(remote.kind, RemoteEventKind::SeriesMaster);
    assert!(remote.recurrence.is_some());
    assert_eq!(remote.exceptions, event.exceptions);
    assert_eq!(remote.series_day, Some(event.start.date));
}

#[test]
fn an_exception_names_its_series_and_day() {
    let event = timed_event("series-1#20261015T090000");
    let remote = event_to_remote(&event, "/calendar/series-1.ics", "\"e3\"", Zone::UTC, "sam@example.org", Some("/calendar/series-1.ics"));
    assert_eq!(remote.kind, RemoteEventKind::Exception);
    assert_eq!(remote.series, "/calendar/series-1.ics");
    assert_eq!(remote.series_day, Some(Date::new(2026, 10, 15).unwrap()));
    assert_eq!(remote.uid, "series-1");
    // Without the master's address it is still an exception, of a series not yet known.
    let remote = event_to_remote(&event, "/calendar/series-1.ics", "\"e3\"", Zone::UTC, "sam@example.org", None);
    assert_eq!(remote.kind, RemoteEventKind::Exception);
    assert_eq!(remote.series, "");
}

#[test]
fn a_whole_day_event_is_utc_midnight_of_its_days() {
    let event = Event {
        uid: "allday".into(),
        all_day: true,
        start: DateTime::at_midnight(Date::new(2026, 10, 14).unwrap()),
        end: DateTime::at_midnight(Date::new(2026, 10, 16).unwrap()),
        ..Default::default()
    };
    let remote = event_to_remote(&event, "/calendar/allday.ics", "\"e4\"", Zone::UTC, "sam@example.org", None);
    assert!(remote.all_day);
    assert_eq!(remote.start, DateTime::at_midnight(Date::new(2026, 10, 14).unwrap()).to_timestamp(0));
    assert_eq!(remote.end, DateTime::at_midnight(Date::new(2026, 10, 16).unwrap()).to_timestamp(0));
}

#[test]
fn the_event_zone_wins_over_the_shown_one() {
    let event = Event { uid: "zoned".into(), start_zone: "Europe/Berlin".into(), ..timed_event("zoned") };
    let shown = Zone::UTC;
    let remote = event_to_remote(&event, "/c/z.ics", "\"e5\"", shown, "sam@example.org", None);
    let berlin = Zone::named("Europe/Berlin").unwrap();
    assert_eq!(remote.start, berlin.instant(event.start));
    assert_eq!(remote.start_zone, "Europe/Berlin");
}

#[test]
fn the_owners_answer_is_read_from_their_attendee_row() {
    let mut event = timed_event("meet");
    event.attendees = vec![
        crate::Attendee { address: crate::Address::new("Sam", "sam@example.org"), response: Response::Accepted, ..Default::default() },
        crate::Attendee { address: crate::Address::new("Jo", "jo@example.org"), response: Response::Declined, ..Default::default() },
    ];
    let remote = event_to_remote(&event, "/c/m.ics", "\"e6\"", Zone::UTC, "sam@example.org", None);
    assert_eq!(remote.response, Response::Accepted);
    // With no organizer named, whether the owner organizes it is left to be told.
    assert_eq!(remote.is_organizer, None);
    event.organizer = Some(crate::Address::new("Jo", "jo@example.org"));
    let remote = event_to_remote(&event, "/c/m.ics", "\"e6\"", Zone::UTC, "sam@example.org", None);
    assert_eq!(remote.is_organizer, Some(false));
    event.organizer = Some(crate::Address::new("Sam", "sam@example.org"));
    let remote = event_to_remote(&event, "/c/m.ics", "\"e6\"", Zone::UTC, "sam@example.org", None);
    assert_eq!(remote.is_organizer, Some(true));
}

#[test]
fn a_contact_keeps_what_a_card_says() {
    let contact = Contact {
        display_name: "Sam Example".into(),
        first: "Sam".into(),
        emails: vec![crate::Labeled::new("work", "sam@example.org")],
        ..Default::default()
    };
    let remote = contact_to_remote(&contact, "/addressbook/sam.vcf", "\"c1\"");
    assert_eq!(remote.id, "/addressbook/sam.vcf");
    assert_eq!(remote.change_key, "\"c1\"");
    assert_eq!(remote.display_name, "Sam Example");
    assert_eq!(remote.emails.len(), 1);
}

#[test]
fn calendars_download_lists_only_calendars() {
    let account = imap_account();
    let collections = vec![
        Collection { account: account.id, href: "/dav/cal/".into(), name: "Home".into(), contacts: false, read_only: false },
        Collection { account: account.id, href: "/dav/ab/".into(), name: "People".into(), contacts: true, read_only: true },
    ];
    let download = calendars_download(&collections, &account, true);
    assert!(download.complete);
    assert_eq!(download.calendars.len(), 1);
    assert_eq!(download.calendars[0].id, "/dav/cal/");
    assert_eq!(download.calendars[0].rights, Rights::Edit);
    assert_eq!(download.account, account.id);
    assert_eq!(download.email, "sam@example.org");
}

#[test]
fn sync_state_is_bounded() {
    let mut sync = DavSync {
        collections: (0..300).map(|i| Collection { href: format!("/c/{i}/"), name: "x".repeat(600), ..Default::default() }).collect(),
        ..Default::default()
    };
    sync.tokens.insert("/c/0/".into(), "t".repeat(3_000));
    sync.tokens.insert("k".repeat(2_000), "token".into());
    sync.clamp();
    assert_eq!(sync.collections.len(), DavSync::MAX_COLLECTIONS);
    assert!(sync.collections.iter().all(|c| c.name.chars().count() <= 500));
    assert!(sync.tokens.values().all(|t| t.chars().count() <= DavSync::MAX_TOKEN));
    assert!(!sync.tokens.contains_key(&"k".repeat(2_000)));
}
