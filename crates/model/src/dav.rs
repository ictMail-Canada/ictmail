// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! DAV collection metadata, resource snapshots, the sync state a DAV server leaves behind, and
//! the bridge between what a CalDAV/CardDAV server keeps and what the mailbox keeps. No
//! credentials or I/O live here.
use std::collections::BTreeMap;

use mailcraft_time::{Date, DateTime, Timestamp, Zone};
use serde::{Deserialize, Serialize};

use crate::remote::{CalendarsDownload, RemoteCalendar, RemoteContact, RemoteEvent, RemoteEventKind, RemoteProposal};
use crate::{Account, AccountKind, Contact, Event, Id, Response, Rights};

/// One discovered calendar or address book. Permissions default to read-only until known.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Collection {
    pub account: Id,
    /// The collection's address on the server, ending in `/`.
    pub href: String,
    pub name: String,
    /// It is an address book (CardDAV), not a calendar.
    pub contacts: bool,
    pub read_only: bool,
}

impl Default for Collection {
    fn default() -> Self {
        Self { account: 0, href: String::new(), name: String::new(), contacts: false, read_only: true }
    }
}

/// The downloaded document and the parsed fields it produced. Keeping both lets a later edit
/// change known properties without rewriting unknown ones. An ETag is opaque, including quotes.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Resource {
    pub account: Id,
    pub collection: String,
    pub href: String,
    pub etag: String,
    pub raw: String,
    pub contacts: Vec<Contact>,
    pub events: Vec<Event>,
}

/// What the account's DAV server last said about its collections, and how far each has been
/// synced ([`crate::Mailbox::dav_sync`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DavSync {
    /// The collections the server last offered, as discovery found them.
    pub collections: Vec<Collection>,
    /// The sync-token each collection's next sync asks for (RFC 6578), by collection href; a
    /// collection missing here is synced in full. A token the server no longer honours is
    /// answered with an error, and the collection is synced in full again.
    pub tokens: BTreeMap<String, String>,
}

impl DavSync {
    /// Most collections one account's sync state keeps.
    pub const MAX_COLLECTIONS: usize = 256;
    /// Longest collection address or sync-token kept, in characters.
    pub const MAX_TOKEN: usize = 2_048;

    /// Cut what was read or written beyond what is kept: the collections to the most, each
    /// href and name to a line, each token to its bound.
    pub fn clamp(&mut self) {
        self.collections.truncate(Self::MAX_COLLECTIONS);
        for collection in &mut self.collections {
            collection.href = crate::remote::line(&collection.href, crate::remote::MAX_KEY);
            collection.name = crate::remote::line(&collection.name, crate::remote::MAX_LINE);
        }
        self.tokens = std::mem::take(&mut self.tokens)
            .into_iter()
            .filter(|(href, token)| href.chars().count() <= crate::remote::MAX_KEY && token.chars().count() <= Self::MAX_TOKEN)
            .take(Self::MAX_COLLECTIONS)
            .collect();
    }
}

/// Where an IMAP account's CalDAV/CardDAV server is ([`crate::Account::dav`]), for the
/// calendars and address books it keeps (SOGo on ictMail hosts, Radicale, Baïkal and the
/// rest). Plain settings: the password is the account's own, given by whoever holds the
/// connection, and is never kept here.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DavServer {
    /// The DAV address to start from: the principal's or a home set's (`https://…`). Empty
    /// means the address the account's incoming server suggests ([`DavServer::suggested`]):
    /// the SOGo layout, `https://<host>/SOGo/dav/<user>/`.
    pub url: String,
    /// The name the DAV server signs in with; empty means the account's e-mail address.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub username: String,
    /// The SHA-256 fingerprint (64 hexadecimal digits) of the one certificate the owner chose
    /// to trust for this account although the system doesn't; empty when none is.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub trusted: String,
}

