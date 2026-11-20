//! Compile cost of generated UserForm glue grows linearly with the form's controls.
#![allow(clippy::indexing_slicing)]
#[path = "../../vba-project/tests/support/native_forms.rs"]
mod fixture;
#[allow(clippy::unwrap_used)]
#[path = "../../vba-project/tests/support/mod.rs"]
mod project_fixture;
use vba_ide::{Action, Ide, NoHost};
use vba_project::forms::{Kind, new_designer_control};
type R = Result<(), Box<dyn std::error::Error>>;

const CONTROLS: u32 = 96;

fn project() -> Vec<u8> {
    let mut spec = project_fixture::Spec::new("NativeForm", "NativeForm", "BaseClass");
    spec.source = fixture::SOURCE.as_bytes().to_vec();
    let mut f = project_fixture::Fixture::new(1252, &[spec]);
    let marker = [0x25, 0, 0, 0, 0, 0];
    while let Some(at) = f.dir.windows(6).position(|w| w == marker) {
        f.dir.drain(at..at + 6);
    }
    f.streams.extend(
        fixture::streams()
            .into_iter()
            .map(|(name, bytes)| (format!("/NativeForm/{name}"), bytes)),
    );
    f.streams.push((
        "/NativeForm/\u{3}VBFrame".into(),
        fixture::frame(false).into_bytes(),
    ));
    f.bytes()
}

/// A form with many controls, module-level object variables and authored
/// handlers, as real templates have, compiles within the default compiler and
/// verifier limits. Before generated glue stopped aliasing every
/// module variable, the nominal-local and metadata charges grew quadratically.
#[test]
fn large_native_form_compiles_within_default_limits() -> R {
    let original = project();
    let project = vba_project::decode(&original, &vba_project::Limits::default())?;
    let mut form = project.modules[0].form.clone().ok_or("native")?;
    let mut declarations = String::new();
    let mut handlers = String::new();
    for i in 0..CONTROLS {
        let kind = [Kind::Label, Kind::TextBox, Kind::CommandButton][i as usize % 3];
        let name = format!("Ctl{i}");
        let mut control = new_designer_control(kind, 100 + i, &name)?;
        control.top = i32::try_from(i)? * 400;
        form.design.controls.push(control);
        declarations.push_str(&format!("Private Held{i} As Object\r\n"));
        if kind == Kind::CommandButton {
            handlers.push_str(&format!(
                "Private Sub {name}_Click()\r\nSet Held{i} = {name}\r\nEnd Sub\r\n"
            ));
        }
    }
    let source = fixture::SOURCE.replace("initialized", "edited").replace(
        "Option Explicit\r\n",
        &format!("Option Explicit\r\n{declarations}"),
    ) + &handlers;
    let mut ide = Ide::new();
    ide.attach_native_form(form)?;
    ide.set_text(source)?;
    ide.add_module(
        "Launch.bas",
        "Option Explicit\nSub Main()\nNativeForm.Ctl2.Caption = \"go\"\nEnd Sub\n",
    )?;
    ide.dispatch(Action::Compile, &mut NoHost)?;
    assert!(ide.diagnostics.is_empty(), "{:?}", ide.diagnostics);
    Ok(())
}
