//! Designer edits on imported native forms export and reopen exactly: added
//! controls, z-order changes and deletes inside Frames and MultiPages, with
//! explicit refusals for edits the encoder cannot persist.
#![allow(clippy::indexing_slicing)]
#[path = "../../vba-project/tests/support/native_forms.rs"]
mod fixture;
#[allow(clippy::unwrap_used)]
#[path = "../../vba-project/tests/support/mod.rs"]
mod project_fixture;
use vba_ide::{
    Action, Ide, NativeFormResource, NoHost, SourceId,
    forms::{Bounds, Control, ControlKind, DesignEdit},
};
type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn import(companion: Vec<u8>) -> R<(Ide, SourceId)> {
    let mut ide = Ide::new();
    ide.import_native_form_export(
        "NativeForm.frm",
        &fixture::export(),
        1252,
        &[NativeFormResource {
            name: "NativeForm.frx".into(),
            bytes: companion,
        }],
    )?;
    let id = ide.active_module().ok_or("source")?.id;
    Ok((ide, id))
}
fn controls(ide: &Ide, id: SourceId) -> R<Vec<Control>> {
    Ok(ide.form(id).ok_or("form")?.form().controls.clone())
}
fn control(ide: &Ide, id: SourceId, control: u32) -> R<Control> {
    controls(ide, id)?
        .into_iter()
        .find(|c| c.id == control)
        .ok_or_else(|| "control".into())
}
/// (id, parent) in designer order.
fn layout(controls: &[Control]) -> Vec<(u32, Option<u32>)> {
    controls.iter().map(|c| (c.id, c.parent)).collect()
}
/// Export, reimport, and return the reopened controls.
fn reopen(ide: &Ide, id: SourceId) -> R<Vec<Control>> {
    let snapshot = ide.export_native_form(id)?;
    let mut again = Ide::new();
    again.import_source_bundle(snapshot.files(), 1252)?;
    let reopened = again.active_module().ok_or("source")?.id;
    controls(&again, reopened)
}
fn select(ide: &mut Ide, id: SourceId, control: u32) -> R {
    ide.select_form_control(id, Some(control), false)?;
    Ok(())
}
fn add(ide: &mut Ide, id: SourceId, kind: ControlKind, top: f32) -> R<u32> {
    ide.edit_form(
        id,
        DesignEdit::Add(
            kind,
            Bounds {
                left: 6.0,
                top,
                width: 72.0,
                height: 18.0,
            },
        ),
    )?;
    Ok(controls(ide, id)?.last().ok_or("added")?.id)
}

