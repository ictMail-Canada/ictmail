// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! CalDAV and CardDAV over HTTPS (RFC 4918, RFC 4791, RFC 6352, RFC 6578): finding an account's
//! calendars and address books, and keeping them in step with `sync-collection`.
//!
//! Every call blocks; the engine runs them on a worker thread, as it does [`crate::https`]. The
//! password goes into the `Authorization` header of requests to the configured server alone,
//! is never sent to another origin (a redirect elsewhere ends the exchange), and is never
//! logged or put into an error. Discovery is generic — it asks the server for its principal,
//! its home sets and the collections inside them — so it works with SOGo, Radicale, Baïkal
//! and the rest rather than only one layout. The XML itself is read by [`mailcraft_dav`],
//! which is pure and bounded.
//!
//! Sync-collection (RFC 6578) is used where the server offers it; where it doesn't, the
//! collection is listed whole with a depth-1 PROPFIND and the caller reconciles.

use mailcraft_charset::base64::STANDARD;
use mailcraft_dav::protocol as proto;
use mailcraft_dav::xml::{self, CALDAV, CARDDAV, DAV, Node};
use mailcraft_model::dav::Collection;

use crate::NetError;
use crate::http::Url;
use crate::https::{self, Request, Response};

/// The most answer a DAV exchange reads: the bound the XML reader enforces anyway.
const MAX_ANSWER: usize = xml::MAX_BYTES;
/// Redirects followed inside one exchange; all of them stay on the server the request went to.
const MAX_REDIRECTS: usize = 3;
/// Home sets and collections one discovery follows, so a hostile answer can't make it ask
/// for thousands of URLs.
const MAX_HOMES: usize = 32;
const MAX_COLLECTIONS: usize = 256;
/// Server addresses, as `http::Url` reads them.
const MAX_URL: usize = 2_000;

/// Why a DAV exchange didn't give what was asked. It holds no text a server wrote.
#[derive(Debug)]
pub enum DavError {
    /// The connection, TLS or transport layer failed.
    Net(NetError),
    /// The server answered with a status that isn't a usable DAV answer (401 and 403 are the
    /// credential's, 404 and 410 a thing that isn't there, 412 a changed document, 429 and 5xx
    /// a busy server).
    Status(u16),
    /// A multistatus answer that couldn't be read within its bounds.
    Document,
}

impl std::fmt::Display for DavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DavError::Net(e) => std::fmt::Display::fmt(e, f),
            DavError::Status(status) => write!(f, "the DAV server answered HTTP {status}"),
            DavError::Document => f.write_str("the DAV server's answer couldn't be read"),
        }
    }
}

impl std::error::Error for DavError {}

impl From<NetError> for DavError {
    fn from(e: NetError) -> DavError {
        DavError::Net(e)
    }
}

/// One resource a sync reported: where it lives, its tag, and its document when the server
/// sent it inline. An empty `etag` with no `data` is how a sync-collection answer says the
/// resource is gone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    /// The resource's absolute URL.
    pub href: String,
    /// Its ETag as the server wrote it (quotes kept); empty when the answer carried none.
    pub etag: String,
    /// The document (`calendar-data`/`card-data`), when the server inlined it.
    pub data: Option<String>,
}

/// The answer of one collection's sync: what changed and the token to keep for the next one.
#[derive(Clone, Debug, Default)]
pub struct Synced {
    pub changes: Vec<Change>,
    /// The collection's next sync token. `None` means the server offered no sync-collection:
    /// `changes` is then a whole listing and the caller reconciles by absence.
    pub token: Option<String>,
}

/// A CalDAV/CardDAV account's credentials and address: the DAV root or principal URL
/// (`https://mail.example.com/SOGo/dav/user@example.com/` for SOGo), the sign-in the server
/// takes (the mailbox's own for most hosts), and the SHA-256 fingerprint of a certificate
/// the owner chose to trust, as [`crate::https`] takes it.
#[derive(Clone, Copy)]
pub struct Dav<'a> {
    pub base: &'a str,
    pub user: &'a str,
    pub password: &'a str,
    pub trusted: &'a str,
}

