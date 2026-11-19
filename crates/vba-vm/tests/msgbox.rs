//! VBA library MsgBox call statements and enums. Independently authored.
use std::error::Error;
use vba_compiler::{CompileOptions, compile};
use vba_diagnostics::{SourceFile, SourceId};
use vba_foreign::{Completion, Reply};
use vba_ir as ir;
use vba_values::Value;
use vba_vm::{self as vm, HostBinding, Limits, Outcome, ReturnValue, Session};
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
