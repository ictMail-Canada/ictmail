// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The host's side of an IMAP account's CalDAV/CardDAV server (ictMail's SOGo, Radicale, Baïkal
//! and the rest): `mailcraft-net`'s [`dav`] does the exchanges, this does what the window needs —
//! discover the account's calendars and address books, keep each in step by its sync-token, turn
//! the `.ics`/`.vcf` documents into the downloads `sync.applyEvents`/`applyContacts`/
//! `applyCalendars` take in, and carry the event changes made here up as PUT/DELETE with the
//! ETag guards a DAV write is owed.
//!
//! **The credential** is the account's own: the sign-in name is the DAV settings' (else the
//! account's user name, else its address — [`mailcraft_model::dav::dav_user`]) and the password
//! is the one the window hands over with the job, the same the IMAP connection uses. It lives in
//! memory for the length of the run and is in no log line.
//!
//! **The calendar list** is discovered on every run (it is cheap and is how a collection that
//! appeared or went away is noticed). Each calendar collection is synced on its own sync-token
//! ([`DavSync::tokens`]); a collection the server can't sync-collection offers is listed whole
//! and its download is `complete`, which is how deletions are reconciled there. A sync-token
//! delta that reports a gone resource puts its address in the download's `gone`, which the
//! merge reads as the events whose `origin.id`/`origin.series` it is.
//!
//! **Contacts** merge every address book into one download, and a download can only say a
//! contact is gone by not listing it: a contacts delta that reports deletions is re-asked with
//! no token, so the run's download of that book is the whole book and `complete` can be said.
//!
//! **Uploads** write whole iCalendar documents: a change is laid over the copy the server has
//! ([`remote::put_fields`]) and PUT back under its ETag; a new event is a new resource
//! (`If-None-Match: *`); a deletion is a guarded DELETE. One occurrence of a series and a
//! forwarded meeting are refused in words — plain CalDAV has no way to say them.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mailcraft_engine::cmd::remote;
use mailcraft_model::dav::{self, Collection, DavSync};
use mailcraft_model::remote::{self as model_remote, EventChangeKind, EventOutcome, EventResult, EventsDownload, EventsUploaded};
use mailcraft_model::{Account, Event, Id};
use mailcraft_net::CertificateProblem;
use mailcraft_net::dav::{Dav, DavError, Synced};
use mailcraft_net::https;
use mailcraft_time::{Timestamp, Zone};

/// The job: one run of an account's DAV sync, downloads and the changes waiting to go up.
pub(crate) struct DavJob {
    /// The IMAP account the DAV settings belong to.
    pub account: Account,
    /// The account's password (the IMAP one); never logged.
    pub password: String,
    /// The zone the calendar is shown in: times are read and written by its clock.
    pub zone: Zone,
    /// This run's time, for the `DTSTAMP` a written document carries.
    pub now: Timestamp,
    /// The calendars asked for this run (as a Graph job's `asks`); empty asks for every
    /// discovered calendar collection.
    pub asks: Vec<model_remote::CalendarAsk>,
    /// Also bring the address books down.
    pub contacts: bool,
    /// What was made, changed or deleted here, for the server to be told first.
    pub changes: Option<model_remote::EventsUpload>,
    /// The sync state the mailbox last kept for the account.
    pub sync: DavSync,
    /// Set when the owner stops the run: it stops after the exchange in hand.
    pub cancel: Arc<AtomicBool>,
}

/// What a [`DavJob`] came to. Never an error as a whole: mail goes on either way.
#[derive(Default)]
pub(crate) struct DavResult {
    pub account: Id,
    /// The collections and tokens to keep for the next run.
    pub sync: DavSync,
    /// The calendars the server lists, when discovery answered.
    pub calendars: Option<model_remote::CalendarsDownload>,
    /// One download per calendar collection synced this run.
    pub events: Vec<EventsDownload>,
    /// The address books' contacts, when they were asked for and read.
    pub contacts: Option<model_remote::ContactsDownload>,
    /// What the server made of the changes it was told.
    pub uploaded: Option<EventsUploaded>,
    /// The sign-in was refused (401/403): the password is wrong or was taken back.
    pub auth_refused: bool,
    /// A certificate the system doesn't trust and the account doesn't either: the fingerprint
    /// of the one shown, for the owner to be asked about (`DavServer::trusted`).
    pub untrusted: Option<String>,
    /// What went wrong, in a few plain words; no address and no credential in it.
    pub errors: Vec<String>,
}

