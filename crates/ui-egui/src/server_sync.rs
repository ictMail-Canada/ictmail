// SPDX-FileCopyrightText: MailCraft contributors
ctMail/g
// SPDX-License-Identifier: MIT OR Apache-2.0
ctMail/g
//! One question for everything an older sign-in of a Microsoft account doesn't cover: keeping its
ctMail/g
//! to-dos ([`crate::task_sync`]), the rules of its server ([`crate::rule_sync`]), its automatic
ctMail/g
//! replies ([`crate::auto_replies`]) and its list of categories ([`crate::category_sync`]) in step
ctMail/g
//! with Outlook (#661), and its shared mailboxes and the calendars shared with its owner
ctMail/g
//! ([`crate::shared_permission`], #70).
ctMail/g
//!
ctMail/g
//! A new sign-in's page asks for what all of those take (`Tasks.ReadWrite`,
ctMail/g
//! `MailboxSettings.ReadWrite`, `Mail.ReadWrite.Shared`, `Mail.Send.Shared`,
ctMail/g
//! `Calendars.ReadWrite.Shared`) beside the rest, and none of them is asked for by itself. An
ctMail/g
//! account that signed in before is asked once, by the window, when the first of them finds its
ctMail/g
//! sign-in doesn't cover it: one question, then one page of Microsoft's for all of it
ctMail/g
//! (`ui.allowSync`, `mailcraft_net::oauth::sign_in_for_sync`; Microsoft shows only what wasn't
ctMail/g
//! given yet). A question that goes away unanswered is not put again by itself in this session;
ctMail/g
//! the owner's own doing (a send from a shared mailbox, a calendar opened by address) puts it
ctMail/g
//! again. File › Info has one line per account, with Ask again.
ctMail/g

ctMail/g
use mailcraft_model::Id;
ctMail/g
use serde_json::{Value, json};
ctMail/g

ctMail/g
use crate::dialogs::{Dialog, Question};
ctMail/g
use crate::{MailApp, Tokens};
ctMail/g

ctMail/g
/// Where an account's permission stands, for one of the four.
ctMail/g
#[derive(Clone, Debug, PartialEq, Eq)]
ctMail/g
pub(crate) enum Perm {
ctMail/g
    /// Allowed, or nothing is known against it.
ctMail/g
    Fine,
ctMail/g
    /// Waiting for the owner.
ctMail/g
    Waits,
ctMail/g
    /// The owner is being asked.
ctMail/g
    Asking,
ctMail/g
}
ctMail/g

ctMail/g
/// How much of a refusal's reason File › Info and the Categories window show: the reasons the one
ctMail/g
/// question gives are whole in every language (the longest is about 160 characters), and a
ctMail/g
/// server's own words are cut after this many.
ctMail/g
pub(crate) const REASON_SHOWN: usize = 200;
ctMail/g

