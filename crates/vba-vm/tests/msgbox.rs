//! VBA library MsgBox call statements and enums. Independently authored.
use std::error::Error;
use vba_compiler::{CompileOptions, compile};
use vba_diagnostics::{SourceFile, SourceId};
use vba_foreign::{Completion, Reply};
use vba_ir as ir;
use vba_values::{Value, VbaString};
use vba_vm::{self as vm, HostBinding, Limits, NoHost, Outcome, ReturnValue, Session};
type R<T = ()> = Result<T, Box<dyn Error>>;
const HOST: u64 = 47;
fn program(text: &str) -> R<ir::VerifiedProgram> {
    let source = SourceFile::new(SourceId(7), "msgbox.bas".into(), text.into())?;
    Ok(compile(
        &source,
        &CompileOptions {
            capabilities: vm::capabilities(),
            ..Default::default()
        },
    )?)
}

fn answer(source: &str, expected: Value) -> R {
    let mut s = Session::new(program(source)?, 48, vec![], None, Limits::default())?;
    let mut out = Outcome::Yielded;
    for _ in 0..1000 {
        out = s.run_slice(&mut NoHost, 64, &mut |_| Ok(false))?;
        if out != Outcome::Yielded {
            break;
        }
    }
    assert_eq!(
        out,
        Outcome::Completed(ReturnValue::Function(expected)),
        "{source}"
    );
    Ok(())
}

#[test]
fn msgbox_and_doevents_called_as_statements_dispatch_once_and_discard_results() -> R {
    struct PromptHost {
        calls: Vec<(String, Vec<Option<Value>>)>,
        wait: bool,
        resumes: usize,
    }
    impl vba_foreign::Host for PromptHost {
        fn identity(&self) -> u64 {
            HOST
        }
        fn invoke(&mut self, _: vba_foreign::HostCall) -> Reply {
            Reply::Error(vba_foreign::ForeignError {
                number: 438,
                description: "unexpected external member".into(),
            })
        }
        fn invoke_intrinsic(&mut self, call: vba_foreign::HostIntrinsicCall) -> Reply {
            self.calls.push((call.intrinsic, call.arguments));
            if self.wait {
                Reply::Wait(9)
            } else {
                self.resume(9)
            }
        }
        fn resume(&mut self, token: u64) -> Reply {
            assert_eq!(token, 9);
            self.resumes += 1;
            Reply::Complete(Completion {
                value: Value::Integer(1),
                writes: vec![],
            })
        }
        fn cancel(&mut self, _: u64) {}
    }
    for wait in [false, true] {
        let p = program(
            "Function Answer() As Long\nMsgBox \"Done\", 64\nCall MsgBox(\"Again\")\nDoEvents\nAnswer=7\nEnd Function",
        )?;
        let mut s = Session::new(
            p,
            106,
            vec![],
            Some(HostBinding {
                roots: Default::default(),
                identity: HOST,
                permitted: Default::default(),
            }),
            Limits::default(),
        )?;
        let mut host = PromptHost {
            calls: vec![],
            wait,
            resumes: 0,
        };
        let mut out = Outcome::Yielded;
        for _ in 0..100 {
            out = if out == Outcome::Waiting {
                s.resume_host(&mut host)?
            } else {
                s.run_slice(&mut host, 128, &mut |_| Ok(false))?
            };
            if !matches!(out, Outcome::Yielded | Outcome::Waiting) {
                break;
            }
        }
        assert_eq!(
            out,
            Outcome::Completed(ReturnValue::Function(Value::Long(7)))
        );
        let names = host
            .calls
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["MsgBox", "MsgBox", "DoEvents"], "wait={wait}");
        let first = &host.calls.first().ok_or("first call")?.1;
        assert_eq!(
            first.first(),
            Some(&Some(Value::String(vba_values::VbaString::from_utf8(
                "Done"
            )?)))
        );
        assert_eq!(first.get(1), Some(&Some(Value::Integer(64))));
        assert_eq!(host.resumes, 3);
        assert!(s.pending_host_token().is_none());
    }
    Ok(())
}

#[test]
fn msgbox_enums_are_long_and_their_constants_are_available_and_shadowable() -> R {
    // VbMsgBoxStyle: 0+1+2+3+4+5+16+32+48+64+0+256+512+768+0+4096+16384+65536+524288+1048576.
    answer(
        "Option Explicit\nFunction Answer() As Long\nAnswer=vbOKOnly+vbOKCancel+vbAbortRetryIgnore+vbYesNoCancel+vbYesNo+vbRetryCancel+vbCritical+vbQuestion+vbExclamation+vbInformation+vbDefaultButton1+vbDefaultButton2+vbDefaultButton3+vbDefaultButton4+vbApplicationModal+vbSystemModal+vbMsgBoxHelpButton+vbMsgBoxSetForeground+vbMsgBoxRight+vbMsgBoxRtlReading\nEnd Function",
        Value::Long(1_660_591),
    )?;
    // VbMsgBoxResult: vbOK=1 through vbNo=7, as distinct digits.
    answer(
        "Option Explicit\nFunction Answer() As Long\nAnswer=vbOK*1000000+vbCancel*100000+vbAbort*10000+vbRetry*1000+vbIgnore*100+vbYes*10+vbNo\nEnd Function",
        Value::Long(1_234_567),
    )?;
    // The constants are Long, like other VBA enum members.
    answer(
        "Function Answer() As Variant\nAnswer=TypeName(vbInformation) & TypeName(vbOKOnly) & TypeName(vbNo)\nEnd Function",
        Value::String(VbaString::from_utf8("LongLongLong")?),
    )?;
    // Both enum types declare Long locals, parameters and results.
    answer(
        "Option Explicit\nFunction Style(ByVal s As VbMsgBoxStyle) As VbMsgBoxResult\nStyle=s+vbYes\nEnd Function\nFunction Answer() As Variant\nDim r As vbmsgboxresult\nDim s As VbMsgBoxStyle\ns=vbYesNo+vbQuestion\nr=Style(s)\nAnswer=TypeName(r) & TypeName(s) & r\nEnd Function",
        Value::String(VbaString::from_utf8("LongLong42")?),
    )?;
    answer(
        "Option Explicit\nFunction Answer() As Long\nDim vbYes As Long\nvbYes=9\nAnswer=vbYes\nEnd Function",
        Value::Long(9),
    )?;
    let source = SourceFile::new(
        SourceId(7),
        "msgbox.bas".into(),
        "Function Answer() As Long\nDim r As VbMsgBoxResults\nEnd Function".into(),
    )?;
    let error = compile(
        &source,
        &CompileOptions {
            capabilities: vm::capabilities(),
            ..Default::default()
        },
    )
    .err()
    .ok_or("unknown enum type")?;
    assert_eq!(error.code, "VBA0132");
    Ok(())
}
