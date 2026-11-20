//! UserForm designer storage across edited binary-project copies.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod support;
#[path = "support/native_forms.rs"]
mod support_native;
use std::io::{Cursor, Read};
use vba_project::{Limits, ModuleUpdate, decode, encode_updated_project};
/// The shape Office writes for a UserForm whose caption is only in VBFrame:
/// FormControl.Caption (PropMask bit 19) clear and no caption string.
fn streams_with_caption_only_in_frame() -> Vec<(String, Vec<u8>)> {
    let mut streams = support_native::streams();
    let f = &mut streams[0].1;
    let caption = b"Native form\0".to_vec();
    let body = u32::from(u16::from_le_bytes([f[2], f[3]]));
    let mut b = f[4..4 + body as usize].to_vec();
    let rest = f[4 + body as usize..].to_vec();
    let mask = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) & !(1 << 19);
    b.splice(0..8, mask.to_le_bytes());
    b.truncate(b.len() - caption.len());
    let mut out = support_native::record(0x400, &b);
    out.extend(rest);
    *f = out;
    streams
}
type Streams = Vec<(String, Vec<u8>)>;
fn form_project(forms: &[(&str, Streams)]) -> Vec<u8> {
    let mut specs = vec![support::Spec::new("Utilities", "CodeStream", "Module")];
    specs.extend(
        forms
            .iter()
            .map(|(name, _)| support::Spec::new(name, name, "BaseClass")),
    );
    let mut fixture = support::Fixture::new(1252, &specs);
    for (name, streams) in forms {
        fixture.streams.extend(
            streams
                .iter()
                .map(|(stream, bytes)| (format!("/{name}/{stream}"), bytes.clone())),
        );
        fixture.streams.push((
            format!("/{name}/\u{3}VBFrame"),
            support_native::frame(false)
                .replace("NativeForm", name)
                .into_bytes(),
        ));
    }
    make_writable(&mut fixture);
    fixture.bytes()
}
fn storage(bytes: &[u8], root: &str) -> Streams {
    let mut file = cfb::CompoundFile::open_strict(Cursor::new(bytes.to_vec())).unwrap();
    let paths = file
        .walk_storage(root)
        .unwrap()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_owned())
        .collect::<Vec<_>>();
    let mut out = paths
        .into_iter()
        .map(|path| {
            let mut data = Vec::new();
            file.open_stream(&path)
                .unwrap()
                .read_to_end(&mut data)
                .unwrap();
            (path.to_string_lossy().into_owned(), data)
        })
        .collect::<Vec<_>>();
    out.sort();
    out
}
fn form_caption_bit(bytes: &[u8], root: &str) -> bool {
    let f = storage(bytes, root)
        .into_iter()
        .find(|(path, _)| path.ends_with("/f"))
        .unwrap()
        .1;
    u32::from_le_bytes([f[4], f[5], f[6], f[7]]) & (1 << 19) != 0
}
#[test]
fn source_only_edit_keeps_every_form_storage_byte_for_byte() {
    let original = form_project(&[
        ("FrameCaption", streams_with_caption_only_in_frame()),
        ("FieldCaption", support_native::streams()),
    ]);
    let before = decode(&original, &Limits::default()).unwrap();
    // The IDE passes every form's current design, changed or not.
    let updates = before
        .modules
        .iter()
        .map(|m| ModuleUpdate {
            id: m.id,
            source: if m.form.is_some() {
                m.source.clone()
            } else {
                m.source.replace("UnknownHostCall", "Debug.Print 42")
            },
            form_design: m.form.as_ref().map(|f| f.design.clone()),
        })
        .collect::<Vec<_>>();
    let bytes = encode_updated_project(&original, &updates, &Limits::default()).unwrap();
    for root in ["/FrameCaption", "/FieldCaption"] {
        assert_eq!(storage(&bytes, root), storage(&original, root), "{root}");
    }
    let after = decode(&bytes, &Limits::default()).unwrap();
    assert!(after.modules[0].source.contains("Debug.Print 42"));
}
#[test]
fn code_behind_edit_keeps_the_form_designer_storage() {
    let original = form_project(&[("FrameCaption", streams_with_caption_only_in_frame())]);
    let before = decode(&original, &Limits::default()).unwrap();
    let form = &before.modules[1];
    let source = form.source.replace("UnknownHostCall", "Debug.Print 7");
    let bytes = encode_updated_project(
        &original,
        &[ModuleUpdate {
            id: form.id,
            source: source.clone(),
            form_design: None,
        }],
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(
        storage(&bytes, "/FrameCaption"),
        storage(&original, "/FrameCaption")
    );
    assert_eq!(
        decode(&bytes, &Limits::default()).unwrap().modules[1].source,
        source
    );
}
#[test]
fn designer_edit_rewrites_only_that_form_and_keeps_its_caption_representation() {
    let original = form_project(&[
        ("FrameCaption", streams_with_caption_only_in_frame()),
        ("FieldCaption", support_native::streams()),
    ]);
    assert!(!form_caption_bit(&original, "/FrameCaption"));
    assert!(form_caption_bit(&original, "/FieldCaption"));
    let before = decode(&original, &Limits::default()).unwrap();
    let mut updates = Vec::new();
    let mut designs = Vec::new();
    for module in &before.modules[1..] {
        let mut design = module.form.as_ref().unwrap().design.clone();
        design.caption = format!("{} renamed", module.name);
        design.controls[0].caption = "saved label".into();
        designs.push(design.clone());
        updates.push(ModuleUpdate {
            id: module.id,
            source: module.source.clone(),
            form_design: Some(design),
        });
    }
    // Edit the first form only.
    let bytes = encode_updated_project(&original, &updates[..1], &Limits::default()).unwrap();
    assert_eq!(
        storage(&bytes, "/FieldCaption"),
        storage(&original, "/FieldCaption")
    );
    assert_ne!(
        storage(&bytes, "/FrameCaption"),
        storage(&original, "/FrameCaption")
    );
    assert!(!form_caption_bit(&bytes, "/FrameCaption"));
    let after = decode(&bytes, &Limits::default()).unwrap();
    assert_eq!(after.modules[1].form.as_ref().unwrap().design, designs[0]);
    // A form whose original stored the caption in FormControl keeps it there.
    let bytes = encode_updated_project(&original, &updates[1..], &Limits::default()).unwrap();
    assert!(form_caption_bit(&bytes, "/FieldCaption"));
    let after = decode(&bytes, &Limits::default()).unwrap();
    assert_eq!(after.modules[2].form.as_ref().unwrap().design, designs[1]);
}
fn make_writable(fixture: &mut support::Fixture) {
    let marker = [0x25, 0, 0, 0, 0, 0];
    while let Some(at) = fixture.dir.windows(marker.len()).position(|w| w == marker) {
        fixture.dir.drain(at..at + marker.len());
    }
}
