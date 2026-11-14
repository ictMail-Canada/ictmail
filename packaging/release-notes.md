**ictMail 0.13.0 in short**: MailCraft's mail, calendar and contacts program, now ictMail — built for ictTech's hosted mail platform.
- CalDAV and CardDAV sync: an account's calendars and contacts stay in step with a CalDAV/CardDAV server — ictMail/SOGo included — with changes made here going back up.
- The Windows installer offers the sync as an optional feature, and its finish page now always leaves a desktop shortcut that uninstall removes again.
- ictMail has its own update feed and release signing: the app checks this repository's releases and trusts ictMail's release key.
- New ictMail look throughout — name, icon, installer, About page — and `ictmail` / `ictmail-cli` binaries on every platform.
- And the fixes below.

**What's new**
- The program is ictMail: window title, About page with the ictMail wordmark, icon, `ictMail`/`ictmail` data folders, log and diagnostics names, and the `ictmail` / `ictmail-cli` executables (the command-line tool and MCP server). Crate and package names stay `mailcraft-*`, and the project remains a MailCraft fork — upstream is [UCS-Care-Group/mailcraft](https://github.com/UCS-Care-Group/mailcraft).
- **CalDAV/CardDAV sync.** An IMAP account's settings can keep its calendars and contacts in step with a CalDAV or CardDAV server — ictMail/SOGo included: events and contacts come down, and changes made here go back up under the server's own tags. On Windows the installer offers it as the optional **ictMail ActiveSync** feature; portable and source builds have it on.
- The update check now asks ictMail's own repository and trusts ictMail's release-signing key beside MailCraft's, so a release published here — `SHA256SUMS.txt` signed by the ictMail key — is one a copy installs for itself.
- The Windows installer is ictMail's own: branded banner and dialogs, a finish page with **Launch ictMail** and a desktop-shortcut option, and the optional feature tree.
- The interface's translations were carried onto the renamed strings and regenerated (de, es, fr, it, nl and the rest).

**Fixes**
- The Windows installer now leaves a desktop shortcut on the installing user's own Desktop — a redirected (e.g. OneDrive) desktop included — and removes it on uninstall; earlier builds silently made none for a normal install.
- Internal mail headers answer to both their ictMail and MailCraft names, so messages marked by either version are still recognised.
- Linux packages are named `ictmail` end to end — `.deb`, `.rpm`, AppImage, Flatpak, tarball and the FreeBSD package all agree, with documentation under `usr/share/doc/ictmail`.
- The contact-sync commands (`sync.contactChanges`, `sync.applyContactChanges`) are classified for agents.
- A flaky test that needed a port nobody listens on now retries a few times before failing.

**Public beta.** For Microsoft accounts (personal, or work or school) ictMail is a working mail, calendar and contacts program, kept in step with Outlook on the web; keep Outlook to hand while you try it, and tell us what is lost or out of step. Much of what a daily mail program needs is still to come, and other kinds of account are welcome but not part of the beta. What works is in the [README](https://github.com/GITHUB_REPOSITORY#what-works-today).

ictMail for Windows, macOS, Linux, FreeBSD and the web. Every file is listed with its SHA-256 in `SHA256SUMS.txt`.

| Platform | File | |
|---|---|---|
| Windows 10/11 (x64, x86, ARM64) | `ictmail-<ver>-windows-<arch>.msi` | Installer for all users: Program Files\ictMail and the Start menu, with a finish-page option for a desktop shortcut and to launch ictMail; offers ictMail for `mailto:` links and for `.eml`, `.ics` and `.vcf` files without taking them over (choose it in Settings › Default apps); uninstall from Installed apps. `ictmail-cli.exe` is installed next to the app. |
| | `ictmail-<ver>-windows-<arch>-portable.zip` | Portable: unzip anywhere and run `ictmail.exe`; includes `ictmail-cli.exe`. Registers nothing. |
| macOS 11+ (Apple silicon and Intel) | `ictmail-<ver>-macos-universal.dmg` | Drag the app to Applications. |
| | `ictmail-cli-<ver>-macos-universal.zip` | The command-line tool and MCP server alone. |
| Linux x86_64 and aarch64 (glibc 2.35+) | `ictmail-<ver>-linux-<arch>.AppImage` | `chmod +x` and run (if it does not start, add `--appimage-extract-and-run`). The `.zsync` next to it enables delta updates. |
| | `ictmail-<ver>-linux-<arch>.deb` / `.rpm` | `sudo apt install ./ictmail-*.deb` or `sudo dnf install ./ictmail-*.rpm`. |
| | `ictmail-<ver>-linux-<arch>.flatpak` | `flatpak install --user ictmail-*.flatpak` (the freedesktop runtime comes from Flathub). |
| | `ictmail-<ver>-linux-<arch>.tar.gz` | `bin/ictmail`, `bin/ictmail-cli`, the desktop entry, metainfo and icons under `share/`. |
| FreeBSD 15 (x86_64) | `ictmail-<ver>-freebsd-x86_64.tar.gz` | The two programs, the desktop entry and the icons. |
| Any modern browser | `mailcraft-web-<ver>.zip` | The app as a static site (WebAssembly). From a browser it reaches JMAP servers only, to read ([docs/jmap.md](https://github.com/GITHUB_REPOSITORY/blob/main/docs/jmap.md)): bring other mail in with File › Import. |

ictMail keeps your mailbox in its settings folder and saves as you go; one window per mailbox. With a Microsoft 365 or Outlook.com account you sign in on Microsoft's page, and the sign-in is kept by the system's credential store (Windows, macOS, and the Secret Service of GNOME Keyring or KWallet on Linux). What works, and how far each part is from Outlook, is in the [README](https://github.com/GITHUB_REPOSITORY#what-works-today) and in `docs/gaps.md`.

The builds are not signed with a publisher certificate yet: Windows SmartScreen and macOS Gatekeeper will ask before running them.
