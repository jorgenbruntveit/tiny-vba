//! Bounded native designer serialization. No IO, execution or save acknowledgment.
use super::{Design, Format, Kind, MAX_RESOURCE_BYTES, NativeForm, Resource};
use crate::{Limits, Reader, Result, budget, container_error, corrupt, text, unsupported};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};

const FORM: &[(u8, usize)] = &[
    (1, 4),
    (2, 4),
    (3, 4),
    (6, 4),
    (7, 1),
    (8, 1),
    (9, 1),
    (13, 4),
    (16, 1),
    (17, 1),
    (18, 4),
    (19, 4),
    (20, 2),
    (22, 4),
    (26, 4),
    (27, 4),
];
const SITE: &[(u8, usize)] = &[
    (0, 4),
    (1, 4),
    (2, 4),
    (3, 4),
    (4, 4),
    (5, 4),
    (6, 2),
    (7, 2),
    (9, 2),
    (11, 4),
    (13, 4),
    (14, 4),
];
const MORPH: &[(u8, usize)] = &[
    (0, 4),
    (1, 4),
    (2, 4),
    (3, 4),
    (4, 1),
    (5, 1),
    (6, 1),
    (7, 1),
    (9, 2),
    (11, 2),
    (12, 2),
    (13, 2),
    (14, 2),
    (16, 1),
    (17, 1),
    (18, 1),
    (20, 1),
    (21, 1),
    (22, 4),
    (23, 4),
    (25, 4),
    (26, 4),
    (29, 2),
    (32, 4),
];
const LABEL: &[(u8, usize)] = &[
    (0, 4),
    (1, 4),
    (2, 4),
    (3, 4),
    (4, 4),
    (6, 1),
    (7, 4),
    (8, 2),
    (9, 2),
    (11, 2),
];
const BUTTON: &[(u8, usize)] = &[(0, 4), (1, 4), (2, 4), (3, 4), (4, 4), (6, 1), (8, 2)];
/// MS-OFORMS 2.2.9 TabStripPropMask fields that carry data.
const TABSTRIP: &[(u8, usize)] = &[
    (0, 4),
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
];
/// MS-OFORMS 2.2.6 MultiPageProperties: PageCount and the TabStrip ID.
const MULTIPAGE: &[(u8, usize)] = &[(1, 4), (2, 4)];
struct Record<'a> {
    version: u16,
    mask: u64,
    wide: bool,
    fields: BTreeMap<u8, u32>,
    /// The record's ExtraDataBlock as read; `encode` takes a new one.
    extra: &'a [u8],
    tail: &'a [u8],
}
fn align(bytes: &mut Vec<u8>, n: usize) {
    while !bytes.len().is_multiple_of(n) {
        bytes.push(0);
    }
}
fn skip(r: &mut Reader<'_>, n: usize) -> Result<()> {
    r.take((n - r.pos % n) % n)?;
    Ok(())
}
impl<'a> Record<'a> {
    fn read(bytes: &'a [u8], wide: bool, schema: &[(u8, usize)]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let version = r.u16()?;
        let n = r.u16()? as usize;
        let block = r.take(n)?;
        let mut b = Reader::new(block);
        let mut mask = u64::from(b.u32()?);
        if wide {
            mask |= u64::from(b.u32()?) << 32;
        }
        let mut fields = BTreeMap::new();
        for &(bit, size) in schema {
            if mask & (1u64 << bit) != 0 {
                skip(&mut b, size)?;
                let v = match size {
                    1 => u32::from(b.u8()?),
                    2 => u32::from(b.u16()?),
                    4 => b.u32()?,
                    _ => return Err(corrupt("encoder field width")),
                };
                fields.insert(bit, v);
            }
        }
        skip(&mut b, 4)?;
        Ok(Self {
            version,
            mask,
            wide,
            fields,
            extra: block.get(b.pos..).ok_or_else(budget)?,
            tail: r.take(bytes.len() - r.pos)?,
        })
    }
    fn value(&self, bit: u8, default: u32) -> u32 {
        self.fields.get(&bit).copied().unwrap_or(default)
    }
    fn set(&mut self, bit: u8, value: u32) {
        self.mask |= 1u64 << bit;
        self.fields.insert(bit, value);
    }
    fn encode(&self, schema: &[(u8, usize)], extra: &[u8], tail: &[u8]) -> Result<Vec<u8>> {
        let mut b = (self.mask as u32).to_le_bytes().to_vec();
        if self.wide {
            b.extend(((self.mask >> 32) as u32).to_le_bytes());
        }
        for &(bit, size) in schema {
            if self.mask & (1u64 << bit) != 0 {
                align(&mut b, size);
                let v = self.value(bit, 0).to_le_bytes();
                b.extend(v.get(..size).ok_or_else(budget)?);
            }
        }
        align(&mut b, 4);
        b.extend(extra);
        let n = u16::try_from(b.len()).map_err(|_| budget())?;
        let mut out = self.version.to_le_bytes().to_vec();
        out.extend(n.to_le_bytes());
        out.extend(b);
        out.extend(tail);
        if out.len() > MAX_RESOURCE_BYTES {
            return Err(budget());
        }
        Ok(out)
    }
}
fn string(value: &str) -> Result<(u32, Vec<u8>)> {
    if value.contains('\0') || value.encode_utf16().count() > 8192 {
        return Err(budget());
    }
    let mut bytes = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let n = u32::try_from(bytes.len()).map_err(|_| budget())?;
    align(&mut bytes, 4);
    Ok((n, bytes))
}
fn pair(a: i32, b: i32) -> Vec<u8> {
    [a.to_le_bytes(), b.to_le_bytes()].concat()
}
fn fields_string(r: &mut Record<'_>, bit: u8, value: &str, out: &mut Vec<u8>) -> Result<()> {
    let (n, b) = string(value)?;
    r.set(bit, n);
    out.extend(b);
    Ok(())
}
fn leaf(raw: &[u8], c: &super::Control) -> Result<Vec<u8>> {
    let morph = matches!(
        c.kind,
        Kind::TextBox
            | Kind::ComboBox
            | Kind::ListBox
            | Kind::CheckBox
            | Kind::OptionButton
            | Kind::ToggleButton
            | Kind::RefEdit
    );
    let schema = if morph {
        MORPH
    } else if c.kind == Kind::Label {
        LABEL
    } else if c.kind == Kind::CommandButton {
        BUTTON
    } else {
        return Err(unsupported("native edited control kind"));
    };
    let mut r = Record::read(raw, morph, schema)?;
    let mut extra = Vec::new();
    if morph {
        extra.extend(pair(c.width, c.height));
        fields_string(&mut r, 22, &c.text, &mut extra)?;
        if r.mask & (1 << 23) != 0 || !c.caption.is_empty() {
            fields_string(&mut r, 23, &c.caption, &mut extra)?;
        }
        if r.mask & (1u64 << 32) != 0 {
            fields_string(&mut r, 32, &c.group_name, &mut extra)?;
        }
    } else {
        fields_string(&mut r, 3, &c.caption, &mut extra)?;
        extra.extend(pair(c.width, c.height));
    }
    let bit = if morph { 0 } else { 2 };
    let default = if morph {
        0x2c80081b
    } else if c.kind == Kind::Label {
        0x80001b
    } else {
        0x1b
    };
    let flags = r.value(bit, default);
    r.set(bit, (flags & !2) | if c.enabled { 2 } else { 0 });
    r.encode(schema, &extra, r.tail)
}
fn site(raw: &[u8], c: &super::Control, bytes: usize) -> Result<Vec<u8>> {
    let mut r = Record::read(raw, false, SITE)?;
    let mut extra = Vec::new();
    fields_string(&mut r, 0, &c.name, &mut extra)?;
    if r.mask & 2 != 0 {
        fields_string(&mut r, 1, &c.tag, &mut extra)?;
    }
    r.mask |= 1 << 8;
    extra.extend(pair(c.left, c.top));
    for (bit, s) in [
        (11, &c.control_tip_text),
        (13, &c.control_source),
        (14, &c.row_source),
    ] {
        if r.mask & (1 << bit) != 0 {
            fields_string(&mut r, bit, s, &mut extra)?;
        }
    }
    let flags = r.value(4, 0x33);
    r.set(
        4,
        (flags & !3) | (u32::from(c.visible) * 2) | u32::from(c.tab_stop),
    );
    r.set(5, u32::try_from(bytes).map_err(|_| budget())?);
    r.set(6, u32::from(c.tab_index as u16));
    r.encode(SITE, &extra, &[])
}
/// A site record with only its object stream size changed.
fn with_object_size(raw: &[u8], bytes: usize) -> Result<Vec<u8>> {
    let r = Record::read(raw, false, SITE)?;
    if r.mask & (1 << 5) == 0 {
        return Err(corrupt("native site without object data"));
    }
    // Bits 0..=4 are all four-byte fields, so the size field's offset is fixed.
    let at = 8 + 4 * (0..5).filter(|bit| r.mask & (1 << bit) != 0).count();
    let mut out = raw.to_vec();
    out.get_mut(at..at + 4)
        .ok_or_else(|| corrupt("native site size"))?
        .copy_from_slice(&u32::try_from(bytes).map_err(|_| budget())?.to_le_bytes());
    Ok(out)
}
/// One ArrayString (MS-OFORMS 2.4.14.1): a character count with the
/// compression flag, then the text, padded to four bytes. Text whose
/// characters all fit in one byte is stored compressed.
fn array_string(value: &str) -> Result<Vec<u8>> {
    if value.contains('\0') || value.encode_utf16().count() > 8192 {
        return Err(budget());
    }
    if value.is_empty() {
        // An empty string has a zero count and no compression flag.
        return Ok(0u32.to_le_bytes().to_vec());
    }
    let narrow = value.chars().all(|c| u32::from(c) < 0x100);
    let (count, mut bytes) = if narrow {
        let bytes = value
            .chars()
            .map(|c| u32::from(c) as u8)
            .collect::<Vec<_>>();
        (
            u32::try_from(bytes.len()).map_err(|_| budget())? | 0x8000_0000,
            bytes,
        )
    } else {
        let units = value.encode_utf16().collect::<Vec<_>>();
        (
            u32::try_from(units.len()).map_err(|_| budget())?,
            units.iter().flat_map(|u| u.to_le_bytes()).collect(),
        )
    };
    align(&mut bytes, 4);
    let mut out = count.to_le_bytes().to_vec();
    out.extend(bytes);
    Ok(out)
}
/// The ArrayStrings of one TabStrip array, each with its exact bytes.
fn array_strings(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let mut r = Reader::new(bytes);
    let mut out = Vec::new();
    while r.pos < bytes.len() {
        if out.len() >= super::MAX_CONTROLS {
            return Err(budget());
        }
        let start = r.pos;
        let count = r.u32()?;
        let chars = (count & 0x7fff_ffff) as usize;
        let value = if count & 0x8000_0000 != 0 {
            r.take(chars)?.iter().map(|b| char::from(*b)).collect()
        } else {
            text::unicode(r.take(chars.checked_mul(2).ok_or_else(budget)?)?)?
        };
        skip(&mut r, 4)?;
        out.push((value, bytes.get(start..r.pos).ok_or_else(budget)?.to_vec()));
    }
    Ok(out)
}
/// A site record's Name (the first string of its extra data).
fn site_name(site: &Record<'_>) -> Result<String> {
    let count = site.value(0, 0);
    let n = (count & 0x7fff_ffff) as usize;
    let bytes = site
        .extra
        .get(..n)
        .ok_or_else(|| corrupt("native site name"))?;
    if count & 0x8000_0000 != 0 {
        Ok(bytes.iter().map(|b| char::from(*b)).collect())
    } else {
        text::unicode(bytes)
    }
}
/// Bring each MultiPage's tab list in line with its pages after designer
/// edits. A Page's caption, tag, tooltip and state persist in its
/// MultiPage's internal TabStrip; internal tab names, accelerators and the
/// selected tab follow their pages. Adding or reordering pages is refused.
pub fn sync_multipage_tabs(original: &Design, design: &mut Design) -> Result<()> {
    let multipages = design
        .controls
        .iter()
        .filter(|c| c.kind == Kind::MultiPage)
        .map(|c| c.id)
        .collect::<Vec<_>>();
    for id in multipages {
        let pages_of = |d: &Design| {
            d.controls
                .iter()
                .filter(|c| c.parent == Some(id) && c.kind == Kind::Page)
                .cloned()
                .collect::<Vec<_>>()
        };
        let Some(before) = original.controls.iter().find(|c| c.id == id) else {
            continue;
        };
        let behavior = before
            .tabs
            .as_ref()
            .ok_or_else(|| corrupt("MultiPage tabs"))?;
        let (old_pages, new_pages) = (pages_of(original), pages_of(design));
        if behavior.tabs.len() != old_pages.len() {
            return Err(corrupt("MultiPage page/tab count"));
        }
        let mut tabs = Vec::new();
        let mut kept = Vec::new();
        for page in &new_pages {
            let at = old_pages
                .iter()
                .position(|p| p.id == page.id)
                .ok_or_else(|| unsupported("new MultiPage page"))?;
            if kept.last().is_some_and(|last| at <= *last) {
                return Err(unsupported("MultiPage page order edit"));
            }
            kept.push(at);
            let old = behavior.tabs.get(at).ok_or_else(budget)?;
            let was = old_pages.get(at).ok_or_else(budget)?;
            // A tab's own state changes only when its page's state is edited.
            let state = |now: bool, before: bool, tab: bool| if now == before { tab } else { now };
            tabs.push(super::Tab {
                // The internal tab name (Office writes Tab1, Tab2, ...) is not
                // the Page's name and does not follow a Page rename.
                name: old.name.clone(),
                caption: page.caption.clone(),
                tag: page.tag.clone(),
                tooltip: page.control_tip_text.clone(),
                accelerator: old.accelerator.clone(),
                visible: state(page.visible, was.visible, old.visible),
                enabled: state(page.enabled, was.enabled, old.enabled),
            });
        }
        // The selected tab follows its page; if it was deleted, the page that
        // took its place (or the new last page) is selected.
        let selected = usize::try_from(behavior.selected)
            .ok()
            .filter(|s| *s < old_pages.len())
            .map_or(behavior.selected, |old| {
                kept.iter()
                    .position(|k| *k == old)
                    .or_else(|| {
                        (!kept.is_empty()).then(|| {
                            kept.iter()
                                .filter(|k| **k < old)
                                .count()
                                .min(kept.len() - 1)
                        })
                    })
                    .map_or(-1, |i| i as i32)
            });
        let multipage = design
            .controls
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(budget)?;
        let mut next = behavior.clone();
        next.tabs = tabs;
        next.selected = selected;
        multipage.tabs = Some(next);
    }
    Ok(())
}
/// Re-encode a MultiPage's internal TabStrip with the MultiPage's tabs.
/// `previous[i]` is the index tab `i` had before. A string an edit did not
/// change keeps that tab's exact bytes, and fields this writer does not own,
/// such as TabsAllocated, are kept as read.
fn tab_strip(raw: &[u8], behavior: &super::TabBehavior, previous: &[usize]) -> Result<Vec<u8>> {
    let mut r = Record::read(raw, false, TABSTRIP)?;
    let flag_bytes = (r.value(22, 0) as usize)
        .checked_mul(4)
        .ok_or_else(budget)?;
    let props = r
        .tail
        .len()
        .checked_sub(flag_bytes)
        .and_then(|n| r.tail.get(..n))
        .ok_or_else(|| corrupt("TabStrip tab flags"))?;
    // ExtraDataBlock: Size, then the arrays in property order.
    let mut old = Reader::new(r.extra);
    let mut extra = old.take(8)?.to_vec();
    let tabs = &behavior.tabs;
    let arrays: [(u8, Vec<&str>); 5] = [
        (5, tabs.iter().map(|t| t.caption.as_str()).collect()),
        (15, tabs.iter().map(|t| t.tooltip.as_str()).collect()),
        (17, tabs.iter().map(|t| t.name.as_str()).collect()),
        (21, tabs.iter().map(|t| t.tag.as_str()).collect()),
        (23, tabs.iter().map(|t| t.accelerator.as_str()).collect()),
    ];
    for (bit, values) in arrays {
        let before = if r.mask & (1u64 << bit) != 0 {
            array_strings(old.take(r.value(bit, 0) as usize)?)?
        } else {
            Vec::new()
        };
        // Captions and names are always persisted. Office keeps an optional
        // array it wrote even when every entry is empty; a new one is written
        // only when some entry is not.
        let keep = matches!(bit, 5 | 17)
            || r.mask & (1u64 << bit) != 0
            || values.iter().any(|v| !v.is_empty());
        if keep && !tabs.is_empty() {
            let mut bytes = Vec::new();
            for (i, value) in values.into_iter().enumerate() {
                match previous
                    .get(i)
                    .and_then(|at| before.get(*at))
                    .filter(|(v, _)| v == value)
                {
                    Some((_, exact)) => bytes.extend(exact),
                    None => bytes.extend(array_string(value)?),
                }
            }
            r.set(bit, u32::try_from(bytes.len()).map_err(|_| budget())?);
            extra.extend(bytes);
        } else {
            r.mask &= !(1u64 << bit);
            r.fields.remove(&bit);
        }
    }
    old.end()?;
    if behavior.selected < 0 {
        r.mask &= !1;
        r.fields.remove(&0);
    } else {
        r.set(0, behavior.selected as u32);
    }
    let states = tabs
        .iter()
        .map(|t| u32::from(t.visible) | (u32::from(t.enabled) << 1))
        .collect::<Vec<_>>();
    let mut tail = props.to_vec();
    // TabData's file format default is zero, which is not persisted.
    if !tabs.is_empty() && (flag_bytes > 0 || states.iter().any(|f| *f != 3)) {
        r.set(22, u32::try_from(tabs.len()).map_err(|_| budget())?);
        for state in states {
            tail.extend(state.to_le_bytes());
        }
    } else {
        r.mask &= !(1u64 << 22);
        r.fields.remove(&22);
    }
    r.encode(TABSTRIP, &extra, &tail)
}
/// A MultiPage's `x` stream with only the `kept` pages, in their tab order.
fn multipage_pages(raw: &[u8], before: &[u32], kept: &[u32]) -> Result<Vec<u8>> {
    let mut r = Reader::new(raw);
    let block = |r: &mut Reader<'_>| -> Result<Vec<u8>> {
        let start = r.pos;
        r.u16()?;
        let n = r.u16()? as usize;
        r.take(n)?;
        Ok(raw.get(start..r.pos).ok_or_else(budget)?.to_vec())
    };
    // One extra, ignored PageProperties record precedes the real pages.
    let mut pages = Vec::new();
    for _ in 0..=before.len() {
        pages.push(block(&mut r)?);
    }
    let properties = block(&mut r)?;
    let mut ids = Vec::new();
    for _ in before {
        ids.push(r.u32()?);
    }
    r.end()?;
    if ids != before {
        return Err(corrupt("MultiPage page ID/storage mismatch"));
    }
    let mut out = pages.first().ok_or_else(budget)?.clone();
    for id in kept {
        let at = before
            .iter()
            .position(|b| b == id)
            .ok_or_else(|| unsupported("new MultiPage page"))?;
        out.extend(pages.get(at + 1).ok_or_else(budget)?);
    }
    let mut record = Record::read(&properties, false, MULTIPAGE)?;
    record.set(1, u32::try_from(kept.len()).map_err(|_| budget())?);
    out.extend(record.encode(MULTIPAGE, &[], record.tail)?);
    for id in kept {
        out.extend(id.to_le_bytes());
    }
    Ok(out)
}
fn split_sites<'a>(r: &mut Reader<'a>) -> Result<Vec<&'a [u8]>> {
    let n = r.u32()? as usize;
    if n > super::MAX_CONTROLS {
        return Err(budget());
    }
    let len = r.u32()? as usize;
    let mut sites = Reader::new(r.take(len)?);
    let mut count = 0;
    while count < n {
        if sites.u8()? != 0 {
            return Err(unsupported("native site depth"));
        }
        let k = sites.u8()?;
        let repeat = if k & 128 != 0 {
            if sites.u8()? != 1 {
                return Err(corrupt("native site type"));
            }
            usize::from(k & 127)
        } else {
            if k != 1 {
                return Err(corrupt("native site type"));
            }
            1
        };
        if repeat == 0 {
            return Err(corrupt("native site count"));
        }
        count += repeat;
        if count > n {
            return Err(corrupt("native site count"));
        }
    }
    skip(&mut sites, 4)?;
    let mut out = Vec::new();
    for _ in 0..n {
        let start = sites.pos;
        sites.u16()?;
        let n = sites.u16()? as usize;
        sites.take(n)?;
        out.push(sites.bytes.get(start..sites.pos).ok_or_else(budget)?);
    }
    sites.end()?;
    Ok(out)
}
/// Where a container's site records start in its `f` stream tail.
fn sites_start(record: &Record<'_>) -> Result<usize> {
    let mut tail = Reader::new(record.tail);
    if record.mask & (1 << 20) != 0 {
        super::binary::skip_font(&mut tail)?;
    }
    if record.value(6, 4) & 0x8000 == 0 {
        super::binary::skip_classes(&mut tail)?;
    }
    Ok(tail.pos)
}
/// The lowest control ID a designer may give a new control on `original`: above
/// every site ID stored in every container, including sites the public design
/// does not show (a MultiPage's internal TabStrip), and above each container's
/// NextAvailableID, the largest ID used so far.
pub fn next_control_id(original: &NativeForm, limits: &Limits) -> Result<u32> {
    limits.validate()?;
    let (_, resources) = resource_streams(original, limits)?;
    let mut next = 1u32;
    // Stream names compare case-insensitively, as the decoder resolves them.
    for r in resources
        .iter()
        .filter(|r| r.name.to_ascii_lowercase().ends_with("/f"))
    {
        let record = Record::read(&r.bytes, false, FORM)?;
        // NextAvailableID is the largest ID used so far (MS-OFORMS 2.5.55).
        next = next.max(record.value(3, 0).checked_add(1).ok_or_else(budget)?);
        let mut tail = Reader::new(record.tail);
        tail.pos = sites_start(&record)?;
        for site in split_sites(&mut tail)? {
            let id = Record::read(site, false, SITE)?.value(2, 0);
            next = next.max(id.checked_add(1).ok_or_else(budget)?);
        }
    }
    Ok(next)
}
fn rewrite(
    resources: &mut Vec<Resource>,
    path: &str,
    old: &Design,
    new: &Design,
    parent: Option<u32>,
    largest_id: u32,
    depth: usize,
) -> Result<()> {
    if depth > 8 {
        return Err(budget());
    }
    let f = format!("{path}/f");
    let o = format!("{path}/o");
    let raw = resources
        .iter()
        .find(|r| r.name == f)
        .ok_or_else(|| corrupt("missing native form stream"))?
        .bytes
        .clone();
    let mut record = Record::read(&raw, false, FORM)?;
    let mut tail = Reader::new(record.tail);
    tail.pos = sites_start(&record)?;
    let flags = record.value(6, 4);
    let prefix = record.tail.get(..tail.pos).ok_or_else(budget)?.to_vec();
    let sites = split_sites(&mut tail)?;
    let suffix = record.tail.get(tail.pos..).ok_or_else(budget)?;
    let old_o = resources
        .iter()
        .find(|r| r.name == o)
        .ok_or_else(|| corrupt("missing native object stream"))?
        .bytes
        .clone();
    let mut objects = Reader::new(&old_o);
    // A MultiPage's pages are listed in its `x` stream, and each Page's name,
    // caption, tag, tooltip and state also live in its internal TabStrip.
    let multipage = parent.is_some_and(|id| {
        old.controls
            .iter()
            .any(|c| c.id == id && c.kind == Kind::MultiPage)
    });
    fn pages_of(design: &Design, parent: Option<u32>) -> Vec<&super::Control> {
        design
            .controls
            .iter()
            .filter(|c| c.parent == parent && c.kind == Kind::Page)
            .collect()
    }
    let old_pages = pages_of(old, parent)
        .iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    let pages = pages_of(new, parent)
        .iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    // Pages can be deleted, not added or reordered.
    let mut last = None;
    for id in &pages {
        let at = old_pages
            .iter()
            .position(|p| p == id)
            .ok_or_else(|| unsupported("new MultiPage page"))?;
        if last.is_some_and(|last| at <= last) {
            return Err(unsupported("MultiPage page order edit"));
        }
        last = Some(at);
    }
    let pages_removed = pages.len() != old_pages.len();
    let tabs = |d: &Design| {
        parent
            .and_then(|id| d.controls.iter().find(|c| c.id == id))
            .and_then(|c| c.tabs.clone())
    };
    let new_tabs = tabs(new);
    let tabs_changed = multipage && (pages_removed || tabs(old) != new_tabs);
    let mut records = Vec::new();
    let mut count = 0usize;
    for raw_site in sites {
        let s = Record::read(raw_site, false, SITE)?;
        let id = s.value(2, 0);
        let bytes = s.value(5, 0) as usize;
        let data = objects.take(bytes)?;
        let old_control = old
            .controls
            .iter()
            .find(|c| c.id == id && c.parent == parent);
        // MultiPage's private TabStrip is not part of the public designer.
        let Some(before) = old_control else {
            if multipage {
                let private = site_name(&s)?;
                if !private.is_empty()
                    && new
                        .controls
                        .iter()
                        .any(|c| c.parent == parent && c.name.eq_ignore_ascii_case(&private))
                {
                    return Err(unsupported(
                        "a Page name used by the MultiPage's internal TabStrip",
                    ));
                }
            }
            if tabs_changed {
                let behavior = new_tabs.as_ref().ok_or_else(|| corrupt("MultiPage tabs"))?;
                let previous = pages
                    .iter()
                    .map(|id| old_pages.iter().position(|p| p == id).ok_or_else(budget))
                    .collect::<Result<Vec<_>>>()?;
                let data = tab_strip(data, behavior, &previous)?;
                records.push((id, with_object_size(raw_site, data.len())?, data));
            } else {
                records.push((id, raw_site.to_vec(), data.to_vec()));
            }
            count += 1;
            continue;
        };
        let Some(after) = new
            .controls
            .iter()
            .find(|c| c.id == id && c.parent == parent)
        else {
            let prefix = format!("{path}/i{id:02}/");
            resources.retain(|r| !r.name.starts_with(&prefix));
            continue;
        };
        if before.kind != after.kind {
            return Err(unsupported("native control kind edit"));
        }
        let data = if matches!(before.kind, Kind::Frame | Kind::MultiPage | Kind::Page) {
            let child_path = format!("{path}/i{id:02}");
            rewrite(
                resources,
                &child_path,
                old,
                new,
                Some(id),
                largest_id,
                depth + 1,
            )?;
            Vec::new()
        } else if before.caption == after.caption
            && before.text == after.text
            && before.width == after.width
            && before.height == after.height
            && before.enabled == after.enabled
        {
            data.to_vec()
        } else {
            leaf(data, after)?
        };
        records.push((id, site(raw_site, after, data.len())?, data));
        count += 1;
    }
    objects.end()?;
    for c in new
        .controls
        .iter()
        .filter(|c| c.parent == parent && !old.controls.iter().any(|o| o.id == c.id))
    {
        let class = match c.kind {
            Kind::Label => 21,
            Kind::TextBox => 23,
            Kind::CommandButton => 17,
            _ => return Err(unsupported("new native control resource kind")),
        };
        let raw = super::binary::new_control_record(c.kind)?;
        let data = leaf(&raw, c)?;
        let mut fields = BTreeMap::new();
        fields.insert(2, c.id);
        fields.insert(4, 0x13);
        fields.insert(7, class);
        let record = Record {
            version: 0,
            mask: (1 << 2) | (1 << 4) | (1 << 5) | (1 << 7),
            wide: false,
            fields,
            extra: &[],
            tail: &[],
        };
        let raw = record.encode(SITE, &[], &[])?;
        records.push((c.id, site(&raw, c, data.len())?, data));
        count += 1;
    }
    records.sort_by_key(|(id, _, _)| {
        new.controls
            .iter()
            .position(|c| c.id == *id)
            .map_or(0, |i| i + 1)
    });
    // NextAvailableID is the largest ID used so far (MS-OFORMS 2.5.55).
    let largest = new
        .controls
        .iter()
        .map(|c| c.id)
        .chain(records.iter().map(|(id, _, _)| *id))
        .max()
        .unwrap_or(0)
        .max(largest_id);
    record.set(3, record.value(3, 0).max(largest));
    let mut next_sites = Vec::new();
    let mut next_objects = Vec::new();
    for (_, site, data) in records {
        next_sites.push(site);
        next_objects.extend(data);
    }
    let (caption, width, height, enabled) = if let Some(id) = parent {
        let c = new
            .controls
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| corrupt("missing parent"))?;
        (&c.caption, c.width, c.height, c.enabled)
    } else {
        (&new.caption, new.width, new.height, new.enabled)
    };
    let mut extra = Vec::new();
    record.mask |= 1 << 10;
    extra.extend(pair(width, height));
    if record.mask & (1 << 11) != 0 {
        extra.extend(pair(width, height));
    }
    if record.mask & (1 << 12) != 0 {
        extra.extend(pair(0, 0));
    }
    // A UserForm's caption normally lives only in VBFrame; Office then leaves
    // FormControl.Caption clear, and writing it as well makes Office draw the
    // caption again inside the form. Keep the original representation; nested
    // Frame/Page captions have no VBFrame copy and need the field when set.
    if record.mask & (1 << 19) != 0 || (parent.is_some() && !caption.is_empty()) {
        fields_string(&mut record, 19, caption, &mut extra)?;
    }
    record.set(6, (flags & !4) | if enabled { 4 } else { 0 });
    let mut site_bytes = Vec::new();
    for _ in 0..count {
        site_bytes.extend([0, 1]);
    }
    align(&mut site_bytes, 4);
    for s in next_sites {
        site_bytes.extend(s);
    }
    let mut new_tail = prefix;
    new_tail.extend((count as u32).to_le_bytes());
    new_tail.extend(
        u32::try_from(site_bytes.len())
            .map_err(|_| budget())?
            .to_le_bytes(),
    );
    new_tail.extend(site_bytes);
    new_tail.extend(suffix);
    let encoded = record.encode(FORM, &extra, &new_tail)?;
    let x = format!("{path}/x");
    let pages_x = if multipage && pages_removed {
        let raw = &resources
            .iter()
            .find(|r| r.name == x)
            .ok_or_else(|| corrupt("missing MultiPage page stream"))?
            .bytes;
        Some(multipage_pages(raw, &old_pages, &pages)?)
    } else {
        None
    };
    for r in resources {
        if r.name == f {
            r.bytes = encoded.clone();
        } else if r.name == o {
            r.bytes = next_objects.clone();
        } else if r.name == x
            && let Some(bytes) = &pages_x
        {
            r.bytes = bytes.clone();
        }
    }
    Ok(())
}
fn encode_text(s: &str, cp: u16) -> Result<Vec<u8>> {
    let (enc, _, errors) = text::encoding(cp)?.encode(s);
    if errors {
        return Err(unsupported(
            "text is not representable in the project code page",
        ));
    }
    Ok(enc.into_owned())
}
fn quote(s: &str) -> Result<String> {
    if s.contains(['\r', '\n', '\0']) {
        return Err(unsupported("native frame string"));
    }
    Ok(format!("\"{}\"", s.replace('"', "\"\"")))
}
fn header(original: &str, name: &str, design: &Design, companion: &str) -> Result<String> {
    let parsed = super::frame::parse(original, true)?;
    let mut out = String::new();
    let mut font = false;
    for line in original
        .get(..parsed.source_start)
        .ok_or_else(budget)?
        .split_inclusive('\n')
    {
        let clean = line.trim();
        if clean.starts_with("Begin ") {
            out.push_str(&format!(
                "Begin {{C62A69F0-16DC-11CE-9E98-00AA00574A4F}} {name}\r\n"
            ));
            continue;
        }

        if clean.starts_with("BeginProperty") {
            font = true;
        }
        if clean == "EndProperty" {
            font = false;
            out.push_str(line);
            continue;
        }
        if !font && clean == "End" {
            out.push_str(&format!(" Caption = {}\r\n ClientWidth = {}\r\n ClientHeight = {}\r\n OleObjectBlob = {}:0000\r\nEnd\r\n",quote(&design.caption)?,(i64::from(design.width)*72+63)/127,(i64::from(design.height)*72+63)/127,quote(companion)?));
            break;
        }
        if !font
            && clean.split_once('=').is_some_and(|(key, _)| {
                ["caption", "clientwidth", "clientheight", "oleobjectblob"]
                    .contains(&key.trim().to_ascii_lowercase().as_str())
            })
        {
            continue;
        }
        out.push_str(line);
    }
    Ok(out)
}
/// Serialize a snapshot of an attached native form. Source is never executed.
/// The decoder reopens the result before it is returned; caller IO/commit is separate.
pub fn encode_export(
    original: &NativeForm,
    design: &Design,
    source: &str,
    limits: &Limits,
) -> Result<Vec<Resource>> {
    encode_export_named(original, &original.name, design, source, limits)
}
/// Export a standalone component under its current designer name. Embedded
/// project component identity remains checked by the project writeback boundary.
/// A design whose Pages changed must first go through [`sync_multipage_tabs`];
/// otherwise the reopen check refuses it.
pub fn encode_export_named(
    original: &NativeForm,
    name: &str,
    design: &Design,
    source: &str,
    limits: &Limits,
) -> Result<Vec<Resource>> {
    encode_export_designed(original, name, design, source, 0, limits)
}
/// As [`encode_export_named`], for a designer that has given out control IDs
/// up to `largest_id`. Each container's NextAvailableID (the largest ID used,
/// MS-OFORMS 2.5.55) is at least that, so the IDs of controls deleted since the
/// form was opened, saved in between or not, are never given out again.
pub fn encode_export_designed(
    original: &NativeForm,
    name: &str,
    design: &Design,
    source: &str,
    largest_id: u32,
    limits: &Limits,
) -> Result<Vec<Resource>> {
    limits.validate()?;
    if name.len() > 64
        || name.is_empty()
        || !name
            .chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit())
    {
        return Err(unsupported("native component export name"));
    }
    if original.resources.len() > 512
        || original
            .resources
            .iter()
            .try_fold(0usize, |n, r| n.checked_add(r.bytes.len()))
            .is_none_or(|n| n > 8 * 1024 * 1024)
    {
        return Err(budget());
    }
    if source.len() > limits.source_bytes || design.controls.len() > super::MAX_CONTROLS {
        return Err(budget());
    }
    text::attributes(source, name)?;
    let (frame, mut resources) = resource_streams(original, limits)?;
    rewrite(
        &mut resources,
        "",
        &original.design,
        design,
        None,
        largest_id,
        0,
    )?;
    let mut file = cfb::CompoundFile::create(Cursor::new(Vec::new())).map_err(container_error)?;
    for resource in resources {
        if let Some((parent, _)) = resource.name.rsplit_once('/')
            && !parent.is_empty()
        {
            file.create_storage_all(parent).map_err(container_error)?;
        }
        file.create_stream(&resource.name)
            .map_err(container_error)?
            .write_all(&resource.bytes)
            .map_err(container_error)?;
    }
    if original.storages.len() > 512 {
        return Err(budget());
    }
    for storage in &original.storages {
        if file.exists(&storage.path) {
            file.set_storage_clsid(
                &storage.path,
                storage
                    .clsid
                    .parse()
                    .map_err(|_| corrupt("native storage class ID"))?,
            )
            .map_err(container_error)?;
            file.set_state_bits(&storage.path, storage.state_bits)
                .map_err(container_error)?;
        }
    }
    let bytes = file.into_inner().into_inner();
    if bytes.len() > MAX_RESOURCE_BYTES.min(limits.stream_bytes) {
        return Err(budget());
    }
    let mut companion = (bytes.len() as u32).to_le_bytes().to_vec();
    companion.extend(bytes);
    let primary = format!("{name}.frm");
    let hash = companion.iter().fold(0xcbf29ce484222325u64, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    let frx = format!("{name}.{hash:016x}.frx");
    let mut frm = header(&frame, name, design, &frx)?;
    frm.push_str(source);
    let bytes = encode_text(&frm, original.code_page)?;
    let resources = vec![Resource {
        name: frx,
        bytes: companion,
    }];
    let reopened = super::decode_export(&primary, &bytes, original.code_page, &resources, limits)?;
    if reopened.source != source || reopened.design != *design {
        return Err(corrupt("native encode/reopen snapshot mismatch"));
    }
    let mut out = vec![Resource {
        name: primary,
        bytes,
    }];
    out.extend(resources);
    Ok(out)
}

