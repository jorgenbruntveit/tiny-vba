//! Each loaded control instance holds its fields in session locations, so a
//! form's control count is bounded by `Limits::locations`. The store follows
//! that limit; it has no separate fixed cap.
use vba_ide::forms::{Bounds, ControlKind, DesignEdit};
use vba_ide::{Action, Ide, Limits, Mode, NoHost};
type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn load_form_with_labels(controls: usize, limits: Limits) -> R<Ide> {
    let mut ide = Ide::new();
    ide.configure_execution_limits(limits)?;
    let id = ide.add_form("F")?;
    let mut top = 12.0;
    for _ in 0..controls {
        ide.edit_form(
            id,
            DesignEdit::Add(
                ControlKind::Label,
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

fn generous_instructions() -> Limits {
    Limits {
        instructions: 100_000_000,
        ..Limits::default()
    }
}

#[test]
fn a_form_with_sixty_labels_loads_with_the_default_locations() -> R {
    // About 75 locations per control: 56 Labels exceeded the former fixed 4,096.
    let ide = load_form_with_labels(60, generous_instructions())?;
    assert_eq!(ide.mode(), Mode::Completed, "{:?}", ide.diagnostics);
    assert_eq!(ide.output(), ["loaded"]);
    Ok(())
}

#[test]
fn the_locations_limit_bounds_a_form_load() -> R {
    let limits = Limits {
        locations: 2048,
        ..generous_instructions()
    };
    let ide = load_form_with_labels(60, limits)?;
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
