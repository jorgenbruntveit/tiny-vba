//! Loading a form ranks its controls in tab order. The ranking visits each
//! control a fixed number of times, so a form with many controls loads within
//! a budget that grows with the control count rather than its square.
use vba_ide::forms::{Bounds, ControlKind, DesignEdit};
use vba_ide::{Action, Ide, Limits, Mode, NoHost};
type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn load_form_with_check_boxes(controls: usize, limits: Limits) -> R<Ide> {
    let mut ide = Ide::new();
    ide.configure_execution_limits(limits)?;
    let id = ide.add_form("F")?;
    let mut top = 12.0;
    for _ in 0..controls {
        ide.edit_form(
            id,
            DesignEdit::Add(
                ControlKind::CheckBox,
                Bounds {
                    left: 12.0,
                    top,
                    width: 120.0,
                    height: 6.0,
                },
            ),
        )?;
        top += 6.0;
    }
    ide.add_module(
        "Main.bas",
        "Sub Main()\nLoad F\nDebug.Print \"loaded\"\nEnd Sub",
    )?;
    ide.dispatch(Action::Run, &mut NoHost)?;
    for _ in 0..100_000 {
        if ide.mode() != Mode::Running {
            break;
        }
        ide.tick(&mut NoHost)?;
    }
    Ok(ide)
}

#[test]
fn seventy_two_check_boxes_load_without_a_quadratic_tab_ranking() -> R {
    // The former nested TabIndex scan needed more than 10,000,000 units here.
    let limits = Limits {
        instructions: 3_000_000,
        ..Limits::default()
    };
    let ide = load_form_with_check_boxes(72, limits)?;
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    assert_eq!(ide.output(), ["loaded"]);
    Ok(())
}