impl Dav<'_> {
    /// The calendars and address books the account has, discovered through its principal and
    /// home sets. Empty when the server offers none — or when it isn't a DAV server at all
    /// (the caller tells a 404/405 from an empty list by trying and reading the error).
    pub fn discover(&self) -> Result<Vec<Collection>, DavError> {
        let mut found: Vec<Collection> = Vec::new();
        let mut homes: Vec<String> = Vec::new();
        // Who is the principal? Ask the base; absent the property, the base is it.
        let reply = self.propfind(self.base, 0, &proto::current_user_principal())?;
        let root = multistatus(&reply)?;
        let principal = proto::answers(&root)
            .iter()
            .find_map(|a| proto::prop(a, DAV, "current-user-principal"))
            .and_then(|n| n.child(DAV, "href"))
            .map(|n| n.text.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.base.to_string());
        let Some(principal) = resolve(self.base, &principal) else {
            return Ok(found);
        };
        // Depth 1 on the principal: its home sets, and on some servers the collections too.
        let reply = self.propfind(&principal, 1, &proto::principal_collections())?;
        let root = multistatus(&reply)?;
        for a in proto::answers(&root) {
            collect(&a, &principal, &mut found, &mut homes);
        }
        // The collections live inside the home sets.
        let mut none: Vec<String> = Vec::new();
        for home in homes {
            let reply = self.propfind(&home, 1, &proto::collections_in_home())?;
            let root = multistatus(&reply)?;
            for a in proto::answers(&root) {
                collect(&a, &home, &mut found, &mut none);
            }
        }
        Ok(found)
    }

    /// Keep one collection in step: everything that changed since `token` (everything, when
    /// `token` is empty), and the token to keep. A token the server no longer honours gets
    /// one fresh start. Where the server has no sync-collection, the whole collection is
    /// listed and `token` comes back `None`.
    pub fn sync(&self, collection: &str, token: &str, contacts: bool) -> Result<Synced, DavError> {
        let data = if contacts { (CARDDAV, "card-data") } else { (CALDAV, "calendar-data") };
        let reply = self.propfind(collection, 0, &proto::collection_state())?;
        let root = multistatus(&reply)?;
        let state = proto::answers(&root).into_iter().next();
        let Some(state) = state else { return Err(DavError::Document) };
        if !proto::supports_sync(&state) {
            return self.listing(collection, data);
        }
        // A stale token is answered 410: ask again with none, once.
        let mut reply = self.report(collection, &proto::sync_collection(token, data))?;
        if reply.status == 410 && !token.is_empty() {
            reply = self.report(collection, &proto::sync_collection("", data))?;
        }
        let root = multistatus(&reply)?;
        let mut changes = Vec::new();
        for c in proto::changes(&root, data.0, data.1) {
            if let Some(href) = resolve(collection, c.href) {
                changes.push(Change { href, etag: c.etag.unwrap_or_default().to_string(), data: c.data.map(str::to_string) });
            }
        }
        // The next token is a property of the collection, read after the changes.
        let reply = self.propfind(collection, 0, &proto::collection_state())?;
        let root = multistatus(&reply)?;
        Ok(Synced { changes, token: proto::sync_token(&root) })
    }

    /// One resource's document (`.ics`/`.vcf`) and its ETag, for servers that don't inline it
    /// in a sync answer and for edits that are guarded by the tag (`put`, `delete`). `url` is
    /// the absolute address a [`Change`] or discovery gave; the tag is empty when the answer
    /// carried none.
    pub fn fetch(&self, url: &str) -> Result<(String, String), DavError> {
        let reply = self.exchange("GET", url, None, None, &[], &[])?;
        if reply.status != 200 {
            return Err(DavError::Status(reply.status));
        }
        let body = String::from_utf8(reply.body).map_err(|_| DavError::Document)?;
        Ok((body, reply.etag.unwrap_or_default()))
    }

    /// Create (`etag` `None`: `If-None-Match: *`) or update (`etag`: `If-Match`) one resource's
    /// document. A 412 means the document changed since its tag was read: fetch it again. The
    /// answer's own ETag comes back when the server gives one (empty when it doesn't): it is
    /// the version just written, for the next guarded change.
    pub fn put(&self, url: &str, content_type: &str, body: &[u8], etag: Option<&str>) -> Result<String, DavError> {
        let extra: Vec<(String, String)> = match etag {
            Some(tag) => vec![("If-Match".to_string(), tag.to_string())],
            None => vec![("If-None-Match".to_string(), "*".to_string())],
        };
        let reply = self.exchange("PUT", url, None, Some(content_type), &extra, body)?;
        if matches!(reply.status, 200 | 201 | 204) { Ok(reply.etag.unwrap_or_default()) } else { Err(DavError::Status(reply.status)) }
    }

    /// Remove one resource, guarded by its tag when one is known (`If-Match`).
    pub fn delete(&self, url: &str, etag: Option<&str>) -> Result<(), DavError> {
        let extra: Vec<(String, String)> = etag.map(|tag| vec![("If-Match".to_string(), tag.to_string())]).unwrap_or_default();
        let reply = self.exchange("DELETE", url, None, None, &extra, &[])?;
        if matches!(reply.status, 200 | 202 | 204) { Ok(()) } else { Err(DavError::Status(reply.status)) }
    }

    /// One exchange with headers already built: Basic credentials, the headers asked for,
    /// and redirects followed only while they stay on the same server.
    fn exchange(
        &self,
        method: &str,
        url: &str,
        depth: Option<u8>,
        content_type: Option<&str>,
        extra: &[(String, String)],
        body: &[u8],
    ) -> Result<Response, DavError> {
        let mut current = checked(url).ok_or(DavError::Document)?;
        let mut reply = self.once(method, &current, depth, content_type, extra, body)?;
        for _ in 0..MAX_REDIRECTS {
            let location = match reply.status {
                301 | 302 | 303 | 307 | 308 => reply.location.clone(),
                _ => None,
            };
            let Some(location) = location else { return Ok(reply) };
            let Some(next) = resolve(&current, &location).filter(|n| same_origin(&current, n)) else {
                return Ok(reply);
            };
            current = next;
            reply = self.once(method, &current, depth, content_type, extra, body)?;
        }
        Ok(reply)
    }

    /// One request, no redirects: the `Authorization` header is the only place the password
    /// goes, and only ever to `url`'s own server.
    fn once(
        &self,
        method: &str,
        url: &str,
        depth: Option<u8>,
        content_type: Option<&str>,
        extra: &[(String, String)],
        body: &[u8],
    ) -> Result<Response, DavError> {
        let auth = format!("Basic {}", STANDARD.encode(format!("{}:{}", self.user, self.password)));
        let mut fields: Vec<(String, String)> = Vec::with_capacity(4 + extra.len());
        fields.push(("Authorization".to_string(), auth));
        if let Some(depth) = depth {
            fields.push(("Depth".to_string(), depth.to_string()));
        }
        if let Some(content_type) = content_type {
            fields.push(("Content-Type".to_string(), content_type.to_string()));
        }
        fields.extend(extra.iter().cloned());
        let refs: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        Ok(https::send(&Request { method, url, fields: &refs, body, max_response: MAX_ANSWER, trusted: self.trusted })?)
    }

    fn propfind(&self, url: &str, depth: u8, body: &str) -> Result<Response, DavError> {
        self.exchange("PROPFIND", url, Some(depth), Some("application/xml; charset=utf-8"), &[], body.as_bytes())
    }

    fn report(&self, url: &str, body: &str) -> Result<Response, DavError> {
        self.exchange("REPORT", url, None, Some("application/xml; charset=utf-8"), &[], body.as_bytes())
    }

    /// The depth-1 fallback for a collection with no sync-collection: every member with its
    /// tag and document.
    fn listing(&self, collection: &str, data: proto::Prop) -> Result<Synced, DavError> {
        let reply = self.propfind(collection, 1, &proto::listing(data))?;
        let root = multistatus(&reply)?;
        let mut changes = Vec::new();
        for a in proto::answers(&root) {
            let Some(href) = resolve(collection, a.href) else { continue };
            let document = proto::prop(&a, data.0, data.1).map(proto::text);
            let tag = proto::prop(&a, DAV, "getetag").map(proto::text);
            // The collection answers for itself too; a member is what carries a document or,
            // failing that, a tag at another address.
            if document.is_none() && (tag.is_none() || href == collection) {
                continue;
            }
            changes.push(Change { href, etag: tag.unwrap_or_default().to_string(), data: document.map(str::to_string) });
        }
        Ok(Synced { changes, token: None })
    }
}