ctMail/g
impl MailApp {
ctMail/g
    /// The answers of the to-dos', rules', calendar's and categories' jobs, taken in at `now`: a
ctMail/g
    /// job that came to nothing waits from when its answer is read, even after the window went
ctMail/g
    /// a long time without a frame (#661).
ctMail/g
    pub fn take_in_sync_answers(&mut self, now: f64) {
ctMail/g
        self.session.driver.calendar.clock = now;
ctMail/g
        self.session.driver.tasks.clock = now;
ctMail/g
        self.session.driver.rules.clock = now;
ctMail/g
        self.category_sync.clock = now;
ctMail/g
        self.poll_calendar();
ctMail/g
        self.poll_tasks();
ctMail/g
        self.poll_rules();
ctMail/g
        self.poll_categories();
ctMail/g
    }
ctMail/g

ctMail/g
    /// The one question is on screen, or its page is open in the browser.
ctMail/g
    pub(crate) fn sync_question_open(&self) -> bool {
ctMail/g
        self.signing_in.is_some() || matches!(&self.dialog, Some(Dialog::Confirm { command, .. }) if command == "ui.allowSync")
ctMail/g
    }
ctMail/g

ctMail/g
    /// Put the one question to the owner of `account`, when one of the four found its sign-in
ctMail/g
    /// doesn't cover it. Not again by itself once it went away unanswered in this session (the
ctMail/g
    /// one that asked takes that as a no). Returns whether it asked.
ctMail/g
    pub(crate) fn ask_sync(&mut self, account: Id) -> bool {
ctMail/g
        if self.sync_declined.contains(&account) {
ctMail/g
            self.sync_not_now(account);
ctMail/g
            return false;
ctMail/g
        }
ctMail/g
        self.put_sync_question(account)
ctMail/g
    }
ctMail/g

ctMail/g
    /// File › Info's Ask again, and the Categories window's Allow: the question, whatever was
ctMail/g
    /// answered before.
ctMail/g
    pub(crate) fn ask_sync_again(&mut self, account: Id) -> bool {
ctMail/g
        if self.dialog.is_some() || self.signing_in.is_some() {
ctMail/g
            return false;
ctMail/g
        }
ctMail/g
        self.sync_declined.remove(&account);
ctMail/g
        self.shared_page_given.remove(&account);
ctMail/g
        self.put_sync_question(account)
ctMail/g
    }
ctMail/g

ctMail/g
    /// Whether keeping `account` in step with Outlook covers its rules and its categories too: an
ctMail/g
    /// account kept in step through Microsoft Graph. A Microsoft account still on IMAP keeps only its
ctMail/g
    /// to-dos and its automatic replies there, and the question and the texts say only those.
ctMail/g
    pub(crate) fn sync_covers_lists(&self, account: Id) -> bool {
ctMail/g
        self.session.mailbox.keeps_rules(account)
ctMail/g
    }
ctMail/g

ctMail/g
    /// Whether `account`'s to-dos are kept in step with Microsoft To Do already (their permission
ctMail/g
    /// was given by itself, as an older version asked it): the question and its texts then say
ctMail/g
    /// nothing of them.
ctMail/g
    fn sync_tasks_kept(&self, account: Id) -> bool {
ctMail/g
        self.session.driver.tasks.perm.get(&account) == Some(&crate::task_sync::Permission::Given)
ctMail/g
    }
ctMail/g

ctMail/g
    /// Whether `account`'s sign-in lets ictMail change its mailbox settings already (its rules,
ctMail/g
    /// automatic replies or categories were changed with it, as an older version asked it by
ctMail/g
    /// itself) while its to-dos wait: the question and its texts then say only the to-dos.
ctMail/g
    fn sync_only_tasks(&self, account: Id) -> bool {
ctMail/g
        let settings = matches!(self.session.driver.rules.perm.get(&account), Some(crate::rule_sync::Permission::Given))
ctMail/g
            || matches!(self.session.driver.auto_replies.perm.get(&account), Some(crate::auto_replies::Permission::Given))
ctMail/g
            || matches!(self.category_sync.perm.get(&account), Some(crate::task_sync::Permission::Given));
ctMail/g
        settings && !self.sync_tasks_kept(account)
ctMail/g
    }
ctMail/g

ctMail/g
    /// Whether one of the four waits for the one question (the shared mailboxes' part aside).
ctMail/g
    fn sync_four_wait(&self, account: Id) -> bool {
ctMail/g
        [self.tasks_sync_part(account), self.rules_sync_part(account), self.auto_replies_sync_part(account), self.categories_sync_part(account)]
ctMail/g
            .into_iter()
ctMail/g
            .flatten()
ctMail/g
            .any(|(.., perm)| perm != Perm::Fine)
ctMail/g
    }
ctMail/g

ctMail/g
    /// Whether only the shared mailboxes and the calendars shared with the owner wait: the
ctMail/g
    /// question and its texts then say only those.
ctMail/g
    fn sync_only_shared(&self, account: Id) -> bool {
ctMail/g
        !self.sync_four_wait(account) && self.shared_sync_waits(account)
ctMail/g
    }
ctMail/g

ctMail/g
    /// File › Info's tip for Ask again, saying only what `account` keeps in step.
ctMail/g
    pub(crate) fn sync_tip(&self, account: Id) -> &'static str {
ctMail/g
        if self.sync_only_shared(account) {
ctMail/g
            return tl!(
ctMail/g
                "Ask for permission to read and send from shared mailboxes and to read the calendars shared with you: Microsoft's page opens in your browser. Without it, nothing of them is read or sent."
ctMail/g
            );
ctMail/g
        }
ctMail/g
        if self.sync_only_tasks(account) {
ctMail/g
            return tl!(
ctMail/g
                "Ask for permission to read and change your To Do lists and tasks: Microsoft's page opens in your browser. Without it, tasks stay on this computer."
ctMail/g
            );
ctMail/g
        }
ctMail/g
        match (self.sync_covers_lists(account), self.sync_tasks_kept(account)) {
ctMail/g
            (true, false) => tl!(
ctMail/g
                "Ask for permission to keep your tasks, rules, automatic replies and categories in step with Outlook: Microsoft's page opens in your browser. Without it, tasks and rules stay on this computer and the categories can't be changed here."
ctMail/g
            ),
ctMail/g
            (true, true) => tl!(
ctMail/g
                "Ask for permission to keep your rules, automatic replies and categories in step with Outlook: Microsoft's page opens in your browser. Without it, rules stay on this computer and the categories can't be changed here."
ctMail/g
            ),
ctMail/g
            (false, false) => tl!(
ctMail/g
                "Ask for permission to keep your tasks and automatic replies in step with Outlook: Microsoft's page opens in your browser. Without it, tasks stay on this computer."
ctMail/g
            ),
ctMail/g
            (false, true) => tl!(
ctMail/g
                "Ask for permission to keep your automatic replies in step with Outlook: Microsoft's page opens in your browser. Without it, your automatic replies aren't changed."
ctMail/g
            ),
ctMail/g
        }
ctMail/g
    }
