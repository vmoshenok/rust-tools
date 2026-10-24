//! Links `assets/app.manifest` into the .exe as an RT_MANIFEST resource
//! (Common Controls v6 + DPI awareness) without needing rc.exe / windres:
//! * MSVC target: a hand-written `.res` file (link.exe accepts it directly);
//! * GNU target:  a hand-written COFF object with a `.rsrc` section.

use std::{env, fs, path::PathBuf};

const RT_MANIFEST: u32 = 24;
const MANIFEST_ID: u32 = 1; // CREATEPROCESS_MANIFEST_RESOURCE_ID
const LANG_EN_US: u32 = 0x0409;

fn w16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn w32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn align(o: &mut Vec<u8>, n: usize) {
    while o.len() % n != 0 {
        o.push(0);
    }
}

/// MSVC `.res` format.
fn res_file(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut entry = |kind: u32, id: u32, lang: u32, data: &[u8]| {
        w32(&mut out, data.len() as u32);
        w32(&mut out, 32);
        w16(&mut out, 0xFFFF);
        w16(&mut out, kind as u16);
        w16(&mut out, 0xFFFF);
        w16(&mut out, id as u16);
        w32(&mut out, 0); // DataVersion
        w16(&mut out, 0x1030); // MOVEABLE | PURE | DISCARDABLE
        w16(&mut out, lang as u16);
        w32(&mut out, 0); // Version
        w32(&mut out, 0); // Characteristics
        out.extend_from_slice(data);
        align(&mut out, 4);
    };
    entry(0, 0, 0, &[]); // mandatory empty first entry
    entry(RT_MANIFEST, MANIFEST_ID, LANG_EN_US, data);
    out
}

/// COFF object (x86-64) with a `.rsrc` section holding one resource:
/// type dir -> id dir -> language dir -> data entry -> data.
fn coff_object(data: &[u8]) -> Vec<u8> {
    let dir = |o: &mut Vec<u8>, id: u32, target: u32| {
        w32(o, 0); // Characteristics
        w32(o, 0); // TimeDateStamp
        w16(o, 0); // Major
        w16(o, 0); // Minor
        w16(o, 0); // named entries
        w16(o, 1); // id entries
        w32(o, id);
        w32(o, target);
    };
    // Each directory with one entry is 24 bytes.
    let mut sec = Vec::new();
    dir(&mut sec, RT_MANIFEST, 0x8000_0000 | 24);
    dir(&mut sec, MANIFEST_ID, 0x8000_0000 | 48);
    dir(&mut sec, LANG_EN_US, 72); // -> data entry
    let reloc_at = sec.len() as u32;
    let data_off = 72 + 16;
    w32(&mut sec, data_off); // addend: section-relative offset (relocated)
    w32(&mut sec, data.len() as u32);
    w32(&mut sec, 0); // CodePage
    w32(&mut sec, 0); // Reserved
    assert_eq!(sec.len() as u32, data_off);
    sec.extend_from_slice(data);
    align(&mut sec, 4);

    let hdr = 20 + 40;
    let reloc_ptr = hdr + sec.len();
    let sym_ptr = reloc_ptr + 10;
    let mut o = Vec::new();
    // IMAGE_FILE_HEADER
    w16(&mut o, 0x8664);
    w16(&mut o, 1);
    w32(&mut o, 0);
    w32(&mut o, sym_ptr as u32);
    w32(&mut o, 1); // symbols
    w16(&mut o, 0);
    w16(&mut o, 0);
    // IMAGE_SECTION_HEADER
    o.extend_from_slice(b".rsrc\0\0\0");
    w32(&mut o, 0);
    w32(&mut o, 0);
    w32(&mut o, sec.len() as u32);
    w32(&mut o, hdr as u32);
    w32(&mut o, reloc_ptr as u32);
    w32(&mut o, 0);
    w16(&mut o, 1); // relocations
    w16(&mut o, 0);
    w32(&mut o, 0xC030_0040); // INITIALIZED_DATA | ALIGN_4 | READ | WRITE
    o.extend_from_slice(&sec);
    // Relocation
    w32(&mut o, reloc_at);
    w32(&mut o, 0); // symbol index (.rsrc section symbol)
    w16(&mut o, 3); // IMAGE_REL_AMD64_ADDR32NB
    // Symbol: .rsrc, section 1, static
    o.extend_from_slice(b".rsrc\0\0\0");
    w32(&mut o, 0);
    w16(&mut o, 1);
    w16(&mut o, 0);
    o.push(3);
    o.push(0);
    w32(&mut o, 4); // empty string table
    o
}

fn main() {
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap_or_default();
    let manifest = fs::read("assets/app.manifest").unwrap();

    if target.contains("windows-msvc") {
        let p = out_dir.join("windows-menu.res");
        fs::write(&p, res_file(&manifest)).unwrap();
        println!("cargo:rustc-link-arg-bins={}", p.display());
    } else if target.contains("x86_64-pc-windows-gnu") {
        let p = out_dir.join("windows-menu_res.o");
        fs::write(&p, coff_object(&manifest)).unwrap();
        println!("cargo:rustc-link-arg-bins={}", p.display());
    }
}