/// One answer of a propfind on `against`: a collection to keep, or a home set to look in.
fn collect(a: &proto::Answer<'_>, against: &str, found: &mut Vec<Collection>, homes: &mut Vec<String>) {
    let kind = proto::kind(a);
    if kind.calendar || kind.addressbook {
        let Some(href) = resolve(against, a.href) else { return };
        if found.len() >= MAX_COLLECTIONS || found.iter().any(|c| c.href == href) {
            return;
        }
        found.push(Collection { account: 0, href: href.clone(), name: name_of(a, &href), contacts: kind.addressbook, read_only: !can_write(a) });
        return;
    }
    for (ns, prop_name) in [(CALDAV, "calendar-home-set"), (CARDDAV, "addressbook-home-set")] {
        for h in proto::home_sets(a, ns, prop_name) {
            if let Some(url) = resolve(against, h)
                && homes.len() < MAX_HOMES
                && !homes.contains(&url)
            {
                homes.push(url);
            }
        }
    }
}

/// The multistatus of a 207 answer; any other status is the server's refusal.
fn multistatus(reply: &Response) -> Result<Node, DavError> {
    if reply.status != 207 {
        return Err(DavError::Status(reply.status));
    }
    xml::parse(&reply.body).map_err(|_| DavError::Document)
}

