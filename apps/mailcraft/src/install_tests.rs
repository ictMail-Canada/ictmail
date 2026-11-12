// SPDX-FileCopyrightText: MailCraft contributors
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests of `install.rs`.

use super::*;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

#[test]
fn the_file_for_each_copy() {
    let version = v("0.5.0");
    for (install, arch, name) in [
        (Install::Msi, "x86_64", Some("ictmail-0.5.0-windows-x64.msi")),
        (Install::Msi, "x86", Some("ictmail-0.5.0-windows-x86.msi")),
        (Install::Msi, "aarch64", Some("ictmail-0.5.0-windows-arm64.msi")),
        (Install::MacApp, "aarch64", Some("ictmail-0.5.0-macos-universal.dmg")),
        (Install::MacApp, "x86_64", Some("ictmail-0.5.0-macos-universal.dmg")),
        (Install::AppImage, "x86_64", Some("ictmail-0.5.0-linux-x86_64.AppImage")),
        (Install::AppImage, "aarch64", Some("ictmail-0.5.0-linux-aarch64.AppImage")),
        (Install::AppImage, "riscv64", None),
        (Install::Msi, "powerpc", None),
        (Install::NoticeOnly, "x86_64", None),
    ] {
        assert_eq!(install.file_name(&version, arch).as_deref(), name, "{install:?} {arch}");
    }
    assert_eq!(Install::Msi.file_name(&v("1.2.3-rc.1"), "x86_64").as_deref(), Some("ictmail-1.2.3-rc.1-windows-x64.msi"));
    // Every name is one a release's file may have.
    for install in [Install::Msi, Install::MacApp, Install::AppImage] {
        for arch in ["x86_64", "x86", "aarch64"] {
            if let Some(name) = install.file_name(&v("10.20.30"), arch) {
                assert!(mailcraft_model::update::is_file_name(&name), "{name}");
            }
        }
    }
}

#[test]
fn an_msi_copy_is_the_one_the_installer_registered() {
    let exe = Path::new(r"C:\Program Files\ictMail\ictmail.exe");
    assert_eq!(windows(exe, Some(Path::new(r"C:\Program Files\ictMail\ictmail.exe"))), Install::Msi);
    // The comparison ignores case, as Windows paths do.
    assert_eq!(windows(exe, Some(Path::new(r"c:\program files\ictmail\ICTMAIL.EXE"))), Install::Msi);
    // The portable zip, anywhere, while an installed copy is registered elsewhere; or none.
    let portable = Path::new(r"C:\Users\a\Downloads\ictmail\ictmail.exe");
    assert_eq!(windows(portable, Some(exe)), Install::NoticeOnly);
    assert_eq!(windows(exe, None), Install::NoticeOnly);
}

// Paths of macOS and Linux, which are absolute only there.
#[test]
#[cfg(unix)]
fn a_mac_app_where_the_owner_keeps_it() {
    assert_eq!(mac(Path::new("/Applications/ictMail.app/Contents/MacOS/ictmail")), Install::MacApp);
    assert_eq!(mac(Path::new("/Users/a/Apps/ictMail.app/Contents/MacOS/ictmail")), Install::MacApp);
    for not in [
        "/Volumes/ictMail/ictMail.app/Contents/MacOS/ictmail",
        "/private/var/folders/x/T/AppTranslocation/1234/d/ictMail.app/Contents/MacOS/ictmail",
        "/usr/local/bin/ictmail",
        "/Users/a/ictmail/target/release/ictmail",
        "/Applications/ictMail.app/Contents/Resources/ictmail",
        "ictMail.app/Contents/MacOS/ictmail",
    ] {
        assert_eq!(mac(Path::new(not)), Install::NoticeOnly, "{not}");
    }
}

// Paths of macOS and Linux, which are absolute only there.
#[test]
#[cfg(unix)]
fn an_appimage_by_what_its_runtime_says() {
    let image = Some(OsStr::new("/home/a/Apps/ictmail-0.5.0-linux-x86_64.AppImage"));
    let dir = Some(OsStr::new("/tmp/.mount_ictmXYZ"));
    let exe = Path::new("/tmp/.mount_ictmXYZ/usr/bin/ictmail");
    assert_eq!(linux(image, dir, false, exe), Install::AppImage);
    assert_eq!(linux(image, dir, true, exe), Install::NoticeOnly);
    assert_eq!(linux(None, dir, false, exe), Install::NoticeOnly);
    assert_eq!(linux(image, None, false, exe), Install::NoticeOnly);
    assert_eq!(linux(image, dir, false, Path::new("/usr/bin/ictmail")), Install::NoticeOnly);
    assert_eq!(linux(Some(OsStr::new("relative.AppImage")), dir, false, exe), Install::NoticeOnly);
    assert_eq!(linux(Some(OsStr::new("/home/a/ictmail.tar.gz")), dir, false, exe), Install::NoticeOnly);
}

// #497: a Mac app is told of new versions only while the switch is off, whatever its folder.
#[test]
fn a_mac_app_is_notice_only_while_the_switch_is_off() {
    let mut asked = false;
    assert_eq!(
        updating(Install::MacApp, false, &mut || {
            asked = true;
            true
        }),
        Install::NoticeOnly
    );
    assert!(!asked, "its folder isn't tried");
    assert_eq!(updating(Install::MacApp, true, &mut || true), Install::MacApp);
    assert_eq!(updating(Install::MacApp, true, &mut || false), Install::NoticeOnly);
    // The switch is about the Mac alone.
    assert_eq!(updating(Install::AppImage, false, &mut || true), Install::AppImage);
    assert_eq!(updating(Install::Msi, false, &mut || false), Install::Msi);
}

// #497: a copy this program replaces itself, in a folder it can't write to, is told of new
// versions only; the `.msi` is Windows Installer's to install.
#[test]
fn a_folder_that_cant_be_written_gives_notice_only() {
    assert_eq!(updating(Install::AppImage, true, &mut || false), Install::NoticeOnly);
    assert_eq!(updating(Install::AppImage, true, &mut || true), Install::AppImage);
    assert_eq!(updating(Install::Msi, true, &mut || false), Install::Msi);
    assert_eq!(updating(Install::NoticeOnly, true, &mut || true), Install::NoticeOnly);
}

#[test]
#[cfg(unix)]
fn a_folder_is_writable_when_a_file_can_be_made_in_it() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("ictmail-install-writable-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    assert!(writable(&dir));
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "the file tried is removed");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    // (Root writes anywhere: then there is nothing to tell.)
    let root = fs::write(dir.join("x"), b"x").is_ok();
    let _ = fs::remove_file(dir.join("x"));
    if !root {
        assert!(!writable(&dir));
    }
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!writable(&dir.join("not-there")));
    let _ = fs::remove_dir_all(&dir);
}
