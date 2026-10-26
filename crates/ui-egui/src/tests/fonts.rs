//! The fonts the window is lettered in (IBM Plex, built into the executable, under the SIL Open Font License) and what every
//! package owes with them: the notice, and the licence text beside each font. These tests read the files of the source, as the
//! ones for the Revit reader's notice do (`crates/rvt/src/tests`).

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Each font's folder in `assets/fonts` and its file.
const FONTS: [(&str, &str); 2] = [("ibm-plex-sans", "IBMPlexSans-Variable.ttf"), ("ibm-plex-mono", "IBMPlexMono-Regular.ttf")];

#[test]
fn the_notice_names_the_fonts_the_executable_holds_and_each_has_its_licence_beside_it() {
    let notice = read("NOTICE");
    for (dir, file) in FONTS {
        let sum: String = bimcraft_update::sha256_file(&root().join("assets/fonts").join(dir).join(file))
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(notice.contains(file) && notice.contains(&sum), "NOTICE names {file} by its sha256, {sum}");
        let ofl = read(&format!("assets/fonts/{dir}/OFL.txt"));
        assert!(ofl.starts_with("Copyright © 2017 IBM Corp. with Reserved Font Name \"Plex\""), "{dir}'s notice");
        assert!(ofl.contains("SIL OPEN FONT LICENSE Version 1.1"), "{dir}'s licence text");
    }
    for words in [
        "Copyright (c) 2017 IBM Corp. with Reserved Font Name \"Plex\"",
        "SIL Open Font License, Version 1.1",
        "Version 3.201",
        "Version 2.3",
    ] {
        assert!(notice.contains(words), "NOTICE says `{words}`");
    }
}

/// The OFL asks for its notice and text with every copy of the fonts. Each script that makes a package, by what it must say.
#[test]
fn every_package_ships_the_notice_and_the_licence_of_the_fonts() {
    for (file, wants) in [
        // copy_docs, which every Linux package, the macOS app and the macOS command line zip are given their docs by.
        ("packaging/env.sh", &["NOTICE", "ibm-plex-sans ibm-plex-mono", "assets/fonts/$font/OFL.txt"][..]),
        // The AppImage keeps what it was staged with (it once dropped share/doc), the deb and the rpm take the whole share tree.
        ("packaging/linux/package.sh", &["copy_docs \"$STAGE/usr/share/doc/bimcraft\""][..]),
        ("packaging/linux/nfpm.yaml", &["src: ${STAGE}/usr/share/"][..]),
        ("packaging/linux/arch-package.sh", &["licenses/$pkgname/fonts/$font/OFL.txt"][..]),
        ("packaging/linux/flatpak/io.github.ipedrax.bimcraft.bundle.yml", &["cp -R share/doc/bimcraft"][..]),
        ("packaging/macos/package.sh", &["copy_docs \"$APP/Contents/Resources\""][..]),
        (
            "packaging/windows/bimcraft.wxs",
            &["Source=\"NOTICE\"", "assets\\fonts\\ibm-plex-sans\\OFL.txt", "assets\\fonts\\ibm-plex-mono\\OFL.txt"][..],
        ),
        ("packaging/windows/portable.ps1", &["$root\\NOTICE", "fonts\\ibm-plex-sans\\OFL.txt", "fonts\\ibm-plex-mono\\OFL.txt"][..]),
        ("packaging/windows/check-install.ps1", &["'NOTICE'", "fonts\\ibm-plex-sans\\OFL.txt", "fonts\\ibm-plex-mono\\OFL.txt"][..]),
    ] {
        let text = read(file);
        for want in wants {
            assert!(text.contains(want), "{file} must say `{want}`");
        }
    }
    assert!(!read("packaging/linux/package.sh").contains("rm -rf \"$APPDIR/usr/share/doc\""), "the AppImage ships its docs");
}

#[cfg(unix)]
#[test]
fn copy_docs_puts_the_notice_and_the_two_licences_where_the_notice_says_they_are() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("doc");
    std::fs::create_dir(&out).unwrap();
    let status = std::process::Command::new("bash")
        .current_dir(root())
        .args(["-c", ". packaging/env.sh && copy_docs \"$1\"", "bash"])
        .arg(&out)
        .env("DIST", dir.path().join("dist"))
        .env("CARGO_TARGET_DIR", dir.path().join("target"))
        .status()
        .unwrap();
    assert!(status.success());
    for file in ["README.md", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE"] {
        assert!(out.join(file).is_file(), "{file}");
    }
    for (dir, _) in FONTS {
        let copy = std::fs::read_to_string(out.join("fonts").join(dir).join("OFL.txt")).unwrap();
        assert_eq!(copy, read(&format!("assets/fonts/{dir}/OFL.txt")), "fonts/{dir}/OFL.txt");
    }
    assert!(read("NOTICE").contains("fonts/ibm-plex-sans/OFL.txt"), "and NOTICE names that place");
}
