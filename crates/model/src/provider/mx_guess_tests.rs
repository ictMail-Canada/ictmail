// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;

/// An exchanger that is a usable host name becomes both servers; the note says it is a guess.
#[test]
fn an_exchanger_becomes_the_guessed_servers() {
    let p = guess_mx("  Mail.Example.COM. ").unwrap();
    assert_eq!(p.service, "");
    assert_eq!(p.oauth, None);
    assert_eq!(p.incoming, server("mail.example.com", 993, Security::Tls));
    assert_eq!(p.outgoing, server("mail.example.com", 587, Security::StartTls));
    assert!(!p.note.is_empty());
}

/// Junk exchangers — no dot, a path, an address — aren't host names and guess nothing.
#[test]
fn unusable_exchangers_guess_nothing() {
    for junk in ["", "localhost", "mail", "a/b.example", "a b.example"] {
        assert_eq!(guess_mx(junk), None, "{junk:?}");
    }
}
