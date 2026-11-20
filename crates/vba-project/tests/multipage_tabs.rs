//! A MultiPage's tab list follows its pages: names, captions and state
//! re-encode in the internal TabStrip; new or reordered pages are refused.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
#[path = "support/native_forms.rs"]
mod fixture;
use vba_project::{
    Limits,
    forms::{self, Kind, NativeForm, Resource},
};
fn original(selected: i32) -> NativeForm {
    forms::decode_export(
        "NativeForm.frm",
        &fixture::export(),
        1252,
        &[Resource {
            name: "NativeForm.frx".into(),
            bytes: fixture::multipage_companion(selected),
        }],
        &Limits::default(),
    )
    .unwrap()
}
fn reopen(before: &NativeForm, design: &forms::Design) -> forms::Design {
    let out = forms::encode_export(before, design, &before.source, &Limits::default()).unwrap();
    forms::decode_export(
        &out[0].name,
        &out[0].bytes,
        1252,
        &out[1..],
        &Limits::default(),
    )
    .unwrap()
    .design
}
fn tabs(design: &forms::Design) -> &forms::TabBehavior {
    design
        .controls
        .iter()
        .find(|c| c.kind == Kind::MultiPage)
        .and_then(|c| c.tabs.as_ref())
        .unwrap()
}

#[test]
fn unchanged_pages_keep_their_tabs() {
    let before = original(1);
    let mut design = before.design.clone();
    forms::sync_multipage_tabs(&before.design, &mut design).unwrap();
    assert_eq!(design, before.design);
}

#[test]
fn renamed_and_deleted_pages_follow_into_the_tab_list() {
    let before = original(2);
    let mut design = before.design.clone();
    let page = design.controls.iter_mut().find(|c| c.id == 32).unwrap();
    page.name = "Renamed".into();
    page.caption = "Renamed tab".into();
    // Delete the selected last page and its TextBox.
    design
        .controls
        .retain(|c| c.id != 33 && c.parent != Some(33));
    forms::sync_multipage_tabs(&before.design, &mut design).unwrap();
    let synced = tabs(&design);
    // The internal tab name stays; the caption follows the Page.
    assert_eq!(
        synced
            .tabs
            .iter()
            .map(|t| (t.name.as_str(), t.caption.as_str()))
            .collect::<Vec<_>>()[1],
        ("SecondTab", "Renamed tab")
    );
    assert_eq!(synced.tabs.len(), 2);
    assert_eq!(synced.selected, 1, "the page before the deleted one");
    assert_eq!(reopen(&before, &design), design);
}

#[test]
fn new_or_reordered_pages_are_refused() {
    let before = original(0);
    let mut added = before.design.clone();
    let mut page = added.controls.iter().find(|c| c.id == 33).unwrap().clone();
    page.id = 34;
    page.name = "Extra".into();
    added.controls.push(page);
    let error = forms::sync_multipage_tabs(&before.design, &mut added).unwrap_err();
    assert!(error.message.contains("new MultiPage page"), "{error:?}");

    let mut reordered = before.design.clone();
    let first = reordered.controls.iter().position(|c| c.id == 31).unwrap();
    let block = reordered
        .controls
        .drain(first..first + 2)
        .collect::<Vec<_>>();
    reordered.controls.extend(block);
    let error = forms::sync_multipage_tabs(&before.design, &mut reordered).unwrap_err();
    assert!(error.message.contains("page order"), "{error:?}");
}

#[test]
fn an_unsynchronized_page_caption_is_refused_rather_than_written() {
    // A caption lives in the tab; without sync_multipage_tabs the tab is stale.
    let before = original(0);
    let mut design = before.design.clone();
    design
        .controls
        .iter_mut()
        .find(|c| c.id == 31)
        .unwrap()
        .caption = "Stale".into();
    assert!(forms::encode_export(&before, &design, &before.source, &Limits::default()).is_err());
}