/// The shared MultiPage fixture with its internal TabStrip (stored as ID 21)
/// renumbered, so it can hold the highest ID in the form.
fn multipage_with_strip_id(strip: u32) -> R<Vec<u8>> {
    use std::io::{Cursor, Read, Write};
    let replace = |bytes: &mut Vec<u8>, from: &[u8], to: &[u8]| -> R {
        let at = bytes
            .windows(from.len())
            .position(|w| w == from)
            .ok_or("fixture pattern")?;
        bytes.splice(at..at + from.len(), to.iter().copied());
        Ok(())
    };
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
        let name = path.to_string_lossy().replace('\\', "/");
        // Site record (mask, 12-character name, ID) and MultiPage properties (mask, pages, ID).
        if name == "/i20/f" {
            replace(
                &mut bytes,
                &fixture::u32s(&[0x1f5, 0x8000_000c, 21]),
                &fixture::u32s(&[0x1f5, 0x8000_000c, strip]),
            )?;
        } else if name == "/i20/x" {
            replace(
                &mut bytes,
                &fixture::u32s(&[6, 3, 21]),
                &fixture::u32s(&[6, 3, strip]),
            )?;
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

/// A binary VBA project holding the shared native form with `streams`.
fn project(streams: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut spec = project_fixture::Spec::new("NativeForm", "NativeForm", "BaseClass");
    spec.source = fixture::SOURCE.as_bytes().to_vec();
    let mut f = project_fixture::Fixture::new(1252, &[spec]);
    let marker = [0x25, 0, 0, 0, 0, 0];
    while let Some(at) = f.dir.windows(6).position(|w| w == marker) {
        f.dir.drain(at..at + 6);
    }
    f.streams.extend(
        streams
            .into_iter()
            .map(|(name, bytes)| (format!("/NativeForm/{name}"), bytes)),
    );
    f.streams.push((
        "/NativeForm/\u{3}VBFrame".into(),
        fixture::frame(false).into_bytes(),
    ));
    f.bytes()
}
/// The streams of a `.frx` companion, by path without the leading `/`.
fn companion_streams(companion: &[u8]) -> R<Vec<(String, Vec<u8>)>> {
    use std::io::{Cursor, Read};
    let mut file =
        cfb::CompoundFile::open(Cursor::new(companion.get(4..).ok_or("companion")?.to_vec()))?;
    let paths = file
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_owned())
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    for path in paths {
        let mut bytes = Vec::new();
        file.open_stream(&path)?.read_to_end(&mut bytes)?;
        out.push((
            path.to_string_lossy()
                .replace('\\', "/")
                .trim_start_matches('/')
                .to_string(),
            bytes,
        ));
    }
    Ok(out)
}

#[test]
fn ids_given_out_since_opening_stay_used_in_a_workbook_save() -> R {
    // Delete a MultiPage whose internal TabStrip holds the highest ID (43),
    // save the workbook project, reopen it, and add a control.
    let original = project(companion_streams(&multipage_with_strip_id(43)?)?);
    let decoded = vba_project::decode(&original, &vba_project::Limits::default())?;
    let mut ide = Ide::new();
    let id = ide.attach_native_form(decoded.modules[0].form.clone().ok_or("native")?)?;
    select(&mut ide, id, 20)?;
    ide.edit_form(id, DesignEdit::Delete)?;
    let saved = ide.export_native_project(&original)?;
    let reopened = vba_project::decode(saved.bytes(), &vba_project::Limits::default())?;
    let mut again = Ide::new();
    let id = again.attach_native_form(reopened.modules[0].form.clone().ok_or("native")?)?;
    assert_eq!(add(&mut again, id, ControlKind::Label, 6.0)?, 44);
    Ok(())
}

#[test]
fn a_second_workbook_copy_keeps_the_id_of_a_control_deleted_after_the_first() -> R {
    // Export a copy with a new Label, delete it, edit something else, and
    // export a second copy from the same original workbook project.
    let original = project(fixture::streams());
    let decoded = vba_project::decode(&original, &vba_project::Limits::default())?;
    let mut ide = Ide::new();
    let id = ide.attach_native_form(decoded.modules[0].form.clone().ok_or("native")?)?;
    let added = add(&mut ide, id, ControlKind::Label, 120.0)?;
    ide.export_native_project(&original)?;
    select(&mut ide, id, added)?;
    ide.edit_form(id, DesignEdit::Delete)?;
    let mut heading = control(&ide, id, 11)?;
    heading.caption = "Another edit".into();
    ide.edit_form(id, DesignEdit::Replace(heading))?;
    let second = ide.export_native_project(&original)?;
    let reopened = vba_project::decode(second.bytes(), &vba_project::Limits::default())?;
    let mut again = Ide::new();
    let id = again.attach_native_form(reopened.modules[0].form.clone().ok_or("native")?)?;
    assert_eq!(add(&mut again, id, ControlKind::Label, 120.0)?, added + 1);
    Ok(())
}

#[test]
fn toolbox_controls_added_to_an_imported_native_project_form_export() -> R {
    let original = project(fixture::streams());
    let project = vba_project::decode(&original, &vba_project::Limits::default())?;
    let mut ide = Ide::new();
    let id = ide.attach_native_form(project.modules[0].form.clone().ok_or("native")?)?;
    let mut added = Vec::new();
    for (i, kind) in [
        ControlKind::Label,
        ControlKind::TextBox,
        ControlKind::CommandButton,
    ]
    .into_iter()
    .enumerate()
    {
        let control_id = add(&mut ide, id, kind, 120.0 + i as f32 * 24.0)?;
        let mut c = control(&ide, id, control_id)?;
        if kind == ControlKind::TextBox {
            c.text = "typed".into();
        } else {
            c.caption = format!("New {}", c.name);
        }
        ide.edit_form(id, DesignEdit::Replace(c.clone()))?;
        added.push(c);
    }
    let snapshot = ide.export_native_project(&original)?;
    let after = vba_project::decode(snapshot.bytes(), &vba_project::Limits::default())?;
    let design = &after.modules[0].form.as_ref().ok_or("native")?.design;
    for c in &added {
        let saved = design
            .controls
            .iter()
            .find(|n| n.id == c.id)
            .ok_or("added control saved")?;
        assert_eq!(
            (
                saved.name.as_str(),
                saved.caption.as_str(),
                saved.text.as_str()
            ),
            (c.name.as_str(), c.caption.as_str(), c.text.as_str())
        );
        assert_eq!(saved.parent, None);
    }
    assert_eq!(design.controls.len(), 6);
    Ok(())
}

#[test]
fn front_and_back_keep_each_container_with_its_contents() -> R {
    // Frame1 (20) > Frame2 (21) > Entry (12), with root controls before Frame1.
    let frame = || fixture::frame_companion(2);
    for (target, front) in [(20, true), (20, false), (21, true), (21, false), (12, true)] {
        let (mut ide, id) = import(frame())?;
        select(&mut ide, id, target)?;
        ide.edit_form(
            id,
            if front {
                DesignEdit::Front
            } else {
                DesignEdit::Back
            },
        )?;
        let designed = layout(&controls(&ide, id)?);
        assert_eq!(layout(&reopen(&ide, id)?), designed, "{target} {front}");
    }
    let (mut ide, id) = import(frame())?;
    select(&mut ide, id, 20)?;
    ide.edit_form(id, DesignEdit::Back)?;
    assert_eq!(
        layout(&controls(&ide, id)?),
        [
            (20, None),
            (21, Some(20)),
            (12, Some(21)),
            (11, None),
            (13, None)
        ]
    );

    // MultiPage (20) > pages 31..33, each holding one TextBox (40..42).
    let multipage = || fixture::multipage_companion(0);
    for (target, front) in [(40, true), (42, false), (20, true), (20, false)] {
        let (mut ide, id) = import(multipage())?;
        select(&mut ide, id, target)?;
        ide.edit_form(
            id,
            if front {
                DesignEdit::Front
            } else {
                DesignEdit::Back
            },
        )?;
        let designed = layout(&controls(&ide, id)?);
        assert_eq!(layout(&reopen(&ide, id)?), designed, "{target} {front}");
    }
    // A Page's position is its tab order, so z-order commands leave it in place.
    let (mut ide, id) = import(multipage())?;
    let before = layout(&controls(&ide, id)?);
    select(&mut ide, id, 31)?;
    ide.edit_form(id, DesignEdit::Front)?;
    assert_eq!(layout(&controls(&ide, id)?), before);
    Ok(())
}

#[test]
fn deleting_a_container_deletes_its_contents_and_exports() -> R {
    for (target, gone) in [(21, vec![21, 12]), (20, vec![20, 21, 12])] {
        let (mut ide, id) = import(fixture::frame_companion(2))?;
        select(&mut ide, id, target)?;
        ide.edit_form(id, DesignEdit::Delete)?;
        let reopened = reopen(&ide, id)?;
        assert!(reopened.iter().all(|c| !gone.contains(&c.id)), "{target}");
        assert_eq!(layout(&reopened), layout(&controls(&ide, id)?));
        ide.form_history(id, false)?;
        assert!(controls(&ide, id)?.iter().any(|c| c.id == 12));
    }
    let (mut ide, id) = import(fixture::multipage_companion(0))?;
    select(&mut ide, id, 20)?;
    ide.edit_form(id, DesignEdit::Delete)?;
    assert_eq!(
        layout(&reopen(&ide, id)?),
        [(11, None), (12, None), (13, None)]
    );
    Ok(())
}

#[test]
fn a_new_control_can_be_placed_in_a_container_but_an_existing_one_cannot_move() -> R {
    // New control on the first page of three: kept before the later pages.
    let (mut ide, id) = import(fixture::multipage_companion(0))?;
    let added = add(&mut ide, id, ControlKind::Label, 6.0)?;
    let mut c = control(&ide, id, added)?;
    c.parent = Some(31);
    ide.edit_form(id, DesignEdit::Replace(c))?;
    let reopened = reopen(&ide, id)?;
    assert_eq!(layout(&reopened), layout(&controls(&ide, id)?));
    assert_eq!(
        reopened
            .iter()
            .find(|c| c.id == added)
            .and_then(|c| c.parent),
        Some(31)
    );

    // Moving a persisted control between containers is refused, not lost.
    let (mut ide, id) = import(fixture::frame_companion(2))?;
    let mut entry = control(&ide, id, 12)?;
    entry.parent = None;
    ide.edit_form(id, DesignEdit::Replace(entry))?;
    let error = ide.export_native_form(id).err().ok_or("refused")?;
    assert!(error.message.contains("another container"), "{error:?}");
    Ok(())
}

#[test]
fn new_controls_never_reuse_an_internal_tabstrip_id() -> R {
    // The MultiPage's internal TabStrip holds the highest stored ID.
    let (mut ide, id) = import(multipage_with_strip_id(43)?)?;
    assert!(controls(&ide, id)?.iter().all(|c| c.id < 43));
    let added = add(&mut ide, id, ControlKind::Label, 6.0)?;
    assert_eq!(added, 44);
    assert!(reopen(&ide, id)?.iter().any(|c| c.id == 44));
    Ok(())
}

#[test]
fn an_id_once_saved_is_never_given_to_a_new_control() -> R {
    // NextAvailableID records the largest ID used, so a saved and then deleted
    // control's ID stays used after reopening.
    let (mut ide, id) = import(fixture::companion())?;
    let first = add(&mut ide, id, ControlKind::Label, 6.0)?;
    let snapshot = ide.export_native_form(id)?;
    let mut again = Ide::new();
    again.import_source_bundle(snapshot.files(), 1252)?;
    let id = again.active_module().ok_or("source")?.id;
    select(&mut again, id, first)?;
    again.edit_form(id, DesignEdit::Delete)?;
    let snapshot = again.export_native_form(id)?;
    let mut third = Ide::new();
    third.import_source_bundle(snapshot.files(), 1252)?;
    let id = third.active_module().ok_or("source")?.id;
    assert!(controls(&third, id)?.iter().all(|c| c.id != first));
    assert_eq!(add(&mut third, id, ControlKind::Label, 6.0)?, first + 1);
    Ok(())
}

#[test]
fn tab_order_cannot_reorder_multipage_pages() -> R {
    let (mut ide, id) = import(fixture::multipage_companion(0))?;
    let mut order = ide.form(id).ok_or("form")?.form().tab_order.clone();
    let first = order.iter().position(|c| *c == 31).ok_or("page")?;
    let second = order.iter().position(|c| *c == 32).ok_or("page")?;
    order.swap(first, second);
    ide.edit_form(id, DesignEdit::TabOrder(order))?;
    let error = ide.export_native_form(id).err().ok_or("refused")?;
    assert!(
        error.message.contains("reordering MultiPage pages"),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn a_form_compiles_after_its_bound_controls_are_deleted_or_renamed() -> R {
    // Control 14 (List1) has a RowSource and a ControlSource; running them needs
    // a host range capability, so this checks the generated code compiles.
    let compile = |ide: &mut Ide| -> R {
        ide.add_module("Main.bas", "Sub Main()\nEnd Sub\n")?;
        ide.dispatch(Action::Compile, &mut NoHost)?;
        assert!(ide.diagnostics.is_empty(), "{:?}", ide.diagnostics);
        Ok(())
    };
    let (mut ide, id) = import(fixture::binding_companion())?;
    select(&mut ide, id, 14)?;
    ide.edit_form(id, DesignEdit::Delete)?;
    compile(&mut ide)?;

    let (mut ide, id) = import(fixture::binding_companion())?;
    let mut renamed = control(&ide, id, 14)?;
    renamed.name = "Choices".into();
    ide.edit_form(id, DesignEdit::Replace(renamed))?;
    compile(&mut ide)?;
    Ok(())
}

#[test]
fn ids_given_out_since_opening_stay_used_across_saves() -> R {
    // Saved, then deleted and saved again without reopening.
    let (mut ide, id) = import(fixture::companion())?;
    let added = add(&mut ide, id, ControlKind::Label, 6.0)?;
    let snapshot = ide.export_native_form(id)?;
    ide.acknowledge_native_form_save(&snapshot)?;
    select(&mut ide, id, added)?;
    ide.edit_form(id, DesignEdit::Delete)?;
    let snapshot = ide.export_native_form(id)?;
    let mut again = Ide::new();
    again.import_source_bundle(snapshot.files(), 1252)?;
    let reopened = again.active_module().ok_or("source")?.id;
    assert_eq!(
        add(&mut again, reopened, ControlKind::Label, 6.0)?,
        added + 1
    );

    // A deleted MultiPage's internal TabStrip ID stays used too.
    let (mut ide, id) = import(multipage_with_strip_id(43)?)?;
    select(&mut ide, id, 20)?;
    ide.edit_form(id, DesignEdit::Delete)?;
    let snapshot = ide.export_native_form(id)?;
    let mut again = Ide::new();
    again.import_source_bundle(snapshot.files(), 1252)?;
    let reopened = again.active_module().ok_or("source")?.id;
    assert_eq!(add(&mut again, reopened, ControlKind::Label, 6.0)?, 44);
    Ok(())
}