/// The address, the name and the fingerprint stay out of debug output, which may reach a log.
impl std::fmt::Debug for DavServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DavServer")
            .field("url", &!self.url.is_empty())
            .field("username", &!self.username.is_empty())
            .field("trusted", &!self.trusted.is_empty())
            .finish()
    }
}

impl DavServer {
    /// Longest sign-in name kept, in characters (the address's bound is
    /// [`crate::JmapServer::MAX_SESSION`]'s, which [`DavServer::check_url`] applies).
    pub const MAX_USERNAME: usize = 254;

    /// Whether `address` may be a DAV address: the same rules a JMAP session address has
    /// (`https://` only, plain `http://` to this computer only, no user name in it, bounded).
    pub fn check_url(address: &str) -> Result<(), &'static str> {
        crate::jmap_server::accepted(address).map(|_| ())
    }

    /// Whether `name` may be a sign-in name: not too long and no control character.
    pub fn check_username(name: &str) -> Result<(), &'static str> {
        if name.chars().take(Self::MAX_USERNAME + 1).count() > Self::MAX_USERNAME {
            return Err("is longer than 254 characters");
        }
        if name.chars().any(char::is_control) {
            return Err("has a control character in it");
        }
        Ok(())
    }

    /// Whether `fingerprint` is none, or a SHA-256 fingerprint as 64 hexadecimal digits.
    pub fn check_fingerprint(fingerprint: &str) -> Result<(), &'static str> {
        if fingerprint.is_empty() || (fingerprint.len() == 64 && fingerprint.bytes().all(|b| b.is_ascii_hexdigit())) {
            Ok(())
        } else {
            Err("isn't a SHA-256 fingerprint (64 hexadecimal digits)")
        }
    }

    /// Why these settings can't be used, when they can't. An empty `url` is fine: it means the
    /// address is the one the incoming server suggests.
    pub fn problem(&self) -> Option<&'static str> {
        if !self.url.is_empty()
            && let Err(why) = Self::check_url(&self.url)
        {
            return Some(why);
        }
        Self::check_username(&self.username).err().or_else(|| Self::check_fingerprint(&self.trusted).err())
    }

    /// The DAV address to start from for `account`: `url` when it says one, else the address
    /// the account's incoming mail server suggests — the SOGo layout ictMail hosts serve,
    /// `https://<host>/SOGo/dav/<user>/`. `None` when there is no incoming server or its host
    /// can't make one.
    pub fn effective(&self, account: &Account) -> Option<String> {
        if !self.url.is_empty() {
            return Some(self.url.trim_end_matches('/').to_string());
        }
        Self::derive(account, self.username.trim())
    }

    /// The address an IMAP account's incoming server suggests for DAV: `https://<host>/
    /// SOGo/dav/<user>/`, the user being the DAV sign-in name when the account has one set,
    /// else the account's user name, else its address. `None` for an account with no incoming
    /// server, or a host name that can't be in an address.
    pub fn suggested(account: &Account) -> Option<String> {
        let name = account.dav.as_ref().map(|dav| dav.username.trim()).unwrap_or("");
        Self::derive(account, name)
    }

    /// [`DavServer::suggested`] for a sign-in name that isn't the saved setting's yet — the
    /// settings page's field, as it is being filled in.
    pub fn suggested_as(account: &Account, username: &str) -> Option<String> {
        Self::derive(account, username)
    }

    /// `https://<host>/SOGo/dav/<user>/` for the account's incoming server, `username` (when not
    /// empty) before the account's own names for the user.
    fn derive(account: &Account, username: &str) -> Option<String> {
        if account.kind != AccountKind::Imap {
            return None;
        }
        let host = account.incoming.as_ref()?.host.trim();
        if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c)) {
            return None;
        }
        let name = if !username.is_empty() {
            username
        } else {
            let name = account.username.trim();
            if name.is_empty() { account.email.trim() } else { name }
        };
        let user = name.trim_matches('@');
        if user.is_empty() {
            return None;
        }
        Some(format!("https://{host}/SOGo/dav/{user}/"))
    }
}

