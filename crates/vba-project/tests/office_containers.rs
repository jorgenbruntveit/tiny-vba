//! Frame and MultiPage storage as Office writes it. Observed in Excel for Mac
//! 16.115 output for a test form built through the VBA designer object model.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
#[path = "support/native_forms.rs"]
mod fixture;
use vba_project::{
    Limits,
    forms::{self, Kind, NativeForm, Resource},
};
fn original() -> NativeForm {
    forms::decode_export(
        "NativeForm.frm",
        &fixture::export(),
        1252,
        &[Resource {
            name: "NativeForm.frx".into(),
            bytes: fixture::multipage_companion(0),
        }],
        &Limits::default(),
    )
    .unwrap()
}
/// FormControl PropMask of a stream in an export's companion.
fn form_mask(out: &[Resource], stream: &str) -> u32 {
    use std::io::{Cursor, Read};
    let mut file = cfb::CompoundFile::open(Cursor::new(out[1].bytes[4..].to_vec())).unwrap();
    let mut f = Vec::new();
    file.open_stream(stream)
        .unwrap()
        .read_to_end(&mut f)
        .unwrap();
    u32::from_le_bytes(f[4..8].try_into().unwrap())
}

#[test]
fn a_page_caption_stays_in_its_tab_not_in_the_page() {
    // Office draws a Page FormControl.Caption a second time inside the page.
    let before = original();
    assert_eq!(
        form_mask(
            &forms::encode_export(&before, &before.design, &before.source, &Limits::default())
                .unwrap(),
            "/i20/i31/f"
        ) & (1 << 19),
        0
    );
    let mut design = before.design.clone();
    let page = design.controls.iter_mut().find(|c| c.id == 31).unwrap();
    page.caption = "Edited".into();
    forms::sync_multipage_tabs(&before.design, &mut design).unwrap();
    let out = forms::encode_export(&before, &design, &before.source, &Limits::default()).unwrap();
    for page in ["/i20/i31/f", "/i20/i32/f", "/i20/i33/f"] {
        assert_eq!(form_mask(&out, page) & (1 << 19), 0, "{page}");
    }
    let reopened = forms::decode_export(
        &out[0].name,
        &out[0].bytes,
        1252,
        &out[1..],
        &Limits::default(),
    )
    .unwrap()
    .design;
    assert_eq!(reopened, design);
    assert_eq!(
        reopened
            .controls
            .iter()
            .find(|c| c.id == 31)
            .unwrap()
            .caption,
        "Edited"
    );
}

/// The shared MultiPage fixture with Office's page site flags: fVisible only
/// on the site of the page on display (the first).
fn office_page_flags() -> NativeForm {
    use std::io::{Cursor, Read, Write};
    let original = fixture::multipage_companion(0);
    let mut file = cfb::CompoundFile::open(Cursor::new(original[4..].to_vec())).unwrap();
    let mut f = Vec::new();
    file.open_stream("/i20/f")
        .unwrap()
        .read_to_end(&mut f)
        .unwrap();
    for page in [&b"SecondTab"[..], b"BlockedTab"] {
        let name = f.windows(page.len()).position(|w| w == page).unwrap();
        // The site's BitFlags precede its size, class and name fields.
        let flags = name - 12;
        assert_eq!(
            u32::from_le_bytes(f[flags..flags + 4].try_into().unwrap()) & 2,
            2
        );
        f[flags] &= !2;
    }
    file.create_stream("/i20/f").unwrap().write_all(&f).unwrap();
    let bytes = file.into_inner().into_inner();
    let mut companion = (bytes.len() as u32).to_le_bytes().to_vec();
    companion.extend(bytes);
    forms::decode_export(
        "NativeForm.frm",
        &fixture::export(),
        1252,
        &[Resource {
            name: "NativeForm.frx".into(),
            bytes: companion,
        }],
        &Limits::default(),
    )
    .unwrap()
}
/// fVisible of each page site (31, 32, 33) in an export's MultiPage storage.
fn page_site_visible(out: &[Resource]) -> Vec<(u32, bool)> {
    use std::io::{Cursor, Read};
    let mut file = cfb::CompoundFile::open(Cursor::new(out[1].bytes[4..].to_vec())).unwrap();
    let mut f = Vec::new();
    file.open_stream("/i20/f")
        .unwrap()
        .read_to_end(&mut f)
        .unwrap();
    let u32_at = |at: usize| u32::from_le_bytes(f[at..at + 4].try_into().unwrap());
    let mut found: Vec<(u32, bool)> = Vec::new();
    // Site records: version 0, size, PropMask, then Name/Tag lengths, ID, BitFlags.
    for at in 0..f.len().saturating_sub(24) {
        if f[at..at + 2] != [0, 0] {
            continue;
        }
        let mask = u32_at(at + 4);
        if mask & !0x6bff != 0 || mask & 0x94 != 0x94 {
            continue;
        }
        let mut field = at + 8 + 4 * (mask & 1) as usize + 4 * ((mask >> 1) & 1) as usize;
        let id = u32_at(field);
        field += 4;
        if (31..=33).contains(&id) && found.iter().all(|(i, _)| *i != id) {
            found.push((id, u32_at(field) & 2 != 0));
        }
    }
    found.sort();
    found
}

