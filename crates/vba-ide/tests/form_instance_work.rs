//! A form load's instance bookkeeping (reachability sweeps) is bounded by
//! `Limits::instance_work`, separately from the program's own instructions.
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
fn instance_work_is_not_charged_to_the_instruction_budget() -> R {
    // About 2,100,000 units of instance work, but under 1,000,000 instructions.
    let limits = Limits {
        instance_work: 50_000_000,
        ..Limits::default()
    };
    let ide = load_form_with_check_boxes(40, limits)?;
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    assert_eq!(ide.output(), ["loaded"]);
    Ok(())
}

#[test]
fn the_instance_work_limit_bounds_a_form_load() -> R {
    let limits = Limits {
        instance_work: 100_000,
        ..Limits::default()
    };
    let ide = load_form_with_check_boxes(40, limits)?;
    assert_ne!(ide.mode(), Mode::Completed);
    assert!(
        ide.diagnostics
            .iter()
            .any(|d| d.message == "execution resource limit"),
        "{:?}",
        ide.diagnostics
    );
    Ok(())
}

#[test]
fn the_ide_refuses_a_zero_or_oversized_instance_work_limit() {
    for instance_work in [0, 1_000_000_001] {
        let mut ide = Ide::new();
        assert!(
            ide.configure_execution_limits(Limits {
                instance_work,
                ..Limits::default()
            })
            .is_err()
        );
    }
}