/// Most events one `.ics` resource is read for, and most documents one collection sync brings
/// back before the rest are left for the next run.
const MAX_ICS_EVENTS: usize = 1_000;
const MAX_CHANGES: usize = 20_000;
/// The iCalendar content type a calendar resource is written with.
const ICS_TYPE: &str = "text/calendar; charset=utf-8";

/// Run `job` to its end and answer what came of it. Blocking: the caller is a worker thread.
pub(crate) fn run(job: DavJob) -> DavResult {
    let mut result = DavResult { account: job.account.id, ..Default::default() };
    let Some(server) = job.account.dav.clone() else {
        result.errors.push("the account has no DAV settings".into());
        return result;
    };
    let Some(base) = server.effective(&job.account) else {
        result.errors.push("the account's incoming server gives no DAV address".into());
        return result;
    };
    if server.problem().is_some() {
        result.errors.push("the account's DAV settings can't be used".into());
        return result;
    }
    let user = dav::dav_user(&server, &job.account);
    let dav = Dav { base: &base, user, password: &job.password, trusted: server.trusted.as_str() };
    let mut sync = job.sync.clone();
    let offset_at = |at: Timestamp| job.zone.offset_at(at);

    // The collections are discovered first: the calendar list comes from them, the uploads
    // need them to know where a calendar's resources live, and the syncs are per collection.
    let collections = match dav.discover() {
        Ok(collections) => collections,
        Err(e) => {
            settle(&mut result, &e);
            result.sync = sync;
            return result;
        }
    };
    for collection in &collections {
        let mut kept = collection.clone();
        kept.account = job.account.id;
        sync.collections.retain(|c| c.href != kept.href);
        sync.collections.push(kept);
    }
    sync.clamp();

    // What was made, changed or deleted here goes up before anything is brought down: a change
    // the server takes is then seen in the download that follows, as the engine expects.
    if let Some(changes) = &job.changes {
        result.uploaded = Some(push(&dav, &collections, changes, job.now, job.zone, &offset_at, &job.cancel, &mut result));
        if result.auth_refused || result.untrusted.is_some() || cancelled(&job) {
            result.sync = sync;
            return result;
        }
    }

    // The calendars, each on its own token.
    let main = dav::main_collection(&collections).map(|c| c.href.clone());
    let asked: Vec<&Collection> = collections.iter().filter(|c| !c.contacts).filter(|c| wants(&job.asks, c, main.as_deref())).collect();
    for collection in asked.iter().take(DavSync::MAX_COLLECTIONS) {
        if cancelled(&job) {
            break;
        }
        let token = sync.tokens.get(&collection.href).cloned().unwrap_or_default();
        match dav.sync(&collection.href, &token, false) {
            Ok(synced) => {
                note_token(&mut sync, &collection.href, synced.token.as_deref());
                take_events(&dav, collection, &synced, main.as_deref(), &job, &offset_at, &mut result);
            }
            Err(e) => {
                settle(&mut result, &e);
                if result.auth_refused || result.untrusted.is_some() {
                    break;
                }
                result.errors.push(format!("a calendar couldn't be read ({})", say(&e)));
            }
        }
    }

    // The address books, when they were asked for: all of them in one download.
    if job.contacts {
        let mut contacts = model_remote::ContactsDownload::default().for_account(&job.account);
        contacts.complete = true;
        let mut any = false;
        for collection in collections.iter().filter(|c| c.contacts).take(DavSync::MAX_COLLECTIONS) {
            if cancelled(&job) {
                break;
            }
            let token = sync.tokens.get(&collection.href).cloned().unwrap_or_default();
            match dav.sync(&collection.href, &token, true) {
                Ok(mut synced) => {
                    // A delta that says a card is gone can't say it in a download: ask the
                    // book whole, so `complete` can be said and the merge takes the gone ones
                    // out by absence.
                    if synced.token.is_some()
                        && synced.changes.iter().any(|c| c.etag.is_empty() && c.data.is_none())
                        && let Ok(whole) = dav.sync(&collection.href, "", true)
                    {
                        synced = whole;
                    }
                    let whole = synced.token.is_none();
                    note_token(&mut sync, &collection.href, synced.token.as_deref());
                    any = true;
                    contacts.complete &= whole;
                    take_contacts(&dav, &synced, &mut contacts, &mut result.errors);
                }
                Err(e) => {
                    settle(&mut result, &e);
                    contacts.complete = false;
                    if result.auth_refused || result.untrusted.is_some() {
                        break;
                    }
                    result.errors.push(format!("an address book couldn't be read ({})", say(&e)));
                }
            }
        }
        if any && !cancelled(&job) {
            result.contacts = Some(contacts);
        }
    }

    if result.calendars.is_none() && !collections.is_empty() {
        result.calendars = Some(dav::calendars_download(&collections, &job.account, true));
    }
    sync.clamp();
    result.sync = sync;
    result
}