#[test]
fn page_site_visibility_follows_the_selection_only_when_tabs_change() {
    let before = office_page_flags();
    let encode = |design: &forms::Design| {
        let mut design = design.clone();
        forms::sync_multipage_tabs(&before.design, &mut design).unwrap();
        let out =
            forms::encode_export(&before, &design, &before.source, &Limits::default()).unwrap();
        let reopened = forms::decode_export(
            &out[0].name,
            &out[0].bytes,
            1252,
            &out[1..],
            &Limits::default(),
        )
        .unwrap()
        .design;
        assert_eq!(reopened, design);
        page_site_visible(&out)
    };
    // A rename leaves the tabs alone, so the site bits stay as read.
    let mut renamed = before.design.clone();
    renamed
        .controls
        .iter_mut()
        .find(|c| c.id == 32)
        .unwrap()
        .name = "Renamed".into();
    assert_eq!(encode(&renamed), [(31, true), (32, false), (33, false)]);
    // A caption edit rewrites the tabs: only the selected page's site is shown.
    let mut captioned = before.design.clone();
    captioned
        .controls
        .iter_mut()
        .find(|c| c.id == 33)
        .unwrap()
        .caption = "Edited".into();
    assert_eq!(encode(&captioned), [(31, true), (32, false), (33, false)]);
    // A hidden selected page is not on display.
    let mut hidden = before.design.clone();
    hidden
        .controls
        .iter_mut()
        .find(|c| c.id == 31)
        .unwrap()
        .visible = false;
    assert_eq!(encode(&hidden), [(31, false), (32, false), (33, false)]);
    // Deleting the selected page moves the selection, and fVisible with it.
    let mut deleted = before.design.clone();
    deleted
        .controls
        .retain(|c| c.id != 31 && c.parent != Some(31));
    assert_eq!(encode(&deleted), [(32, true), (33, false)]);
}

#[test]
fn page_visibility_comes_from_the_tab() {
    // Office sets fVisible only on the site of the page on display; the other
    // pages are still visible.
    let form = office_page_flags();
    let pages = form
        .design
        .controls
        .iter()
        .filter(|c| c.kind == Kind::Page)
        .map(|c| (c.name.as_str(), c.visible))
        .collect::<Vec<_>>();
    assert_eq!(
        pages,
        [
            ("FirstTab", true),
            ("SecondTab", true),
            ("BlockedTab", true)
        ]
    );
}

