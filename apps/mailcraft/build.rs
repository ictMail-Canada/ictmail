// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Windows only: embed the app icon and version info (VERSIONINFO) into `ictmail.exe`, so it
//! shows in Explorer, the taskbar, the Start menu and Alt-Tab.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `MAILCRAFT_REQUIRE_WINRES=1` turns it
//! into an error (for release builds).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/mailcraft.ico");
    println!("cargo:rerun-if-env-changed=MAILCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/mailcraft.ico")
        .set("ProductName", "ictMail")
        .set("FileDescription", "ictMail mail and calendar")
        .set("CompanyName", "ictTech")
        .set("LegalCopyright", "Copyright (c) 2026 ictTech. Based on MailCraft (MIT OR Apache-2.0).")
        .set("OriginalFilename", "ictmail.exe")
        .set("InternalName", "ictmail");
    if let Err(e) = res.compile() {
        if std::env::var_os("MAILCRAFT_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=ictmail.exe built without icon/version resources: {e}");
    }
}