/// Whether the job asks for this collection: every calendar when `asks` is empty, else the
/// named ones — an ask names a calendar the way its download does, the collection's address,
/// or nothing for the main one.
fn wants(asks: &[model_remote::CalendarAsk], collection: &Collection, main: Option<&str>) -> bool {
    if asks.is_empty() {
        return true;
    }
    let named = if Some(collection.href.as_str()) == main { "" } else { collection.href.as_str() };
    asks.iter().any(|ask| ask.calendar == named)
}

/// Keep or drop a collection's next sync-token: `Some` is the token to ask with next time,
/// `None` a server with no sync-collection, whose collections are listed whole every run.
fn note_token(sync: &mut DavSync, href: &str, token: Option<&str>) {
    match token {
        Some(token) if !token.is_empty() => {
            sync.tokens.insert(href.to_string(), token.chars().take(DavSync::MAX_TOKEN).collect());
        }
        _ => {
            sync.tokens.remove(href);
        }
    }
}

fn cancelled(job: &DavJob) -> bool {
    job.cancel.load(Ordering::Relaxed)
}

/// What went wrong with one exchange, as the result carries it: the sign-in refused, a
/// certificate to ask about, or a plain error line.
fn settle(result: &mut DavResult, error: &DavError) {
    match error {
        DavError::Status(401 | 403) => result.auth_refused = true,
        DavError::Net(net) => {
            if let Some(problem) = https::certificate_problem(net) {
                match problem {
                    CertificateProblem::NotTrusted(info) | CertificateProblem::Changed(info) => {
                        result.untrusted = Some(info.fingerprint.clone());
                        return;
                    }
                    CertificateProblem::Unacceptable(_, _) => {}
                }
            }
            result.errors.push(format!("the DAV server couldn't be reached ({})", say(error)));
        }
        _ => result.errors.push(format!("the DAV server answered badly ({})", say(error))),
    }
}

/// An error in a few words that never names a server or carries a credential.
fn say(error: &DavError) -> &'static str {
    match error {
        DavError::Status(404 | 410) => "not there",
        DavError::Status(412) => "it changed while it was being written",
        DavError::Status(_) => "the server refused",
        DavError::Document => "an answer couldn't be read",
        DavError::Net(_) => "no connection",
    }
}