/// A URL that parses as the `http` layer reads one: https (or loopback http for tests),
/// bounded, no credentials inside.
fn checked(url: &str) -> Option<String> {
    Url::parse(url).ok().map(|_| url.to_string())
}

/// Whether `a` and `b` name the same server, spoken to the same way: credentials may follow a
/// redirect only there.
fn same_origin(a: &str, b: &str) -> bool {
    let port = |u: &Url| match (u.port, u.secure) {
        (0, true) => 443,
        (0, false) => 80,
        (port, _) => port,
    };
    match (Url::parse(a), Url::parse(b)) {
        (Ok(x), Ok(y)) => x.secure == y.secure && x.host.eq_ignore_ascii_case(&y.host) && port(&x) == port(&y),
        _ => false,
    }
}

/// Join a `DAV:href` against the URL it was answered to: an absolute address stays, `/path`
/// keeps the origin, anything else merges under the base's directory with `.` and `..` folded.
/// `None` for what can't be a DAV address (another scheme's reference, a query, a fragment).
fn resolve(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.len() > MAX_URL || href.contains(['?', '#']) {
        return None;
    }
    if href.starts_with("https://") || href.starts_with("http://") {
        return checked(href);
    }
    if href.starts_with("//") {
        return None;
    }
    let scheme_end = base.find("://")? + 3;
    let path_at = base[scheme_end..].find('/').map(|i| scheme_end + i).unwrap_or(base.len());
    let origin = &base[..path_at];
    let merged = if href.starts_with('/') {
        format!("{origin}{href}")
    } else {
        match base.rfind('/').filter(|&i| i >= path_at) {
            Some(i) => format!("{}/{href}", &base[..i]),
            None => format!("{origin}/{href}"),
        }
    };
    normalize(&merged)
}

/// Fold `.` and `..` out of a URL's path, keeping its authority and any trailing slash.
fn normalize(url: &str) -> Option<String> {
    let scheme_end = url.find("://")? + 3;
    let path_at = url[scheme_end..].find('/').map(|i| scheme_end + i).unwrap_or(url.len());
    let (head, path) = url.split_at(path_at);
    let trailing = path.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let mut out = format!("{head}/{}", parts.join("/"));
    if trailing && !out.ends_with('/') {
        out.push('/');
    }
    checked(&out)
}

/// The collection's name: its `displayname`, else the last path segment less a document
/// suffix (`personal.ics` reads `personal`).
fn name_of(a: &proto::Answer<'_>, href: &str) -> String {
    if let Some(name) = proto::prop(a, DAV, "displayname").map(proto::text).filter(|s| !s.is_empty()) {
        return name.to_string();
    }
    let segment = href.rsplit('/').find(|s| !s.is_empty()).unwrap_or(href);
    segment.trim_end_matches(".ics").trim_end_matches(".vcf").to_string()
}

/// Whether the answer's privilege set lets the sign-in write: `write`, `write-content`,
/// `bind` or `all` under `current-user-privilege-set`.
fn can_write(a: &proto::Answer<'_>) -> bool {
    let Some(node) = proto::prop(a, DAV, "current-user-privilege-set") else { return false };
    grants(node)
}

fn grants(node: &Node) -> bool {
    node.children.iter().any(|c| c.is(DAV, "write") || c.is(DAV, "write-content") || c.is(DAV, "all") || c.is(DAV, "bind") || grants(c))
}

#[cfg(test)]
#[path = "dav_tests.rs"]
mod tests;
