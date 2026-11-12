// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! CalDAV/CardDAV protocol documents: the bodies of the PROPFIND and sync-collection REPORT
//! requests, and readers for their multistatus answers (RFC 4918, RFC 4791, RFC 6352, RFC 6578).
//!
//! Nothing here opens a socket or reads a file: the caller performs each HTTP exchange and hands
//! the answer's bytes to the readers. Every reader is bounded through [`crate::xml`].
use crate::xml::{CALDAV, CARDDAV, DAV, Node};

/// The names a PROPFIND can ask for, as `(namespace, local name)` pairs. Only the three
/// namespaces the request template declares may be used.
pub type Prop = (&'static str, &'static str);

fn propfind(props: &[Prop]) -> String {
    let mut body = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><d:propfind xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\" xmlns:card=\"urn:ietf:params:xml:ns:carddav\"><d:prop>",
    );
    for (ns, name) in props {
        body.push('<');
        body.push_str(prefix(ns));
        body.push(':');
        body.push_str(name);
        body.push_str("/>");
    }
    body.push_str("</d:prop></d:propfind>");
    body
}

fn prefix(ns: &str) -> &'static str {
    match ns {
        DAV => "d",
        CALDAV => "c",
        CARDDAV => "card",
        _ => "d",
    }
}

/// Depth-0 PROPFIND on a principal URL: which principal it is.
pub fn current_user_principal() -> String {
    propfind(&[(DAV, "current-user-principal")])
}

/// Depth-1 PROPFIND on a principal URL: the home sets and what collections it holds.
pub fn principal_collections() -> String {
    propfind(&[
        (DAV, "resourcetype"),
        (DAV, "displayname"),
        (CALDAV, "calendar-home-set"),
        (CARDDAV, "addressbook-home-set"),
        (DAV, "current-user-privilege-set"),
    ])
}

/// Depth-1 PROPFIND on a home set: the collections inside it, with what each is for.
pub fn collections_in_home() -> String {
    propfind(&[(DAV, "resourcetype"), (DAV, "displayname"), (DAV, "supported-report-set"), (DAV, "current-user-privilege-set")])
}

/// Depth-0 PROPFIND on one collection: its sync token, tag and what it supports.
pub fn collection_state() -> String {
    propfind(&[
        (DAV, "resourcetype"),
        (DAV, "sync-token"),
        (DAV, "supported-report-set"),
        (DAV, "current-user-privilege-set"),
        (CALDAV, "calendar-description"),
        (CARDDAV, "addressbook-description"),
    ])
}

/// Depth-1 PROPFIND on a collection for a server without sync-collection: every member with
/// its tag and document, so the caller can reconcile a whole listing.
pub fn listing(data: Prop) -> String {
    propfind(&[(DAV, "resourcetype"), (DAV, "displayname"), (DAV, "getetag"), data])
}

/// A sync-collection REPORT (RFC 6578) for one collection: everything changed since `token`,
/// or everything when `token` is empty. `data` is the property holding each resource's document:
/// `(CALDAV, "calendar-data")` or `(CARDDAV, "card-data")`.
pub fn sync_collection(token: &str, data: Prop) -> String {
    let mut body = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>");
    body.push_str("<d:sync-collection xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\" xmlns:card=\"urn:ietf:params:xml:ns:carddav\">");
    if !token.is_empty() {
        body.push_str("<d:sync-token>");
        body.push_str(&escape(token));
        body.push_str("</d:sync-token>");
    }
    body.push_str("<d:sync-level>1</d:sync-level><d:prop><d:getetag/>");
    body.push('<');
    body.push_str(prefix(data.0));
    body.push(':');
    body.push_str(data.1);
    body.push_str("/></d:prop></d:sync-collection>");
    body
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// One `response` element of a multistatus answer: its href and the property elements a
/// successful propstat returned, as parsed nodes (structured properties keep their children).
pub struct Answer<'a> {
    /// The raw text of `DAV:href`; resolve it against the request URL before use.
    pub href: &'a str,
    /// Every property node from the 200 propstats. A property asked for but not returned, or
    /// returned only by a failed propstat, is absent.
    pub props: Vec<&'a Node>,
}