/// The internal TabStrip record from an export's MultiPage object stream,
/// split into (mask, fields by bit, extra data, tail).
fn strip_record(out: &[Resource]) -> (u32, std::collections::BTreeMap<u8, u32>, Vec<u8>) {
    use std::io::{Cursor, Read};
    let mut file = cfb::CompoundFile::open(Cursor::new(out[1].bytes[4..].to_vec())).unwrap();
    let mut o = Vec::new();
    file.open_stream("/i20/o")
        .unwrap()
        .read_to_end(&mut o)
        .unwrap();
    let n = u16::from_le_bytes([o[2], o[3]]) as usize;
    let block = &o[4..4 + n];
    let mask = u32::from_le_bytes(block[..4].try_into().unwrap());
    let mut at = 4;
    let mut fields = std::collections::BTreeMap::new();
    for (bit, size) in [
        (0u8, 4usize),
        (1, 4),
        (2, 4),
        (5, 4),
        (6, 1),
        (8, 4),
        (9, 4),
        (11, 4),
        (12, 4),
        (15, 4),
        (17, 4),
        (18, 4),
        (20, 4),
        (21, 4),
        (22, 4),
        (23, 4),
    ] {
        if mask & (1 << bit) != 0 {
            at += (size - at % size) % size;
            let mut v = [0u8; 4];
            v[..size].copy_from_slice(&block[at..at + size]);
            fields.insert(bit, u32::from_le_bytes(v));
            at += size;
        }
    }
    at += (4 - at % 4) % 4;
    let mut all = fields.clone();
    all.insert(255, mask);
    (mask, all, block[at..].to_vec())
}

#[test]
fn tab_strip_rewrite_keeps_fields_it_does_not_own() {
    let before = original(0);
    let untouched =
        forms::encode_export(&before, &before.design, &before.source, &Limits::default()).unwrap();
    let (_, original_fields, original_extra) = strip_record(&untouched);

    // Rename one page: the other tabs' strings keep their exact bytes, and
    // TabsAllocated (tabs inserted over the control's lifetime) is unchanged.
    let mut renamed = before.design.clone();
    renamed
        .controls
        .iter_mut()
        .find(|c| c.id == 31)
        .unwrap()
        .name = "Renamed".into();
    forms::sync_multipage_tabs(&before.design, &mut renamed).unwrap();
    let out = forms::encode_export(&before, &renamed, &before.source, &Limits::default()).unwrap();
    let (_, fields, extra) = strip_record(&out);
    assert_eq!(fields.get(&20), original_fields.get(&20));
    let second = original_extra
        .windows(b"SecondTab".len())
        .position(|w| w == b"SecondTab")
        .map(|_| ())
        .or_else(|| {
            let wide = "SecondTab"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            original_extra
                .windows(wide.len())
                .position(|w| w == wide)
                .map(|_| ())
        });
    assert!(second.is_some(), "fixture tab name");
    for name in ["SecondTab", "BlockedTab"] {
        let narrow = name.as_bytes();
        let wide = name
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let in_original = |needle: &[u8]| original_extra.windows(needle.len()).any(|w| w == needle);
        let in_new = |needle: &[u8]| extra.windows(needle.len()).any(|w| w == needle);
        assert_eq!(in_original(narrow), in_new(narrow), "{name}");
        assert_eq!(in_original(&wide), in_new(&wide), "{name}");
    }

    // Delete every page: TabsAllocated stays, and TabData (default 0) is omitted.
    let mut empty = before.design.clone();
    empty
        .controls
        .retain(|c| !matches!(c.id, 31..=33) && !matches!(c.parent, Some(31..=33)));
    forms::sync_multipage_tabs(&before.design, &mut empty).unwrap();
    let out = forms::encode_export(&before, &empty, &before.source, &Limits::default()).unwrap();
    let (mask, fields, _) = strip_record(&out);
    assert_eq!(fields.get(&20), original_fields.get(&20));
    assert_eq!(mask & (1 << 22), 0);
}

#[test]
fn an_unrelated_edit_keeps_each_tabs_own_state() {
    // A Page hidden by its own site while its tab stays visible decodes as
    // hidden; editing something else must not copy that into the tab.
    let mut before = original(0);
    before
        .design
        .controls
        .iter_mut()
        .find(|c| c.id == 31)
        .unwrap()
        .visible = false;
    let mut design = before.design.clone();
    design
        .controls
        .iter_mut()
        .find(|c| c.id == 11)
        .unwrap()
        .caption = "Other".into();
    forms::sync_multipage_tabs(&before.design, &mut design).unwrap();
    assert_eq!(tabs(&design), tabs(&before.design));
    assert!(tabs(&design).tabs[0].visible);
}