fn resource_streams(original: &NativeForm, limits: &Limits) -> Result<(String, Vec<Resource>)> {
    Ok(match original.format {
        Format::Embedded => {
            let r = original
                .resources
                .iter()
                .find(|r| r.name.ends_with("/\u{3}VBFrame"))
                .ok_or_else(|| corrupt("missing native VBFrame"))?;
            let path = r.name.strip_suffix("/\u{3}VBFrame").ok_or_else(budget)?;
            (
                text::decode(&r.bytes, original.code_page)?,
                original
                    .resources
                    .iter()
                    .filter(|s| s.name != r.name)
                    .map(|s| Resource {
                        name: s.name.strip_prefix(path).unwrap_or(&s.name).into(),
                        bytes: s.bytes.clone(),
                    })
                    .collect::<Vec<_>>(),
            )
        }
        Format::Export => {
            let r = original
                .resources
                .iter()
                .find(|r| r.name.to_ascii_lowercase().ends_with(".frm"))
                .ok_or_else(|| corrupt("missing original native export"))?;
            let frame = text::decode(&r.bytes, original.code_page)?;
            let h = super::frame::parse(&frame, true)?;
            let Some((name, offset)) = super::frame::blob(&h)? else {
                let fields = BTreeMap::from([(6, 0x8004), (27, 0)]);
                let record = Record {
                    version: 0x0400,
                    mask: (1 << 6) | (1 << 10) | (1 << 27),
                    wide: false,
                    fields,
                    extra: &[],
                    tail: &[],
                };
                let f = record.encode(
                    FORM,
                    &pair(original.design.width, original.design.height),
                    &[0; 8],
                )?;
                return Ok((
                    frame,
                    vec![
                        Resource {
                            name: "/f".into(),
                            bytes: f,
                        },
                        Resource {
                            name: "/o".into(),
                            bytes: Vec::new(),
                        },
                    ],
                ));
            };
            let companion = original
                .resources
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case(&name))
                .ok_or_else(|| corrupt("missing companion"))?;
            let raw = companion.bytes.get(offset..).ok_or_else(budget)?;
            let data = if raw.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]) {
                raw
            } else {
                let mut reader = Reader::new(raw);
                let n = reader.u32()? as usize;
                let data = reader.take(n)?;
                reader.end()?;
                data
            };
            let mut file = super::open_resource(data, limits)?;
            let paths = file
                .walk()
                .filter(|e| e.is_stream())
                .map(|e| e.path().to_owned())
                .collect::<Vec<_>>();
            let mut resources = Vec::new();
            for p in paths {
                let name = super::resource_path(&p)?;
                let bytes = crate::read_stream(&mut file, &name, MAX_RESOURCE_BYTES)?;
                resources.push(Resource { name, bytes });
            }
            (frame, resources)
        }
    })
}
/// Native embedded streams for the existing component storage identity.
pub fn encode_embedded(
    original: &NativeForm,
    design: &Design,
    source: &str,
    limits: &Limits,
) -> Result<Vec<Resource>> {
    encode_embedded_designed(original, design, source, 0, limits)
}
/// As [`encode_embedded`], for a designer that has given out control IDs up
/// to `largest_id` (see [`encode_export_designed`]).
pub fn encode_embedded_designed(
    original: &NativeForm,
    design: &Design,
    source: &str,
    largest_id: u32,
    limits: &Limits,
) -> Result<Vec<Resource>> {
    if original.format != Format::Embedded {
        return Err(unsupported("embedded native origin required"));
    }
    let files =
        encode_export_designed(original, &original.name, design, source, largest_id, limits)?;
    let first = files.first().ok_or_else(budget)?;
    let export = super::decode_export(
        &first.name,
        &first.bytes,
        original.code_page,
        files.get(1..).ok_or_else(budget)?,
        limits,
    )?;
    let (header, streams) = resource_streams(&export, limits)?;
    let parsed = super::frame::parse(&header, true)?;
    let frame = header
        .get(..parsed.source_start)
        .ok_or_else(budget)?
        .split_inclusive('\n')
        .filter(|line| {
            !line
                .trim()
                .to_ascii_lowercase()
                .starts_with("oleobjectblob")
        })
        .collect::<String>();
    let frame_path = original
        .resources
        .iter()
        .find(|r| r.name.ends_with("/\u{3}VBFrame"))
        .ok_or_else(budget)?;
    let root = frame_path
        .name
        .strip_suffix("/\u{3}VBFrame")
        .ok_or_else(budget)?;
    let mut out = vec![Resource {
        name: frame_path.name.clone(),
        bytes: encode_text(&frame, original.code_page)?,
    }];
    out.extend(streams.into_iter().map(|r| Resource {
        name: format!("{root}{}", r.name),
        bytes: r.bytes,
    }));
    Ok(out)
}