ctMail/g

ctMail/g
    fn put_sync_question(&mut self, account: Id) -> bool {
ctMail/g
        let Some(address) = self.session.mailbox.account(account).map(|a| a.email.trim().to_string()) else { return false };
ctMail/g
        let text = match (self.sync_covers_lists(account), self.sync_tasks_kept(account)) {
ctMail/g
            _ if self.sync_only_shared(account) => tl!(
ctMail/g
                "To read and send from the shared mailboxes you open with {address}, and to show the calendars shared with you, ictMail needs your permission. Microsoft's page opens in your browser to ask you, once, for everything ictMail uses that you haven't allowed yet.\n\nUntil you allow it, nothing of those mailboxes and calendars is read, mail and meeting answers from a shared mailbox wait on this computer, and nothing is sent from your own address in their place."
ctMail/g
            ),
ctMail/g
            _ if self.sync_only_tasks(account) => tl!(
ctMail/g
                "To keep the tasks of {address} in step with Microsoft To Do, ictMail needs your permission to read and change your To Do lists and tasks. Microsoft's page opens in your browser to ask you, once.\n\nUntil you allow it, tasks stay on this computer and nothing of them is sent to Microsoft."
ctMail/g
            ),
ctMail/g
            (true, false) => tl!(
ctMail/g
                "To keep the tasks, rules, automatic replies and categories of {address} in step with Outlook, ictMail needs your permission to change your To Do lists and your mailbox settings. Microsoft's page opens in your browser to ask you, once.\n\nUntil you allow it, tasks and rules stay on this computer and nothing of them is sent to Microsoft, your automatic replies aren't changed, and the categories of this mailbox can't be changed here."
ctMail/g
            ),
ctMail/g
            (true, true) => tl!(
ctMail/g
                "To keep the rules, automatic replies and categories of {address} in step with Outlook, ictMail needs your permission to change your mailbox settings. Microsoft's page opens in your browser to ask you, once.\n\nUntil you allow it, rules stay on this computer and nothing of them is sent to Microsoft, your automatic replies aren't changed, and the categories of this mailbox can't be changed here."
ctMail/g
            ),
ctMail/g
            (false, false) => tl!(
ctMail/g
                "To keep the tasks and automatic replies of {address} in step with Outlook, ictMail needs your permission to change your To Do lists and your mailbox settings. Microsoft's page opens in your browser to ask you, once.\n\nUntil you allow it, tasks stay on this computer and nothing of them is sent to Microsoft, and your automatic replies aren't changed."
ctMail/g
            ),
ctMail/g
            (false, true) => tl!(
ctMail/g
                "To keep the automatic replies of {address} in step with Outlook, ictMail needs your permission to change your mailbox settings. Microsoft's page opens in your browser to ask you, once.\n\nUntil you allow it, your automatic replies aren't changed."
ctMail/g
            ),
ctMail/g
        };
ctMail/g
        let mut text = crate::i18n::fmt(text, &[("address", &address)]);
ctMail/g
        // (What else the page asks for: the shared mailboxes and calendars, for an account that
ctMail/g
        // may have them, so that nothing is asked again for them later.)
ctMail/g
        if !self.sync_only_shared(account) && self.shared_on_page(account) {
ctMail/g
            text.push_str("\n\n");
ctMail/g
            text.push_str(tl!(
ctMail/g
                "The same page asks to read and send from the shared mailboxes you have access to and to read the calendars shared with you, so that ictMail doesn't have to ask again."
ctMail/g
            ));
ctMail/g
        }
ctMail/g
        let title = if self.sync_only_shared(account) { tl!("Shared Mailboxes and Calendars") } else { tl!("Keep in Step with Outlook") };
ctMail/g
        let ask = Question {
ctMail/g
            title: title.to_string(),
ctMail/g
            text,
ctMail/g
            action: tl!("Continue").to_string(),
ctMail/g
            with: Value::Null,
ctMail/g
            more: Vec::new(),
ctMail/g
            note: None,
ctMail/g
            cancel: None,
ctMail/g
            tick: None,
ctMail/g
        };
ctMail/g
        self.sync_mark_asking(account);
ctMail/g
        self.dialog = Some(Dialog::Confirm { command: "ui.allowSync".to_string(), params: json!({"account": account}), ask });
ctMail/g
        true
ctMail/g
    }
