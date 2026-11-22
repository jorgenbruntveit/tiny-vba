//! UserForm InsideWidth/InsideHeight return the client region, read-only.
use vba_ide::forms::{Bounds, ControlKind, DesignEdit};
use vba_ide::{Action, Ide, Mode, NoHost};
type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn run(code: &str) -> R<Ide> {
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
        "Sub Main()\nDim host As Object\nLoad F\nSet host = F.CheckBox1.Parent\nDebug.Print host.InsideWidth = F.Width, host.InsideHeight = F.Height\nF.Height = F.Height + 30\nDebug.Print host.InsideHeight = F.Height\nDebug.Print TypeName(F.InsideHeight)\nEnd Sub",
    )?;
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    assert_eq!(ide.output(), ["True\tTrue", "True", "Single"]);
    Ok(())
}

#[test]
fn inside_height_cannot_be_assigned() {
    // Read-only: the compiler refuses the assignment before anything runs.
    let error = run("Sub Main()\nLoad F\nF.InsideHeight = 10\nEnd Sub")
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(error.contains("VBA0110"), "{error}");
}