#[test]
fn only_a_multipage_internal_tabstrip_may_be_unnamed() {
    // A root TabStrip ("Tabs1") stored without a Name.
    use std::io::{Cursor, Read, Write};
    let original = fixture::tab_companion(0);
    let mut file = cfb::CompoundFile::open(Cursor::new(original[4..].to_vec())).unwrap();
    let mut f = Vec::new();
    file.open_stream("/f").unwrap().read_to_end(&mut f).unwrap();
    let name = f.windows(5).position(|w| w == b"Tabs1").unwrap();
    // Site record: version, size, PropMask, Name length (compressed), ID,
    // BitFlags, ObjectStreamSize, TabIndex, class, then the padded name.
    let record = name - 28;
    let u32_at = |f: &[u8], at: usize| u32::from_le_bytes(f[at..at + 4].try_into().unwrap());
    assert_eq!(u32_at(&f, record + 4), 0x1f5);
    assert_eq!(u32_at(&f, record + 8), 0x8000_0005);
    // Drop the Name: clear PropMask bit 0, remove its length and its 8 bytes.
    f.drain(name..name + 8);
    f.drain(record + 8..record + 12);
    f[record + 4..record + 8].copy_from_slice(&0x1f4u32.to_le_bytes());
    let size = u16::from_le_bytes([f[record + 2], f[record + 3]]) - 12;
    f[record + 2..record + 4].copy_from_slice(&size.to_le_bytes());
    // After the FormControl record: a u16, the site count, then the site
    // list's byte count.
    let list = 4 + u16::from_le_bytes([f[2], f[3]]) as usize + 2 + 4 + 4;
    let total = u32_at(&f, list - 4) - 12;
    f[list - 4..list].copy_from_slice(&total.to_le_bytes());
    file.create_stream("/f").unwrap().write_all(&f).unwrap();
    let bytes = file.into_inner().into_inner();
    let mut companion = (bytes.len() as u32).to_le_bytes().to_vec();
    companion.extend(bytes);
    let decoded = forms::decode_export(
        "NativeForm.frm",
        &fixture::export(),
        1252,
        &[Resource {
            name: "NativeForm.frx".into(),
            bytes: companion,
        }],
        &Limits::default(),
    );
    assert_eq!(
        decoded.err().map(|e| e.message).as_deref(),
        Some("unnamed native control")
    );
}

#[test]
fn a_zero_logical_size_stays_zero_when_a_page_is_rewritten() {
    // Office writes a Page's LogicalSize as (0, 0); give the first page's
    // FormControl that shape: set bit 11 and append (0, 0) after DisplayedSize.
    use std::io::{Cursor, Read, Write};
    let original = fixture::multipage_companion(0);
    let mut file = cfb::CompoundFile::open(Cursor::new(original[4..].to_vec())).unwrap();
    let mut f = Vec::new();
    file.open_stream("/i20/i31/f")
        .unwrap()
        .read_to_end(&mut f)
        .unwrap();
    let size = u16::from_le_bytes([f[2], f[3]]) as usize;
    let mask = u32::from_le_bytes(f[4..8].try_into().unwrap()) | (1 << 11);
    f[4..8].copy_from_slice(&mask.to_le_bytes());
    f[2..4].copy_from_slice(&((size + 8) as u16).to_le_bytes());
    f.splice(4 + size..4 + size, [0u8; 8]);
    file.create_stream("/i20/i31/f")
        .unwrap()
        .write_all(&f)
        .unwrap();
    let bytes = file.into_inner().into_inner();
    let mut companion = (bytes.len() as u32).to_le_bytes().to_vec();
    companion.extend(bytes);
    let before = forms::decode_export(
        "NativeForm.frm",
        &fixture::export(),
        1252,
        &[Resource {
            name: "NativeForm.frx".into(),
            bytes: companion,
        }],
        &Limits::default(),
    )
    .unwrap();
    let mut design = before.design.clone();
    design
        .controls
        .iter_mut()
        .find(|c| c.id == 31)
        .unwrap()
        .caption = "Edited".into();
    forms::sync_multipage_tabs(&before.design, &mut design).unwrap();
    let out = forms::encode_export(&before, &design, &before.source, &Limits::default()).unwrap();
    let mut file = cfb::CompoundFile::open(Cursor::new(out[1].bytes[4..].to_vec())).unwrap();
    let mut f = Vec::new();
    file.open_stream("/i20/i31/f")
        .unwrap()
        .read_to_end(&mut f)
        .unwrap();
    let size = u16::from_le_bytes([f[2], f[3]]) as usize;
    assert_ne!(
        u32::from_le_bytes(f[4..8].try_into().unwrap()) & (1 << 11),
        0
    );
    // The record ends with DisplayedSize, then LogicalSize (0, 0).
    assert_eq!(f[4 + size - 8..4 + size], [0u8; 8]);
}