ctMail/g

ctMail/g
    /// What waits for the owner is being asked.
ctMail/g
    fn sync_mark_asking(&mut self, account: Id) {
ctMail/g
        self.sync_asking.insert(account);
ctMail/g
        self.tasks_sync_asking(account);
ctMail/g
        self.rules_sync_asking(account);
ctMail/g
        self.auto_replies_sync_asking(account);
ctMail/g
        self.categories_sync_asking(account);
ctMail/g
        self.shared_sync_asking(account);
ctMail/g
    }
ctMail/g

ctMail/g
    /// The question went away unanswered, or was put before: what waits takes it as a no.
ctMail/g
    pub(crate) fn sync_not_now(&mut self, account: Id) {
ctMail/g
        self.sync_declined.insert(account);
ctMail/g
        self.tasks_sync_not_now(account);
ctMail/g
        self.rules_sync_not_now(account);
ctMail/g
        self.auto_replies_sync_not_now(account);
ctMail/g
        self.categories_sync_not_now(account);
ctMail/g
        self.shared_sync_not_now(account);
ctMail/g
    }
ctMail/g

ctMail/g
    /// A question that went away unanswered is a no for this session. Called every frame.
ctMail/g
    pub(crate) fn sync_ask_tick(&mut self) {
ctMail/g
        if self.sync_question_open() {
ctMail/g
            return;
ctMail/g
        }
ctMail/g
        let asking: Vec<Id> = self.sync_asking.drain().collect();
ctMail/g
        for account in asking {
ctMail/g
            self.sync_not_now(account);
ctMail/g
        }
ctMail/g
    }
ctMail/g

ctMail/g
    /// `ui.allowSync` (and the older ids that ask for one part of it): Microsoft's page, for all
ctMail/g
    /// of it, because the owner said go ahead (or asked for a shared mailbox to be read).
ctMail/g
    pub(crate) fn begin_sync_sign_in(&mut self, account: Id) -> Result<Value, String> {
ctMail/g
        let owner = self.session.mailbox.account(account).filter(|a| a.oauth.is_some() && !a.is_shared()).cloned();
ctMail/g
        let owner = owner.ok_or_else(|| tl!("This account has no sign-in with Microsoft").to_string())?;
ctMail/g
        let oauth = owner.oauth.clone().filter(|o| o.provider == mailcraft_model::OAuthProvider::Microsoft);
ctMail/g
        let oauth = oauth.ok_or_else(|| tl!("This account has no sign-in with Microsoft").to_string())?;
ctMail/g
        self.sync_next = true;
ctMail/g
        let begun = self.begin_sign_in(owner.id, oauth, &owner.email, String::new(), false, true);
ctMail/g
        self.sync_next = false;
ctMail/g
        begun?;
ctMail/g
        self.sync_declined.remove(&account);
ctMail/g
        self.sync_mark_asking(account);
ctMail/g
        self.session.sync_status =
ctMail/g
            tl!("Allow ictMail to keep this mailbox in step with Outlook on the page that opened in your browser…").to_string();
ctMail/g
        Ok(json!({"pending": "signIn", "account": owner.id}))
ctMail/g
    }