/// One calendar collection's sync, as the download `sync.applyEvents` takes in: the events it
/// reports, the resources it says are gone, and whether it is all the collection has.
fn take_events(
    dav: &Dav<'_>,
    collection: &Collection,
    synced: &Synced,
    main: Option<&str>,
    job: &DavJob,
    offset_at: &dyn Fn(Timestamp) -> i32,
    result: &mut DavResult,
) {
    let owner = job.account.email.trim().to_string();
    let mut download = EventsDownload::default().for_account(&job.account);
    download.calendar = if Some(collection.href.as_str()) == main { String::new() } else { collection.href.clone() };
    download.complete = synced.token.is_none();
    download.from = 0;
    download.to = Timestamp::MAX;
    download.has_categories = true;
    // (The options a `.ics` can't say — online meetings, proposals — stay as they are here.)
    download.has_options = false;

    // The documents first, so that an exception can be told its series' address: a
    // `RECURRENCE-ID` override names its series by uid, and the series' resource is the one
    // that carries that uid as a master.
    let mut parsed: Vec<(String, String, Vec<Event>)> = Vec::new();
    for change in synced.changes.iter().take(MAX_CHANGES) {
        if cancelled(job) {
            break;
        }
        if change.etag.is_empty() && change.data.is_none() {
            download.gone.push(change.href.clone());
            continue;
        }
        let doc = match &change.data {
            Some(data) => data.clone(),
            None => match dav.fetch(&change.href) {
                Ok((body, _)) => body,
                Err(e) => {
                    result.errors.push(format!("an event couldn't be read ({})", say(&e)));
                    continue;
                }
            },
        };
        let file = mailcraft_ical::parse_events(&doc, offset_at, MAX_ICS_EVENTS);
        if !file.events.is_empty() {
            parsed.push((change.href.clone(), change.etag.clone(), file.events));
        }
    }
    let mut masters: HashMap<String, String> = HashMap::new();
    for (href, _, events) in &parsed {
        for event in events {
            if event.recurrence.is_some() && dav::instance_of(&event.uid).is_none() {
                masters.insert(event.uid.clone(), href.clone());
            }
        }
    }
    for (href, etag, events) in &parsed {
        for event in events {
            let master = dav::instance_of(&event.uid).and_then(|(base, _)| masters.get(base).map(String::as_str));
            let mut remote = dav::event_to_remote(event, href, etag, job.zone, &owner, master);
            remote.tidy();
            download.events.push(remote);
        }
    }
    result.events.push(download);
}

/// One address book's sync into the contacts download: every card the sync reports, changed or
/// whole, becomes a [`RemoteContact`](model_remote::RemoteContact) the merge takes.
fn take_contacts(dav: &Dav<'_>, synced: &Synced, contacts: &mut model_remote::ContactsDownload, errors: &mut Vec<String>) {
    for change in synced.changes.iter().take(MAX_CHANGES) {
        // (A gone card was re-asked whole by the caller; nothing of it is here to add.)
        if change.etag.is_empty() && change.data.is_none() {
            continue;
        }
        let doc = match &change.data {
            Some(data) => data.clone(),
            None => match dav.fetch(&change.href) {
                Ok((body, _)) => body,
                Err(e) => {
                    errors.push(format!("a contact couldn't be read ({})", say(&e)));
                    continue;
                }
            },
        };
        for card in mailcraft_vcard::parse(&doc) {
            contacts.contacts.push(dav::contact_to_remote(&card, &change.href, &change.etag));
        }
    }
}

/// The changes made here, told to the server: PUTs and DELETEs under their ETags, one outcome
/// for each op tried, in order. An op the run doesn't reach has no outcome and waits, as the
/// engine expects.
fn push(
    dav: &Dav<'_>,
    collections: &[Collection],
    upload: &model_remote::EventsUpload,
    now: Timestamp,
    zone: Zone,
    offset_at: &dyn Fn(Timestamp) -> i32,
    cancel: &AtomicBool,
    result: &mut DavResult,
) -> EventsUploaded {
    let mut uploaded = EventsUploaded { account: upload.account, email: upload.email.clone(), ..Default::default() };
    let main = dav::main_collection(collections).map(|c| c.href.clone());
    for op in upload.ops.iter().take(model_remote::MAX_EVENT_OPS) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match push_one(dav, &main, op, now, zone, offset_at) {
            Ok(outcome) => uploaded.outcomes.push(outcome),
            Err(Stop::Auth) => {
                uploaded.token_refused = true;
                result.auth_refused = true;
                break;
            }
            Err(Stop::Untrusted(fingerprint)) => {
                result.untrusted = Some(fingerprint);
                break;
            }
        }
    }
    uploaded
}

/// Why an upload op ended the whole run early.
enum Stop {
    Auth,
    Untrusted(String),
}