/// The sign-in name a DAV exchange uses: the setting's, else the account's user name, else its
/// e-mail address.
pub fn dav_user<'a>(server: &'a DavServer, account: &'a Account) -> &'a str {
    let name = server.username.trim();
    if !name.is_empty() {
        name
    } else {
        let name = account.username.trim();
        if name.is_empty() { account.email.trim() } else { name }
    }
}

/// A contact as a server keeps it: what a download of one address book becomes for
/// `sync.applyContacts`. `href` is the resource's address and `etag` its tag; both are kept
/// so the next change can be guarded by them.
pub fn contact_to_remote(contact: &Contact, href: &str, etag: &str) -> RemoteContact {
    RemoteContact {
        id: href.to_string(),
        change_key: etag.to_string(),
        display_name: contact.display_name.clone(),
        first: contact.first.clone(),
        middle: contact.middle.clone(),
        last: contact.last.clone(),
        nickname: contact.nickname.clone(),
        company: contact.company.clone(),
        title: contact.title.clone(),
        department: contact.department.clone(),
        emails: contact.emails.clone(),
        phones: contact.phones.clone(),
        addresses: contact.addresses.clone(),
        websites: contact.websites.clone(),
        birthday: contact.birthday,
        notes: contact.notes.clone(),
        modified: 0,
    }
}

/// An event as a server keeps it: what a download of one calendar becomes for
/// `sync.applyEvents`. `href` is the resource's address and `etag` its tag.
///
/// `zone` is the zone the calendar is shown in, for an event that has no zone of its own.
/// `owner` is the account's address, for the response the owner gave a meeting they were
/// invited to. `master` is the resource address of the series an exception belongs to, for a
/// `VEVENT` with a `RECURRENCE-ID` (the iCalendar reader writes its uid as `<uid>#<stamp>`);
/// pass `None` when it isn't known, and the exception is taken as an event of its own.
pub fn event_to_remote(event: &Event, href: &str, etag: &str, zone: Zone, owner: &str, master: Option<&str>) -> RemoteEvent {
    let (kind, series, series_day, uid) = match instance_of(&event.uid) {
        Some((base, day)) => (RemoteEventKind::Exception, master.unwrap_or("").to_string(), Some(day), base.to_string()),
        None if event.recurrence.is_some() => (RemoteEventKind::SeriesMaster, String::new(), Some(event.start.date), event.uid.clone()),
        None => (RemoteEventKind::Single, String::new(), None, event.uid.clone()),
    };
    let owner = owner.trim();
    let response = event.attendees.iter().find(|who| who.address.email.eq_ignore_ascii_case(owner)).map_or(Response::None, |who| who.response);
    let event_zone = || zone_named(&event.start_zone).unwrap_or(zone);
    RemoteEvent {
        id: href.to_string(),
        change_key: etag.to_string(),
        uid,
        title: event.title.clone(),
        location: event.location.clone(),
        description: event.description.clone(),
        start: instant_of(event.start, &event.start_zone, zone, event.all_day),
        end: instant_of(event.end, &event.end_zone, zone, event.all_day),
        all_day: event.all_day,
        start_zone: event.start_zone.clone(),
        end_zone: event.end_zone.clone(),
        organizer: event.organizer.clone(),
        attendees: event.attendees.clone(),
        response,
        show_as: event.show_as,
        private: event.private,
        cancelled: event.cancelled,
        kind,
        series,
        reminder_minutes: event.reminder_minutes,
        modified: 0,
        is_organizer: event.organizer.as_ref().map(|who| who.email.eq_ignore_ascii_case(owner)),
        token: String::new(),
        recurrence: if kind == RemoteEventKind::SeriesMaster { event.recurrence.clone() } else { None },
        series_day,
        original: None,
        categories: event.categories.clone(),
        importance: event.importance,
        options: event.response_options,
        online: event.online.clone(),
        proposals: event
            .proposals
            .iter()
            .map(|p| RemoteProposal { from: p.from.clone(), start: event_zone().instant(p.start), end: event_zone().instant(p.end) })
            .collect(),
        attachments: event.has_attachments,
        exceptions: if kind == RemoteEventKind::SeriesMaster { event.exceptions.clone() } else { Vec::new() },
    }
}

