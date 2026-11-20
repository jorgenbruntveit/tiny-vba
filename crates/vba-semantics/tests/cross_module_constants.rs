//! A standard module's `Public Const` is visible from other modules; Private
//! and default (Private) constants are not. Locals, the module's own constants
//! and procedures come first; two exporting modules, or an exported constant
//! and another module's public procedure, make the name ambiguous.
use std::error::Error;
use vba_diagnostics::{Diagnostic, SourceFile, SourceId};
use vba_semantics::{BoundProgram, bind};
use vba_syntax::{ModuleKind, parse};
type R = Result<(), Box<dyn Error>>;

fn bind_modules(sources: &[&str]) -> Result<Result<BoundProgram, Diagnostic>, Box<dyn Error>> {
    let mut parsed = Vec::new();
    for (i, text) in sources.iter().enumerate() {
        let id = u64::try_from(i)? + 1;
        parsed.push(parse(
            &SourceFile::new(SourceId(id), format!("Module{id}.bas"), (*text).into())?,
            ModuleKind::Standard,
        )?);
    }
    Ok(bind(&parsed, "Main"))
}

/// The bound body of `Main`, for checking which constant a name bound to.
fn main_body(program: &BoundProgram) -> String {
    program
        .procedures
        .iter()
        .find(|p| p.signature.name == "Main")
        .map(|p| format!("{:?}", p.body))
        .unwrap_or_default()
}

fn message(result: Result<BoundProgram, Diagnostic>) -> String {
    result.err().map(|d| d.message).unwrap_or_default()
}

const SETTINGS: &str = "Attribute VB_Name = \"Settings\"\nPublic Const Limit As Long = 5\n";

#[test]
fn a_public_constant_of_another_standard_module_is_visible() -> R {
    for main in [
        "Attribute VB_Name = \"Report\"\nSub Main()\nx = Limit\nEnd Sub\n",
        "Attribute VB_Name = \"Report\"\nOption Explicit\nSub Main()\nDim x As Long\nx = Limit\nEnd Sub\n",
    ] {
        let program = bind_modules(&[SETTINGS, main])??;
        assert!(main_body(&program).contains("Constant(Long(5))"), "{main}");
    }
    // Still a constant: it cannot be assigned.
    let main = "Attribute VB_Name = \"Report\"\nSub Main()\nLimit = 6\nEnd Sub\n";
    assert_eq!(
        message(bind_modules(&[SETTINGS, main])?),
        "constant is not writable"
    );
    Ok(())
}

#[test]
fn private_and_default_constants_stay_in_their_module() -> R {
    let settings = "Attribute VB_Name = \"Settings\"\nConst Hidden = 3\nPrivate Const Secret = 4\n";
    for name in ["Hidden", "Secret"] {
        let explicit = format!(
            "Attribute VB_Name = \"Report\"\nOption Explicit\nSub Main()\nDim x\nx = {name}\nEnd Sub\n"
        );
        assert_eq!(
            message(bind_modules(&[settings, &explicit])?),
            format!("undeclared name {name}")
        );
        // Without Option Explicit the name is an implicit local, as in VBA.
        let implicit = format!("Attribute VB_Name = \"Report\"\nSub Main()\nx = {name}\nEnd Sub\n");
        let program = bind_modules(&[settings, &implicit])??;
        let main = program
            .procedures
            .iter()
            .find(|p| p.signature.name == "Main")
            .ok_or("Main")?;
        assert!(main.locals.iter().any(|l| l.name == name), "{name}");
    }
    Ok(())
}

#[test]
fn the_module_own_constant_and_procedure_come_first() -> R {
    let own_constant =
        "Attribute VB_Name = \"Report\"\nConst Limit = 9\nSub Main()\nx = Limit\nEnd Sub\n";
    let program = bind_modules(&[SETTINGS, own_constant])??;
    assert!(main_body(&program).contains("Constant(Integer(9))"));

    let own_function = "Attribute VB_Name = \"Report\"\nSub Main()\nx = Limit\nEnd Sub\nFunction Limit() As Long\nLimit = 1\nEnd Function\n";
    let program = bind_modules(&[SETTINGS, own_function])??;
    let body = main_body(&program);
    assert!(
        body.contains("Call {") && !body.contains("Constant(Long(5))"),
        "{body}"
    );
    Ok(())
}

#[test]
fn two_exports_or_a_constant_and_a_procedure_are_ambiguous() -> R {
    let other = "Attribute VB_Name = \"Defaults\"\nPublic Const Limit = 7\n";
    let main = "Attribute VB_Name = \"Report\"\nSub Main()\nx = Limit\nEnd Sub\n";
    assert_eq!(
        message(bind_modules(&[SETTINGS, other, main])?),
        "ambiguous name Limit"
    );
    // A public procedure of a third module, used as a value and as a call.
    let procedure =
        "Attribute VB_Name = \"Tools\"\nPublic Function Limit() As Long\nEnd Function\n";
    for body in ["x = Limit", "Limit", "Call Limit"] {
        let main = format!("Attribute VB_Name = \"Report\"\nSub Main()\n{body}\nEnd Sub\n");
        assert_eq!(
            message(bind_modules(&[SETTINGS, procedure, &main])?),
            "ambiguous name Limit",
            "{body}"
        );
    }
    Ok(())
}

#[test]
fn a_project_constant_shadows_library_procedures_and_is_not_callable() -> R {
    let one = "Attribute VB_Name = \"One\"\nPublic Const DoEvents = 1\n";
    let two = "Attribute VB_Name = \"Two\"\nPublic Const DoEvents = 2\n";
    for body in ["DoEvents", "Call DoEvents"] {
        let main = format!("Attribute VB_Name = \"Report\"\nSub Main()\n{body}\nEnd Sub\n");
        assert_eq!(
            message(bind_modules(&[one, two, &main])?),
            "ambiguous name DoEvents",
            "{body}"
        );
        assert_eq!(
            message(bind_modules(&[one, &main])?),
            "constant DoEvents is not a procedure",
            "{body}"
        );
    }
    Ok(())
}

#[test]
fn form_show_constants_are_library_constants_a_project_can_shadow() -> R {
    let main = "Attribute VB_Name = \"Report\"\nOption Explicit\nSub Main()\nDim x As Long\nx = vbModeless\nEnd Sub\n";
    assert!(main_body(&bind_modules(&[main])??).contains("Constant(Long(0))"));
    let modal = main.replace("vbModeless", "vbModal");
    assert!(main_body(&bind_modules(&[&modal])??).contains("Constant(Long(1))"));
    let own = "Attribute VB_Name = \"Settings\"\nPublic Const vbModeless As Long = 7\n";
    assert!(main_body(&bind_modules(&[own, main])??).contains("Constant(Long(7))"));
    Ok(())
}

#[test]
fn constant_initializers_do_not_yet_read_other_modules_constants() -> R {
    // Deferred: refused, never bound to a wrong value.
    for main in [
        "Attribute VB_Name = \"Report\"\nConst Twice = Limit * 2\nSub Main()\nx = Twice\nEnd Sub\n",
        "Attribute VB_Name = \"Report\"\nSub Main()\nConst Twice = Limit * 2\nx = Twice\nEnd Sub\n",
    ] {
        let result = bind_modules(&[SETTINGS, main])?;
        assert!(result.is_err(), "{main}");
    }
    Ok(())
}
