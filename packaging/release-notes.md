**ictMail 0.23.0 in short**: a maintenance release on 0.13.0 — mail setup that works for ictMail-hosted domains out of the box.
- Add Account suggests a hosted domain's own mail server, `mail.<domain>`, instead of `imap.`/`smtp.` names that usually don't resolve.
- The About window and page show the white ictMail wordmark on a dark theme.
- And the fixes below.

**What's new**
- Add Account now suggests a domain's own mail exchanger as its incoming and outgoing servers when nothing else is known about the domain, so an ictMail-hosted domain's `mail.<domain>` is offered instead of `imap.`/`smtp.` names that usually don't resolve.

**Fixes**
- The About window and page show the white ictMail wordmark when the interface is dark, where the colour one all but vanished.

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
