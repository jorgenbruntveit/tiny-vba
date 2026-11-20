//! Frames of embedding-generated procedures alias only the module variables they name.
use std::collections::BTreeMap;
use std::error::Error;
use vba_diagnostics::{SourceFile, SourceId};
use vba_semantics::{BoundProgram, bind_with_generated_code};
use vba_syntax::{ModuleKind, parse};
type R = Result<(), Box<dyn Error>>;

const AUTHORED: &str = "Attribute VB_Name = \"Panel\"\nOption Explicit\nPrivate First As Object\nPrivate Second As Long\nPrivate Third As String\nPublic Sub Authored()\nEnd Sub\n";
const GENERATED: &str = "Public Sub GeneratedNamesSecond()\nSecond = 2\nEnd Sub\nPublic Sub GeneratedNamesNone()\nEnd Sub\n";

fn bind(generated: &BTreeMap<SourceId, u32>) -> Result<BoundProgram, Box<dyn Error>> {
    let source = format!("{AUTHORED}{GENERATED}");
    let class = parse(
        &SourceFile::new(SourceId(1), "Panel.cls".into(), source)?,
        ModuleKind::Class,
    )?;
    let entry = parse(
        &SourceFile::new(
            SourceId(2),
            "Launch.bas".into(),
            "Attribute VB_Name = \"Launch\"\nPublic Totals(1 To 3) As Long\nSub Main()\nEnd Sub\nSub Fill()\nEnd Sub\n".into(),
        )?,
        ModuleKind::Standard,
    )?;
    Ok(bind_with_generated_code(
        &[class, entry],
        "Main",
        &[],
        &vba_foreign::ReferenceSet::default(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        generated,
    )?)
}
fn fields(program: &BoundProgram, name: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let procedure = program
        .procedures
        .iter()
        .find(|p| p.signature.name.eq_ignore_ascii_case(name))
        .ok_or("procedure")?;
    Ok(procedure
        .locals
        .iter()
        .filter(|l| l.field.is_some() || l.persistent.is_some())
        .map(|l| l.name.clone())
        .collect())
}
#[test]
fn generated_procedures_alias_only_the_module_variables_they_name() -> R {
    let start = u32::try_from(AUTHORED.len())?;
    let program = bind(&BTreeMap::from([(SourceId(1), start), (SourceId(2), 0)]))?;
    assert_eq!(fields(&program, "Authored")?, ["First", "Second", "Third"]);
    assert_eq!(fields(&program, "GeneratedNamesSecond")?, ["Second"]);
    assert!(fields(&program, "GeneratedNamesNone")?.is_empty());
    // A standard-module array keeps its alias in generated code: it is
    // initialized through that frame slot.
    assert_eq!(fields(&program, "Fill")?, ["Totals"]);
    Ok(())
}
#[test]
fn without_generated_code_every_frame_aliases_every_module_variable() -> R {
    let program = bind(&BTreeMap::new())?;
    for name in ["Authored", "GeneratedNamesSecond", "GeneratedNamesNone"] {
        assert_eq!(fields(&program, name)?, ["First", "Second", "Third"]);
    }
    Ok(())
}
