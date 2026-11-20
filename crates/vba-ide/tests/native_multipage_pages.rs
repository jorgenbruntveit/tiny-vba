//! A MultiPage's pages keep their identity in its internal TabStrip and page
//! list. Renaming, recaptioning and deleting pages in the designer rewrites
//! both, so the export reopens with exactly the designed pages.
#![allow(clippy::indexing_slicing)]
#[path = "../../vba-project/tests/support/native_forms.rs"]
mod fixture;
use vba_ide::{
    Action, Ide, Mode, NativeFormResource, NoHost, SourceId,
    forms::{Control, ControlKind, DesignEdit},
};
type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// MultiPage 20 with pages 31 (FirstTab), 32 (SecondTab), 33 (BlockedTab),
/// each holding one TextBox (40, 41, 42).
fn import(selected: i32) -> R<(Ide, SourceId)> {
    let mut ide = Ide::new();
    ide.import_native_form_export(
        "NativeForm.frm",
        &fixture::export(),
        1252,
        &[NativeFormResource {
            name: "NativeForm.frx".into(),
            bytes: fixture::multipage_companion(selected),
        }],
    )?;
    let id = ide.active_module().ok_or("source")?.id;
    Ok((ide, id))
}
fn controls(ide: &Ide, id: SourceId) -> R<Vec<Control>> {
    Ok(ide.form(id).ok_or("form")?.form().controls.clone())
}
fn page(ide: &Ide, id: SourceId, page: u32) -> R<Control> {
    controls(ide, id)?
        .into_iter()
        .find(|c| c.id == page)
        .ok_or_else(|| "page".into())
}
/// Export, reimport, and return the reopened IDE and form.
fn reopen(ide: &Ide, id: SourceId) -> R<(Ide, SourceId)> {
    let snapshot = ide.export_native_form(id)?;
    let mut again = Ide::new();
    again.import_source_bundle(snapshot.files(), 1252)?;
    let reopened = again.active_module().ok_or("source")?.id;
    Ok((again, reopened))
}
fn pages(controls: &[Control]) -> Vec<(u32, String, String)> {
    controls
        .iter()
        .filter(|c| c.kind == ControlKind::Page)
        .map(|c| (c.id, c.name.clone(), c.caption.clone()))
        .collect()
}

#[test]
fn renaming_and_recaptioning_a_page_updates_its_tab() -> R {
    let (mut ide, id) = import(0)?;
    let mut first = page(&ide, id, 31)?;
    first.name = "Settings".into();
    first.caption = "Settings & more".into();
    ide.edit_form(id, DesignEdit::Replace(first))?;
    let mut third = page(&ide, id, 33)?;
    third.caption = "Ünïcode tab".into();
    ide.edit_form(id, DesignEdit::Replace(third))?;
    let (again, reopened) = reopen(&ide, id)?;
    let after = controls(&again, reopened)?;
    assert_eq!(after, controls(&ide, id)?);
    assert_eq!(
        pages(&after),
        [
            (31, "Settings".into(), "Settings & more".into()),
            (32, "SecondTab".into(), page(&ide, id, 32)?.caption),
            (33, "BlockedTab".into(), "Ünïcode tab".into()),
        ]
    );
    // A second edit of the reopened form keeps working.
    let (mut again, reopened) = (again, reopened);
    let mut second = page(&again, reopened, 32)?;
    second.name = "Details".into();
    again.edit_form(reopened, DesignEdit::Replace(second))?;
    let (third_open, form) = reopen(&again, reopened)?;
    assert_eq!(page(&third_open, form, 32)?.name, "Details");
    Ok(())
}

#[test]
fn deleting_a_page_removes_its_tab_and_contents() -> R {
    for (selected, delete) in [(0, 32), (1, 32), (2, 32), (0, 31), (2, 33)] {
        let (mut ide, id) = import(selected)?;
        ide.select_form_control(id, Some(delete), false)?;
        ide.edit_form(id, DesignEdit::Delete)?;
        let designed = controls(&ide, id)?;
        assert!(
            designed
                .iter()
                .all(|c| c.id != delete && c.parent != Some(delete))
        );
        let (again, reopened) = reopen(&ide, id)?;
        assert_eq!(
            controls(&again, reopened)?,
            designed,
            "selected {selected}, deleted {delete}"
        );
    }
    // Every page, one at a time.
    let (mut ide, id) = import(0)?;
    for delete in [31, 32, 33] {
        ide.select_form_control(id, Some(delete), false)?;
        ide.edit_form(id, DesignEdit::Delete)?;
    }
    let (again, reopened) = reopen(&ide, id)?;
    assert_eq!(controls(&again, reopened)?, controls(&ide, id)?);
    Ok(())
}

#[test]
fn page_state_survives_alongside_tab_edits() -> R {
    let (mut ide, id) = import(0)?;
    let mut second = page(&ide, id, 32)?;
    second.enabled = false;
    second.visible = false;
    second.name = "Hidden".into();
    ide.edit_form(id, DesignEdit::Replace(second))?;
    let (again, reopened) = reopen(&ide, id)?;
    let after = page(&again, reopened, 32)?;
    assert_eq!(
        (after.name.as_str(), after.enabled, after.visible),
        ("Hidden", false, false)
    );
    Ok(())
}

