//! A control may take any name VBA accepts, including a name that matches a
//! member the IDE generates for the form (`Enter`, `MouseDown`, `Loaded`,
//! `Lookup`). The generated field that holds each control must not clash
//! with those members.
#![allow(clippy::indexing_slicing)]
#[path = "../../vba-project/tests/support/native_forms.rs"]
mod fixture;
use vba_ide::forms::{Bounds, ControlKind, DesignEdit};
use vba_ide::{Action, Ide, Mode, NativeFormResource, NoHost, Value};
type R<T = ()> = Result<T, Box<dyn std::error::Error>>;
/// Names of generated form members: focus and mouse dispatch, the loaded
/// flag and the control lookup.
const NAMES: [&str; 4] = ["Enter", "MouseDown", "Loaded", "Lookup"];
fn settle(ide: &mut Ide) -> R {
    for _ in 0..2000 {
        if ide.mode() != Mode::Running {
            return Ok(());
        }
        ide.tick(&mut NoHost)?;
    }
    Err("form timeout".into())
}

#[test]
fn buttons_named_like_generated_members_run_their_handlers() -> R {
    let mut ide = Ide::new();
    let id = ide.add_form("NamedForm")?;
    let mut source = String::from("Attribute VB_Name = \"NamedForm\"\r\n");
    for (i, name) in NAMES.iter().enumerate() {
        ide.edit_form(
            id,
            DesignEdit::Add(
                ControlKind::CommandButton,
                Bounds {
                    left: 12.0,
                    top: [12.0, 42.0, 72.0, 102.0][i],
                    width: 120.0,
                    height: 24.0,
                },
            ),
        )?;
        let mut button = ide.form(id).ok_or("form")?.form().controls[i].clone();
        button.name = (*name).into();
        ide.edit_form(id, DesignEdit::Replace(button))?;
        source.push_str(&format!(
            "Private Sub {name}_Click()\nDebug.Print {name}.Name\nEnd Sub\n"
        ));
    }
    ide.set_text(source)?;
    ide.run_form(id, false, &mut NoHost)?;
    settle(&mut ide)?;
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    let buttons: Vec<u32> = ide
        .form(id)
        .ok_or("form")?
        .form()
        .controls
        .iter()
        .map(|c| c.id)
        .collect();
    for button in buttons {
        ide.form_event(id, Some(button), None, &mut NoHost)?;
        settle(&mut ide)?;
    }
    assert_eq!(ide.output(), NAMES);
    // Inspection reads a control's field as `Vba_<Name>`, and as `Vba<Name>`
    // when no generated member has that name; `VbaLoaded` stays the form's flag.
    for name in NAMES {
        let field = ide.form_runtime_field(id, None, &format!("Vba_{name}"))?;
        assert!(matches!(field, Value::Object(_)), "{name}: {field:?}");
        if name != "Loaded" {
            assert_eq!(
                ide.form_runtime_field(id, None, &format!("Vba{name}"))?,
                field
            );
        }
    }
    assert_eq!(
        ide.form_runtime_field(id, None, "VbaLoaded")?,
        Value::Boolean(true)
    );
    Ok(())
}

#[test]
fn native_pages_and_controls_named_like_generated_members_run() -> R {
    let mut ide = Ide::new();
    ide.import_native_form_export(
        "NativeForm.frm",
        &fixture::export(),
        1252,
        &[NativeFormResource {
            name: "NativeForm.frx".into(),
            bytes: fixture::multipage_companion(0),
        }],
    )?;
    let id = ide.active_module().ok_or("source")?.id;
    // MultiPage 20, its first Page 31 and that page's TextBox 40.
    for (control, name) in [(20, "Lookup"), (31, "Enter"), (40, "MouseDown")] {
        let mut c = ide
            .form(id)
            .ok_or("form")?
            .form()
            .controls
            .iter()
            .find(|c| c.id == control)
            .ok_or("control")?
            .clone();
        c.name = name.into();
        ide.edit_form(id, DesignEdit::Replace(c))?;
    }
    ide.add_module(
        "Main.bas",
        "Sub Main()\nDebug.Print NativeForm.Lookup.Enter.Name, NativeForm.Lookup.Pages(0).Name, NativeForm.MouseDown.Parent.Name\nEnd Sub\n",
    )?;
    ide.dispatch(Action::Run, &mut NoHost)?;
    settle(&mut ide)?;
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    assert_eq!(ide.output(), vec!["initialized", "Enter\tEnter\tEnter"]);
    Ok(())
}