ctMail/g

ctMail/g
    /// The owner answered on Microsoft's page: with the tokens, everything that waited goes on at
ctMail/g
    /// once (`read`, a shared mailbox the owner asked to be read, is read now); without, why not
ctMail/g
    /// is said, once.
ctMail/g
    pub(crate) fn sync_signed_in(&mut self, account: Id, answer: Result<Tokens, String>, read: Option<Id>) {
ctMail/g
        use crate::approval::NO_BROWSER;
ctMail/g
        use crate::favorites_sync::{NEEDS_APPROVAL, REFUSED_BY_ORGANISATION, REFUSED_FOR_ACCOUNT};
ctMail/g
        self.session.sync_status.clear();
ctMail/g
        let only_shared = self.sync_only_shared(account);
ctMail/g
        self.sync_asking.remove(&account);
ctMail/g
        // A shared mailbox being added that won't be read now: it isn't kept, and the dialog says
ctMail/g
        // why (S1); otherwise why is said in the status bar.
ctMail/g
        let not_read = |app: &mut MailApp, said: String| match read.filter(|r| app.shared_checking.contains(r)) {
ctMail/g
            Some(shared) => app.finish_shared_check(shared, Err(said)),
ctMail/g
            None => app.status(said),
ctMail/g
        };
ctMail/g
        let refusal = match answer {
ctMail/g
            // (Somebody else's tokens are neither used nor remembered, #103.)
ctMail/g
            Ok(tokens) if self.signed_in_as_other(account, &tokens) => {
ctMail/g
                // (What `signed_in_as_other` said is said to whoever waited for the answer.)
ctMail/g
                let said = self.status_msg.as_ref().map(|(said, _)| said.clone()).unwrap_or_default();
ctMail/g
                self.sync_waits_again(account, &said);
ctMail/g
                if let Some(shared) = read.filter(|r| self.shared_checking.contains(r)) {
ctMail/g
                    self.finish_shared_check(shared, Err(said));
ctMail/g
                }
ctMail/g
                return;
ctMail/g
            }
ctMail/g
            Ok(tokens) => {
ctMail/g
                if !tokens.refresh.is_empty() {
ctMail/g
                    self.remember("refresh", account, &tokens.refresh);
ctMail/g
                    if let Some(kept) = self.tokens.get_mut(&account) {
ctMail/g
                        kept.refresh = tokens.refresh.clone();
ctMail/g
                    }
ctMail/g
                }
ctMail/g
                log::info!("sync: the owner allowed what his older sign-in lacked; the next jobs find out");
ctMail/g
                if tokens.sync_not_given {
ctMail/g
                    // (Its page was asked again for the shared mailboxes' permissions alone: the
ctMail/g
                    // organisation has no To Do lists or settings for this account to give, and
ctMail/g
                    // that is not put again by itself.)
ctMail/g
                    let said = tl!("Your organisation doesn't let ictMail keep this mailbox in step with Outlook");
                    self.sync_declined.insert(account);
                    self.tasks_sync_refused(account, said);
                    self.rules_sync_refused(account, said);
                    self.auto_replies_sync_refused(account, said);
                    self.categories_sync_refused(account, said);
                } else {
                    self.tasks_allowed(account);
                    self.rules_allowed(account);
                    self.auto_replies_allowed(account);
                    self.categories_allowed(account);
                }
                // (A page that came back without the shared permissions didn't have them for this
                // account: they are not allowed, and it is not put again for them by itself.)
                let said = if tokens.shared_not_given {
                    self.shared_sync_not_given(account, read)
                } else {
                    // (Noted, so that a job that still finds them missing doesn't open it again.)
                    self.shared_page_given.insert(account);
                    self.shared_sync_allowed(account, read)
                };
                if let Some(said) = said {
                    not_read(self, said);
                }
                return;
            }
            Err(e) if e == NEEDS_APPROVAL => {
                let said = if only_shared {
                    tl!("Your organisation's administrator has to approve ictMail reading shared mailboxes and calendars once: File › Info")
                } else {
                    tl!("Your organisation's administrator has to approve ictMail keeping this mailbox in step with Outlook once: File › Info")
                };
                let _ = self.run("ui.approvalHelp", json!({"account": account}));
                Some((said.to_string(), "approval"))
            }
            Err(e) if e == REFUSED_BY_ORGANISATION || e == REFUSED_FOR_ACCOUNT => {
                let said = if only_shared {
                    tl!("Your organisation doesn't let ictMail read shared mailboxes and the calendars shared with you")
                } else {
                    tl!("Your organisation doesn't let ictMail keep this mailbox in step with Outlook")
                };
                Some((said.to_string(), "organisation"))
            }
            Err(e) if e == NO_BROWSER => {
                let said = match (self.sync_covers_lists(account), self.sync_tasks_kept(account)) {
                    _ if only_shared => tl!("For now, nothing of shared mailboxes is read or sent: {error}"),
                    _ if self.sync_only_tasks(account) => tl!("Tasks stay on this computer for now: {error}"),
                    (true, false) => tl!(
                        "For now, tasks and rules stay on this computer, your automatic replies aren't changed and the categories can't be changed here: {error}"
                    ),
                    (true, true) => {
                        tl!(
                            "For now, rules stay on this computer, your automatic replies aren't changed and the categories can't be changed here: {error}"
                        )
                    }
                    (false, false) => tl!("For now, tasks stay on this computer and your automatic replies aren't changed: {error}"),
                    (false, true) => tl!("For now, your automatic replies aren't changed: {error}"),
                };
                self.sync_not_now(account);
                not_read(self, crate::i18n::fmt(said, &[("error", &e)]));
                None
            }
            Err(e) if only_shared => {
                Some((crate::i18n::fmt(tl!("Shared mailboxes and calendars weren't allowed: {error}"), &[("error", &e)]), "declined"))
            }
            Err(e) => Some((
                crate::i18n::fmt(tl!("ictMail wasn't allowed to keep this mailbox in step with Outlook: {error}"), &[("error", &e)]),
                "declined",
            )),
        };
        if let Some((said, kind)) = refusal {
            // (Not put again by itself: File › Info has Ask again.)
            self.sync_declined.insert(account);
            self.tasks_sync_refused(account, &said);
            self.rules_sync_refused(account, &said);
            self.auto_replies_sync_refused(account, &said);
            self.categories_sync_refused(account, &said);
            self.shared_sync_refused(account, &said, kind);
            not_read(self, said);
        }
    }

    /// The page signed in somebody else: what waited waits as it did.
    fn sync_waits_again(&mut self, account: Id, said: &str) {
        self.tasks_sync_waits(account);
        self.rules_sync_waits(account);
        self.auto_replies_sync_waits(account);
        self.categories_sync_waits(account);
        self.shared_sync_waits_again(account, said);
    }

    /// What File › Info says under a Microsoft account's card: one line for the four, what each
    /// of them stands at (for the tip), and whether Ask again is offered. `None` for an account
    /// none of them is kept for.
    pub(crate) fn sync_line(&self, account: Id) -> Option<(String, String, bool)> {
        let parts: Vec<(String, bool, Perm)> = [
            self.tasks_sync_part(account),
            self.rules_sync_part(account),
            self.auto_replies_sync_part(account),
            self.categories_sync_part(account),
            self.shared_sync_part(account),
        ]
        .into_iter()
        .flatten()
        .collect();
        if parts.is_empty() {
            return None;
        }
        let details = parts.iter().map(|(line, ..)| line.as_str()).collect::<Vec<_>>().join("\n");
        let again = parts.iter().any(|(_, again, _)| *again);
        // A part is off while it waits, or has a way to ask again; the line says on or off only
        // when every part is, and partly on when they differ (their tip says which).
        let off = |(_, again, p): &(String, bool, Perm)| *again || *p == Perm::Waits;
        let line = if parts.iter().any(|(.., p)| *p == Perm::Asking) {
            tl!("Sync with Outlook: asking for your permission…")
        } else if parts.iter().all(off) {
            tl!("Sync with Outlook: off")
        } else if parts.iter().any(off) {
            tl!("Sync with Outlook: partly on")
        } else {
            tl!("Sync with Outlook: on")
        };
        Some((line.to_string(), details, again))
    }
}