/// One [`EventOp`](model_remote::EventOp) against the server. `Err(Stop)` ends the run; an
/// outcome is always an [`EventOutcome`] with its result.
fn push_one(
    dav: &Dav<'_>,
    main: &Option<String>,
    op: &model_remote::EventOp,
    now: Timestamp,
    zone: Zone,
    offset_at: &dyn Fn(Timestamp) -> i32,
) -> Result<EventOutcome, Stop> {
    let mut outcome = EventOutcome { op: op.clone(), ..Default::default() };
    // One occurrence of a series by its series and its time, and a forwarded meeting, are
    // things plain CalDAV can't say: refused in words, for good.
    if op.of_occurrence() {
        outcome.result = EventResult::Refused;
        outcome.error = "changing one occurrence of a series isn't supported yet".into();
        return Ok(outcome);
    }
    if op.kind == EventChangeKind::Forward {
        outcome.result = EventResult::Refused;
        outcome.error = "forwarding a meeting isn't supported yet".into();
        return Ok(outcome);
    }
    let href = resource_of(op, main);
    match op.kind {
        EventChangeKind::Create => create(dav, &href, op, now, zone, offset_at, &mut outcome)?,
        EventChangeKind::Update | EventChangeKind::Respond => update(dav, &href, op, now, zone, offset_at, &mut outcome)?,
        EventChangeKind::Delete => delete(dav, &href, &mut outcome)?,
        EventChangeKind::Forward => {
            outcome.result = EventResult::Refused;
            outcome.error = "forwarding a meeting isn't supported yet".into();
        }
    }
    Ok(outcome)
}

/// Where an op's resource lives: a change or a deletion names it (`remote`, the `origin.id` the
/// download gave it — the resource's address); a new event goes into the calendar its op names
/// (empty: the main one), as `<collection>/<uid>.ics`.
fn resource_of(op: &model_remote::EventOp, main: &Option<String>) -> String {
    if !op.remote.is_empty() {
        return op.remote.clone();
    }
    let collection =
        if op.calendar.is_empty() || Some(op.calendar.as_str()) == main.as_deref() { main.clone().unwrap_or_default() } else { op.calendar.clone() };
    let base = collection.trim_end_matches('/');
    format!("{base}/{}.ics", uid_of(op))
}

/// The iCalendar `UID` a new event is written with: its mark when it has one, else one made of
/// the account and the event — never empty, and only characters a `UID` may carry unescaped.
fn uid_of(op: &model_remote::EventOp) -> String {
    let source = if op.token.is_empty() { format!("{}-{}", op.event, op.remote) } else { op.token.clone() };
    let uid: String = source.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' }).take(120).collect();
    if uid.is_empty() { "event".to_string() } else { uid }
}

/// The event an op describes as one `.ics` document: the fields laid over `base` (the copy the
/// server has, for a change; a new one for a create), written in the shown zone.
fn document_of(op: &model_remote::EventOp, base: Option<Event>, now: Timestamp, zone: Zone, offset_at: &dyn Fn(Timestamp) -> i32) -> (Event, String) {
    let mut event = base.unwrap_or_default();
    if event.uid.is_empty() {
        event.uid = uid_of(op);
    }
    remote::put_fields(&mut event, &op.fields, offset_at);
    let text = mailcraft_ical::write_in(&[&event], &[], &mailcraft_ical::WriteOptions { method: String::new(), now }, Some(zone));
    (event, text)
}

/// A new event: PUT `If-None-Match: *`. A 412 means the address is taken (a uid that came
/// round again): what is there is written over, guarded by its tag.
fn create(
    dav: &Dav<'_>,
    href: &str,
    op: &model_remote::EventOp,
    now: Timestamp,
    zone: Zone,
    offset_at: &dyn Fn(Timestamp) -> i32,
    outcome: &mut EventOutcome,
) -> Result<(), Stop> {
    let (event, text) = document_of(op, None, now, zone, offset_at);
    match dav.put(href, ICS_TYPE, text.as_bytes(), None) {
        Ok(etag) => {
            outcome.result = EventResult::Done;
            outcome.remote = href.to_string();
            outcome.change_key = etag;
            outcome.uid = event.uid.clone();
        }
        Err(DavError::Status(412)) => match dav.fetch(href) {
            Ok((doc, tag)) => {
                let file = mailcraft_ical::parse_events(&doc, offset_at, 1);
                let base = file.events.into_iter().next();
                let (event, text) = document_of(op, base, now, zone, offset_at);
                match dav.put(href, ICS_TYPE, text.as_bytes(), (!tag.is_empty()).then_some(tag.as_str())) {
                    Ok(etag) => {
                        outcome.result = EventResult::Done;
                        outcome.remote = href.to_string();
                        outcome.change_key = etag;
                        outcome.uid = event.uid;
                    }
                    Err(e) => fail_typed(outcome, &e)?,
                }
            }
            Err(e) => fail_typed(outcome, &e)?,
        },
        Err(e) => fail_typed(outcome, &e)?,
    }
    Ok(())
}