/// The `response` elements of a multistatus document.
pub fn answers(root: &Node) -> Vec<Answer<'_>> {
    let mut out = Vec::new();
    for response in root.children.iter().filter(|n| n.is(DAV, "response")) {
        let href = response.child(DAV, "href").map(|n| n.text.trim()).unwrap_or_default();
        let mut props = Vec::new();
        for propstat in response.children.iter().filter(|n| n.is(DAV, "propstat")) {
            let ok = propstat.value(DAV, "status").is_some_and(|s| s.split_whitespace().nth(1) == Some("200"));
            if !ok {
                continue;
            }
            if let Some(prop) = propstat.child(DAV, "prop") {
                props.extend(prop.children.iter());
            }
        }
        out.push(Answer { href, props });
    }
    out
}

/// The first property node `answer` carries with this namespace and name.
pub fn prop<'a>(answer: &Answer<'a>, ns: &str, name: &str) -> Option<&'a Node> {
    answer.props.iter().copied().find(|p| p.is(ns, name))
}

/// A property's text, trimmed; empty for a property that only wraps children.
pub fn text(node: &Node) -> &str {
    node.text.trim()
}

/// What a `resourcetype` property says its resource is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kind {
    pub principal: bool,
    pub collection: bool,
    pub calendar: bool,
    pub addressbook: bool,
}

/// Read a `resourcetype` property node (or the answer's `resourcetype` prop) into a [`Kind`].
pub fn kind(answer: &Answer<'_>) -> Kind {
    let mut kind = Kind::default();
    let Some(node) = prop(answer, DAV, "resourcetype") else { return kind };
    for child in &node.children {
        if child.is(DAV, "principal") {
            kind.principal = true;
        } else if child.is(DAV, "collection") {
            kind.collection = true;
        } else if child.is(CALDAV, "calendar") {
            kind.calendar = true;
        } else if child.is(CARDDAV, "addressbook") {
            kind.addressbook = true;
        }
    }
    kind
}

/// The hrefs a home-set property wraps (`calendar-home-set`, `addressbook-home-set`).
pub fn home_sets<'a>(answer: &Answer<'a>, ns: &str, name: &str) -> Vec<&'a str> {
    let Some(node) = prop(answer, ns, name) else { return Vec::new() };
    node.children.iter().filter(|n| n.is(DAV, "href")).map(|n| n.text.trim()).collect()
}

/// Whether the answer's `supported-report-set` offers the `sync-collection` report (RFC 6578).
pub fn supports_sync(answer: &Answer<'_>) -> bool {
    let Some(node) = prop(answer, DAV, "supported-report-set") else { return false };
    node.children
        .iter()
        .filter(|n| n.is(DAV, "supported-report"))
        .any(|report| report.value(DAV, "report-name").is_some_and(|name| name.trim() == "sync-collection"))
}

/// One changed resource from a sync-collection answer: where it lives, its tag, and its
/// document when the server sent it inline. `etag` is `None` for a resource the server says
/// is gone — in a sync-level-1 answer that means deleted.
pub struct Change<'a> {
    pub href: &'a str,
    pub etag: Option<&'a str>,
    /// The inline `calendar-data` or `card-data` text, XML-unescaped by the parser already.
    pub data: Option<&'a str>,
}

/// The changed resources of a sync-collection REPORT answer.
pub fn changes<'a>(root: &'a Node, data_ns: &str, data_name: &str) -> Vec<Change<'a>> {
    answers(root)
        .into_iter()
        .map(|a| Change {
            href: a.href,
            etag: prop(&a, DAV, "getetag").map(text).filter(|s| !s.is_empty()),
            data: prop(&a, data_ns, data_name).map(|n| n.text.as_str()).filter(|s| !s.is_empty()),
        })
        .collect()
}

/// The collection's next sync token, from a depth-0 PROPFIND on the collection itself.
pub fn sync_token(root: &Node) -> Option<String> {
    answers(root).iter().find_map(|a| prop(a, DAV, "sync-token").map(text).filter(|s| !s.is_empty()).map(str::to_string))
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
