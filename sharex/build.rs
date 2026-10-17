//! Generates the application icon and links it into the .exe as a Win32
//! resource without needing rc.exe / windres:
//! * MSVC target: a hand-written `.res` file (link.exe accepts it directly);
//! * GNU target:  a hand-written COFF object with a `.rsrc` section.

#[path = "src/icon_art.rs"]
mod icon_art;

use std::{env, fs, path::PathBuf};

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const LANG_EN_US: u16 = 0x0409;

struct Res {
    kind: u16,
    id: u16,
    data: Vec<u8>,
}

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

fn resources() -> Vec<Res> {
    let mut list = Vec::new();
    let mut group = Vec::new();
    w16(&mut group, 0);
    w16(&mut group, 1);
    w16(&mut group, icon_art::SIZES.len() as u16);
    for (i, &size) in icon_art::SIZES.iter().enumerate() {
        let id = (i + 1) as u16;
        let dib = icon_art::icon_dib(size);
        let dim = if size >= 256 { 0 } else { size as u8 };
        group.extend_from_slice(&[dim, dim, 0, 0]);
        w16(&mut group, 1); // planes
        w16(&mut group, 32); // bpp
        w32(&mut group, dib.len() as u32);
        w16(&mut group, id);
        list.push(Res { kind: RT_ICON, id, data: dib });
    }
    // Group id 1 == the icon Explorer shows and the app loads at runtime.
    list.push(Res { kind: RT_GROUP_ICON, id: 1, data: group });
    list
}

/// MSVC `.res` format.
fn res_file(list: &[Res]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut entry = |kind: u16, id: u16, lang: u16, data: &[u8]| {
        w32(&mut out, data.len() as u32);
        w32(&mut out, 32);
        w16(&mut out, 0xFFFF);
        w16(&mut out, kind);
        w16(&mut out, 0xFFFF);
        w16(&mut out, id);
        w32(&mut out, 0); // DataVersion
        w16(&mut out, 0x1010); // MOVEABLE | DISCARDABLE
        w16(&mut out, lang);
        w32(&mut out, 0); // Version
        w32(&mut out, 0); // Characteristics
        out.extend_from_slice(data);
        align(&mut out, 4);
    };
    entry(0, 0, 0, &[]); // mandatory empty first entry
    for r in list {
        entry(r.kind, r.id, LANG_EN_US, &r.data);
    }
    out
}

/// COFF object (x86-64) containing a `.rsrc` section: the 3-level resource
/// directory (type -> id -> language) plus data, with ADDR32NB relocations.
fn coff_object(list: &[Res]) -> Vec<u8> {
    let mut types: Vec<u16> = list.iter().map(|r| r.kind).collect();
    types.sort();
    types.dedup();

    let dir_size = |n: usize| 16 + 8 * n;
    // Layout: root dir, one name-dir per type, one lang-dir per resource,
    // data entries, then the raw data.
    let root_off = 0usize;
    let mut off = dir_size(types.len());
    let mut name_dir_off = Vec::new();
    for t in &types {
        name_dir_off.push(off);
        off += dir_size(list.iter().filter(|r| r.kind == *t).count());
    }
    // Order resources by (type, id) as required.
    let mut order: Vec<usize> = (0..list.len()).collect();
    order.sort_by_key(|&i| (list[i].kind, list[i].id));
    let mut lang_dir_off = vec![0; list.len()];
    for &i in &order {
        lang_dir_off[i] = off;
        off += dir_size(1);
    }
    let mut data_entry_off = vec![0; list.len()];
    for &i in &order {
        data_entry_off[i] = off;
        off += 16;
    }
    let mut data_off = vec![0; list.len()];
    for &i in &order {
        off = (off + 7) & !7;
        data_off[i] = off;
        off += list[i].data.len();
    }

    let mut sec = Vec::new();
    let dir_header = |o: &mut Vec<u8>, n: usize| {
        w32(o, 0);
        w32(o, 0);
        w16(o, 0);
        w16(o, 0);
        w16(o, 0); // named entries
        w16(o, n as u16); // id entries
    };
    debug_assert_eq!(root_off, 0);
    dir_header(&mut sec, types.len());
    for (ti, t) in types.iter().enumerate() {
        w32(&mut sec, *t as u32);
        w32(&mut sec, 0x8000_0000 | name_dir_off[ti] as u32);
    }
    for t in &types {
        let items: Vec<usize> = order.iter().copied().filter(|&i| list[i].kind == *t).collect();
        dir_header(&mut sec, items.len());
        for i in items {
            w32(&mut sec, list[i].id as u32);
            w32(&mut sec, 0x8000_0000 | lang_dir_off[i] as u32);
        }
    }
    for &i in &order {
        dir_header(&mut sec, 1);
        w32(&mut sec, LANG_EN_US as u32);
        w32(&mut sec, data_entry_off[i] as u32);
    }
    let mut relocs = Vec::new();
    for &i in &order {
        relocs.push(sec.len() as u32);
        w32(&mut sec, data_off[i] as u32); // addend: section-relative offset
        w32(&mut sec, list[i].data.len() as u32);
        w32(&mut sec, 0);
        w32(&mut sec, 0);
    }
    for &i in &order {
        align(&mut sec, 8);
        assert_eq!(sec.len(), data_off[i]);
        sec.extend_from_slice(&list[i].data);
    }
    align(&mut sec, 4);

    let hdr = 20 + 40;
    let reloc_ptr = hdr + sec.len();
    let sym_ptr = reloc_ptr + relocs.len() * 10;
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
    w16(&mut o, relocs.len() as u16);
    w16(&mut o, 0);
    w32(&mut o, 0xC030_0040); // INITIALIZED_DATA | ALIGN_4 | READ | WRITE
    o.extend_from_slice(&sec);
    for r in relocs {
        w32(&mut o, r);
        w32(&mut o, 0); // symbol index (.rsrc section symbol)
        w16(&mut o, 3); // IMAGE_REL_AMD64_ADDR32NB
    }
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
    println!("cargo:rerun-if-changed=src/icon_art.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap_or_default();

    // Always produce a standalone .ico (handy for docs / installers).
    fs::write(out_dir.join("rustshot.ico"), icon_art::ico_file()).unwrap();

    let list = resources();
    if target.contains("windows-msvc") {
        let p = out_dir.join("rustshot.res");
        fs::write(&p, res_file(&list)).unwrap();
        println!("cargo:rustc-link-arg-bins={}", p.display());
    } else if target.contains("x86_64-pc-windows-gnu") {
        let p = out_dir.join("rustshot_res.o");
        fs::write(&p, coff_object(&list)).unwrap();
        println!("cargo:rustc-link-arg-bins={}", p.display());
    }
}
