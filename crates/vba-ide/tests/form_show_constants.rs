//! `UserForm.Show vbModeless` compiles from another module: FormShowConstants
//! are VBA library constants, not generated project constants.
#![allow(clippy::indexing_slicing)]
#[path = "../../vba-project/tests/support/native_forms.rs"]
mod fixture;
#[allow(clippy::unwrap_used)]
#[path = "../../vba-project/tests/support/mod.rs"]
mod project_fixture;
use vba_ide::{Action, Ide, NoHost};
type R = Result<(), Box<dyn std::error::Error>>;

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

fn compile(launch: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let original = project();
    let project = vba_project::decode(&original, &vba_project::Limits::default())?;
    let form = project.modules[0].form.clone().ok_or("native")?;
    let mut ide = Ide::new();
    ide.attach_native_form(form)?;
    ide.set_text(fixture::SOURCE.to_string())?;
    ide.add_module("Launch.bas", launch)?;
    ide.dispatch(Action::Compile, &mut NoHost)?;
    Ok(ide.diagnostics.iter().map(|d| d.message.clone()).collect())
}

#[test]
fn show_with_form_show_constants_compiles_from_another_module() -> R {
    for constant in ["vbModeless", "vbModal"] {
        let launch = format!("Option Explicit\nSub Main()\nNativeForm.Show {constant}\nEnd Sub\n");
        assert_eq!(compile(&launch)?, Vec::<String>::new(), "{constant}");
    }
    Ok(())
}

#[test]
fn a_project_constant_named_vb_modeless_shadows_the_library_constant() -> R {
    let launch = "Option Explicit\nPublic Const vbModeless As Long = 0\nSub Main()\nNativeForm.Show vbModeless\nEnd Sub\n";
    assert_eq!(compile(launch)?, Vec::<String>::new());
    Ok(())
}