#[test]
fn a_form_runs_with_the_pages_the_designer_has() -> R {
    // The selected (last) page's neighbour is deleted and the first renamed;
    // the running MultiPage must see the designed tabs, not the imported ones.
    let (mut ide, id) = import(2)?;
    ide.select_form_control(id, Some(32), false)?;
    ide.edit_form(id, DesignEdit::Delete)?;
    let mut first = page(&ide, id, 31)?;
    first.name = "Start".into();
    first.caption = "Start here".into();
    ide.edit_form(id, DesignEdit::Replace(first))?;
    ide.add_module(
        "Main.bas",
        "Sub Main()\nDim m As MSForms.MultiPage, p As MSForms.Page\nSet m = NativeForm.Multi1\nDebug.Print m.Pages.Count, m.Value, m.Pages(m.Value).Name\nFor Each p In m.Pages\nDebug.Print p.Name, p.Caption, p.Index\nNext p\nDebug.Print m.Start Is m.Pages(0)\nEnd Sub\n",
    )?;
    ide.dispatch(Action::Run, &mut NoHost)?;
    for _ in 0..2000 {
        if ide.mode() != Mode::Running {
            break;
        }
        ide.tick(&mut NoHost)?;
    }
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    let blocked = page(&ide, id, 33)?.caption;
    assert_eq!(
        ide.output(),
        vec![
            "initialized".to_string(),
            "2\t1\tBlockedTab".into(),
            "Start\tStart here\t0".into(),
            format!("BlockedTab\t{blocked}\t1"),
            "True".into(),
        ]
    );
    Ok(())
}

/// The shared MultiPage fixture with internal tab names that differ from its
/// Page names, as Office writes them (Page1 on tab "Tab3", and so on).
fn office_tab_names() -> R<Vec<u8>> {
    use std::io::{Cursor, Read, Write};
    let original = fixture::multipage_companion(0);
    let mut file =
        cfb::CompoundFile::open(Cursor::new(original.get(4..).ok_or("companion")?.to_vec()))?;
    let paths = file
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_owned())
        .collect::<Vec<_>>();
    let mut out = cfb::CompoundFile::create(Cursor::new(Vec::new()))?;
    for path in paths {
        let mut bytes = Vec::new();
        file.open_stream(&path)?.read_to_end(&mut bytes)?;
        if path.to_string_lossy().replace('\\', "/") == "/i20/o" {
            // Same-length renames inside the TabStrip's TabNames array only.
            for (from, to) in [
                ("FirstTab", "InnerTb1"),
                ("SecondTab", "InnerTab2"),
                ("BlockedTab", "InnerTab03"),
            ] {
                let wide = |s: &str| {
                    s.encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                };
                for (f, t) in [
                    (from.as_bytes().to_vec(), to.as_bytes().to_vec()),
                    (wide(from), wide(to)),
                ] {
                    if let Some(at) = bytes.windows(f.len()).position(|w| w == f) {
                        bytes.splice(at..at + f.len(), t);
                    }
                }
            }
        }
        if let Some(parent) = path.parent() {
            out.create_storage_all(parent)?;
        }
        out.create_stream(&path)?.write_all(&bytes)?;
    }
    let bytes = out.into_inner().into_inner();
    let mut companion = u32::try_from(bytes.len())?.to_le_bytes().to_vec();
    companion.extend(bytes);
    Ok(companion)
}

#[test]
fn office_internal_tab_names_stay_when_a_page_is_renamed() -> R {
    let mut ide = Ide::new();
    ide.import_native_form_export(
        "NativeForm.frm",
        &fixture::export(),
        1252,
        &[NativeFormResource {
            name: "NativeForm.frx".into(),
            bytes: office_tab_names()?,
        }],
    )?;
    let id = ide.active_module().ok_or("source")?.id;
    let mut first = page(&ide, id, 31)?;
    first.name = "Start".into();
    first.caption = "Start here".into();
    ide.edit_form(id, DesignEdit::Replace(first))?;
    let snapshot = ide.export_native_form(id)?;
    let files = snapshot.files();
    let saved = vba_project::forms::decode_export(
        &files[0].name,
        &files[0].bytes,
        1252,
        &files[1..],
        &vba_project::Limits::default(),
    )?;
    let tabs = saved
        .design
        .controls
        .iter()
        .find(|c| c.id == 20)
        .and_then(|c| c.tabs.as_ref())
        .ok_or("tabs")?;
    assert_eq!(
        tabs.tabs
            .iter()
            .map(|t| (t.name.as_str(), t.caption.as_str()))
            .collect::<Vec<_>>()[0],
        ("InnerTb1", "Start here")
    );
    // The running form reaches Pages by their names, not the internal tab names.
    ide.add_module(
        "Main.bas",
        "Sub Main()\nDim m As MSForms.MultiPage\nSet m = NativeForm.Multi1\nDebug.Print m.Pages(\"Start\").Caption, m.Start Is m.Pages(0), m.SecondTab.Index\nEnd Sub\n",
    )?;
    ide.dispatch(Action::Run, &mut NoHost)?;
    for _ in 0..2000 {
        if ide.mode() != Mode::Running {
            break;
        }
        ide.tick(&mut NoHost)?;
    }
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    assert_eq!(ide.output(), vec!["initialized", "Start here\tTrue\t1"]);
    Ok(())
}