/// A change or an answer to an event the server has: fetch it (its ETag is the guard), lay the
/// fields over it, PUT it back. A 412 means it moved since: fetched and tried again, twice at
/// most; then it is a failure to try later.
fn update(
    dav: &Dav<'_>,
    href: &str,
    op: &model_remote::EventOp,
    now: Timestamp,
    zone: Zone,
    offset_at: &dyn Fn(Timestamp) -> i32,
    outcome: &mut EventOutcome,
) -> Result<(), Stop> {
    for _ in 0..2 {
        let (doc, tag) = match dav.fetch(href) {
            Ok(got) => got,
            Err(DavError::Status(404 | 410)) => {
                // The server no longer has it: a change has nothing to change.
                outcome.result = EventResult::Gone;
                outcome.remote = href.to_string();
                return Ok(());
            }
            Err(e) => {
                fail_typed(outcome, &e)?;
                return Ok(());
            }
        };
        let file = mailcraft_ical::parse_events(&doc, offset_at, 1);
        let Some(base) = file.events.into_iter().next() else {
            outcome.result = EventResult::Failed;
            outcome.error = "the event's document couldn't be read".into();
            return Ok(());
        };
        let (event, text) = document_of(op, Some(base), now, zone, offset_at);
        match dav.put(href, ICS_TYPE, text.as_bytes(), (!tag.is_empty()).then_some(tag.as_str())) {
            Ok(etag) => {
                outcome.result = EventResult::Done;
                outcome.remote = href.to_string();
                outcome.change_key = etag;
                outcome.uid = event.uid;
                return Ok(());
            }
            Err(DavError::Status(412)) => continue,
            Err(e) => {
                fail_typed(outcome, &e)?;
                return Ok(());
            }
        }
    }
    outcome.result = EventResult::Failed;
    outcome.error = say(&DavError::Status(412)).to_string();
    Ok(())
}

/// A deletion: the resource's tag is fetched first so the DELETE is guarded; one that is already
/// gone is `Gone`, as the engine reads it.
fn delete(dav: &Dav<'_>, href: &str, outcome: &mut EventOutcome) -> Result<(), Stop> {
    match dav.fetch(href) {
        Ok((_, tag)) => match dav.delete(href, (!tag.is_empty()).then_some(tag.as_str())) {
            Ok(()) => {
                outcome.result = EventResult::Done;
                outcome.remote = href.to_string();
            }
            Err(DavError::Status(404 | 410)) => {
                outcome.result = EventResult::Gone;
                outcome.remote = href.to_string();
            }
            Err(e) => fail_typed(outcome, &e)?,
        },
        Err(DavError::Status(404 | 410)) => {
            outcome.result = EventResult::Gone;
            outcome.remote = href.to_string();
        }
        Err(e) => fail_typed(outcome, &e)?,
    }
    Ok(())
}

/// An exchange that didn't go through, as the outcome says it: the run stops on a refused
/// sign-in or a certificate to ask about, else the op is a failure to try again later.
fn fail_typed(outcome: &mut EventOutcome, error: &DavError) -> Result<(), Stop> {
    match error {
        DavError::Status(401 | 403) => Err(Stop::Auth),
        DavError::Net(net) => {
            if let Some(CertificateProblem::NotTrusted(info) | CertificateProblem::Changed(info)) = https::certificate_problem(net) {
                return Err(Stop::Untrusted(info.fingerprint.clone()));
            }
            fail_outcome(outcome, error);
            Ok(())
        }
        _ => {
            fail_outcome(outcome, error);
            Ok(())
        }
    }
}

/// The outcome of an op that couldn't be done, in a few words.
fn fail_outcome(outcome: &mut EventOutcome, error: &DavError) {
    outcome.result = match error {
        DavError::Status(404 | 410) => EventResult::Gone,
        DavError::Status(400..=499) => EventResult::Refused,
        _ => EventResult::Failed,
    };
    outcome.status = match error {
        DavError::Status(status) => *status,
        _ => 0,
    };
    outcome.error = say(error).to_string();
}