/// When a wall-clock time is, as an instant: the event's own zone when it names one the zones
/// know, else the zone the calendar is shown in. A whole-day event is the UTC midnight of its
/// day, as a server counts one.
fn instant_of(wall: DateTime, zone_name: &str, shown: Zone, all_day: bool) -> Timestamp {
    if all_day {
        return DateTime::at_midnight(wall.date).to_timestamp(0);
    }
    zone_named(zone_name).unwrap_or(shown).instant(wall)
}

/// The zone a name says: an IANA name (`Europe/Berlin`), `UTC`, or an offset (`UTC+05:30`).
fn zone_named(name: &str) -> Option<Zone> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Zone::named(name).or_else(|| Zone::offset_in_name(name).and_then(Zone::fixed))
}

/// For the uid `<uid>#<stamp>` the iCalendar reader gives a `VEVENT` with a `RECURRENCE-ID`:
/// the series' uid and the day of the series it stands in for. `None` for any other uid —
/// including one whose `#` isn't followed by a date stamp.
pub fn instance_of(uid: &str) -> Option<(&str, Date)> {
    let (base, stamp) = uid.rsplit_once('#')?;
    if base.is_empty() {
        return None;
    }
    instance_day(stamp).map(|day| (base, day))
}

/// The day an iCalendar date or date-time stamp names (`20261014`, `20261014T130000`,
/// `20261014T130000Z`), as written: the zone of the stamp is the series' own, which the day
/// alone can't tell, so only the date is kept.
pub fn instance_day(stamp: &str) -> Option<Date> {
    let digits = stamp.trim().trim_end_matches('Z');
    let day = digits.get(..8)?;
    if !day.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (y, m, d) = (day[0..4].parse().ok()?, day[4..6].parse().ok()?, day[6..8].parse().ok()?);
    // A date-time stamp has a `T` after the day; a bare date doesn't. Anything else isn't one.
    let rest = &digits[8..];
    if !rest.is_empty() && !rest.starts_with('T') {
        return None;
    }
    Date::new(y, m, d)
}

/// The calendar collection that is the account's main one: the one named like a default
/// (`personal`, `default` — SOGo's own is `personal`) when there is one, else the first. The
/// same choice must be made wherever a collection is told apart as the main calendar — the
/// calendar list and the per-collection event downloads both go by it.
pub fn main_collection(collections: &[Collection]) -> Option<&Collection> {
    let calendars = collections.iter().filter(|c| !c.contacts);
    calendars
        .clone()
        .find(|c| {
            let last = c.href.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
            last.eq_ignore_ascii_case("personal") || last.eq_ignore_ascii_case("default")
        })
        .or_else(|| calendars.clone().next())
}

/// The calendars a discovery found, as the calendar list a download carries
/// (`sync.applyCalendars`): the address books are not calendars and are left out here; they
/// are synced through contacts downloads instead. The main collection is marked so the list
/// is taken in at all (a list that says which is main is the only kind `applyCalendars`
/// keeps).
pub fn calendars_download(collections: &[Collection], account: &Account, complete: bool) -> CalendarsDownload {
    let mut download = CalendarsDownload::default().for_account(account);
    download.complete = complete;
    let main = main_collection(collections).map(|c| c.href.as_str());
    for collection in collections.iter().filter(|c| !c.contacts) {
        download.calendars.push(RemoteCalendar {
            id: collection.href.clone(),
            name: collection.name.clone(),
            color: None,
            group: String::new(),
            rights: Rights::from_read_only(collection.read_only),
            owner: None,
            main: Some(collection.href.as_str()) == main,
            online_providers: None,
        });
    }
    download
}

#[cfg(test)]
#[path = "dav_tests.rs"]
mod tests;