#[test]
fn a_page_cannot_take_the_internal_tabstrip_name() {
    let before = original(0);
    let mut design = before.design.clone();
    design
        .controls
        .iter_mut()
        .find(|c| c.id == 31)
        .unwrap()
        .name = "InternalTabs".into();
    forms::sync_multipage_tabs(&before.design, &mut design).unwrap();
    let error =
        forms::encode_export(&before, &design, &before.source, &Limits::default()).unwrap_err();
    assert!(error.message.contains("internal TabStrip"), "{error:?}");
}

/// The elements of one TabStrip array (captions bit 5, tips 15, names 17),
/// each as its exact bytes.
fn array(out: &[Resource], bit: u8) -> Vec<Vec<u8>> {
    let (mask, fields, extra) = strip_record(out);
    let mut at = 8;
    for b in [5u8, 15, 17, 21, 23] {
        if mask & (1 << b) == 0 {
            continue;
        }
        let len = fields[&b] as usize;
        if b == bit {
            let bytes = &extra[at..at + len];
            let mut items = Vec::new();
            let mut i = 0;
            while i < bytes.len() {
                let count = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
                let chars = (count & 0x7fff_ffff) as usize;
                let n = if count & 0x8000_0000 != 0 {
                    chars
                } else {
                    chars * 2
                };
                let end = i + 4 + n + (4 - n % 4) % 4;
                items.push(bytes[i..end].to_vec());
                i = end;
            }
            return items;
        }
        at += len;
    }
    Vec::new()
}

#[test]
fn each_tab_keeps_its_own_string_bytes_and_empty_strings_are_zero() {
    let before = original(0);
    let untouched =
        forms::encode_export(&before, &before.design, &before.source, &Limits::default()).unwrap();
    let tips = array(&untouched, 15);
    // Give the first page the second page's tooltip and clear its caption.
    let mut design = before.design.clone();
    let second_tip = design
        .controls
        .iter()
        .find(|c| c.id == 32)
        .unwrap()
        .control_tip_text
        .clone();
    assert!(!second_tip.is_empty());
    let first = design.controls.iter_mut().find(|c| c.id == 31).unwrap();
    first.control_tip_text.clone_from(&second_tip);
    first.caption.clear();
    forms::sync_multipage_tabs(&before.design, &mut design).unwrap();
    let out = forms::encode_export(&before, &design, &before.source, &Limits::default()).unwrap();
    let after = array(&out, 15);
    // The second tab keeps its own bytes; the first gets a fresh, compressed copy.
    assert_eq!(after[1], tips[1]);
    assert_eq!(after[0][3] & 0x80, 0x80);
    assert_eq!(&after[0][4..4 + second_tip.len()], second_tip.as_bytes());
    // An empty caption is a zero count word without the compression flag.
    assert_eq!(array(&out, 5)[0], [0, 0, 0, 0]);
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
}

#[test]
fn optional_tab_arrays_stay_when_only_empty_entries_remain() {
    // As Excel for Mac writes them: an array it persisted stays, with zero
    // counts, after the Pages carrying its only values are deleted.
    let before = original(0);
    let untouched =
        forms::encode_export(&before, &before.design, &before.source, &Limits::default()).unwrap();
    let (before_mask, _, _) = strip_record(&untouched);
    let mut design = before.design.clone();
    design
        .controls
        .retain(|c| !matches!(c.id, 31 | 32) && !matches!(c.parent, Some(31 | 32)));
    let remaining = design.controls.iter().find(|c| c.id == 33).unwrap();
    assert!(remaining.control_tip_text.is_empty() && remaining.tag.is_empty());
    forms::sync_multipage_tabs(&before.design, &mut design).unwrap();
    let out = forms::encode_export(&before, &design, &before.source, &Limits::default()).unwrap();
    let (mask, _, _) = strip_record(&out);
    for bit in [15, 21] {
        assert_eq!(mask & (1 << bit), before_mask & (1 << bit), "bit {bit}");
    }
    assert_eq!(array(&out, 15), [vec![0, 0, 0, 0]]);
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
}
