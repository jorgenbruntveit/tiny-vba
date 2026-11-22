//! UserForm InsideWidth/InsideHeight return the client region, read-only.
use vba_ide::forms::{Bounds, ControlKind, DesignEdit};
use vba_ide::{Action, Ide, Mode, NoHost, SourceId};
type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn form() -> R<(Ide, SourceId)> {
    let mut ide = Ide::new();
    let id = ide.add_form("F")?;
    ide.edit_form(
        id,
        DesignEdit::Add(
            ControlKind::CheckBox,
            Bounds {
                left: 12.0,
                top: 12.0,
                width: 120.0,
                height: 18.0,
            },
        ),
    )?;
    Ok((ide, id))
}

fn run(mut ide: Ide, code: &str) -> R<Ide> {
    ide.add_module("Main.bas", code)?;
    ide.dispatch(Action::Run, &mut NoHost)?;
    for _ in 0..10_000 {
        if ide.mode() != Mode::Running {
            break;
        }
        ide.tick(&mut NoHost)?;
    }
    Ok(ide)
}

#[test]
fn inside_size_follows_the_client_size_through_a_late_bound_parent() -> R {
    let ide = run(
        form()?.0,
        "Sub Main()\nDim host As Object\nLoad F\nSet host = F.CheckBox1.Parent\nDebug.Print host.InsideWidth = F.Width, host.InsideHeight = F.Height\nF.Height = F.Height + 30\nDebug.Print host.InsideHeight = F.Height\nDebug.Print TypeName(F.InsideHeight)\nEnd Sub",
    )?;
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    assert_eq!(ide.output(), ["True\tTrue", "True", "Single"]);
    Ok(())
}

#[test]
fn inside_height_cannot_be_assigned() -> R {
    // Read-only: the compiler refuses the assignment before anything runs.
    let error = run(
        form()?.0,
        "Sub Main()\nLoad F\nF.InsideHeight = 10\nEnd Sub",
    )
    .err()
    .map(|e| e.to_string())
    .unwrap_or_default();
    assert!(error.contains("VBA0110"), "{error}");
    Ok(())
}

#[test]
fn existing_control_name_takes_precedence_over_inside_height() -> R {
    let (mut ide, id) = form()?;
    let mut control = ide
        .form(id)
        .and_then(|form| form.form().controls.first())
        .ok_or("control")?
        .clone();
    control.name = "iNsIdEhEiGhT".into();
    ide.edit_form(id, DesignEdit::Replace(control))?;
    let ide = run(
        ide,
        "Sub Main()\nDebug.Print F.InsideHeight.Name, F.InsideWidth = F.Width\nEnd Sub",
    )?;
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    assert_eq!(ide.output(), ["iNsIdEhEiGhT\tTrue"]);
    Ok(())
}

#[test]
fn authored_members_take_precedence_over_inside_size_getters() -> R {
    let (mut ide, _) = form()?;
    ide.set_text(
        "Attribute VB_Name = \"F\"\nPublic InsideHeight As Long\nPublic Property Get InsideWidth() As Long\nInsideWidth = 42\nEnd Property\n"
            .into(),
    )?;
    let ide = run(
        ide,
        "Sub Main()\nF.InsideHeight = 17\nDebug.Print F.InsideHeight, F.InsideWidth\nEnd Sub",
    )?;
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    assert_eq!(ide.output(), ["17\t42"]);
    Ok(())
}
