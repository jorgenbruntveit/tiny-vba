# 0104 — Frame aliases for embedding-generated code

Date: 2026-11-20. Status: adopted for issue #2.

The binder gives every executable procedure an alias local for every module
variable. The IDE's UserForm glue adds about four procedures and one or two
module variables per control, so a form's locals, nominal locals and verifier
metadata grew with the square of its controls. An Excel for Mac template with a
72-control UserForm exceeded `MAX_NOMINAL_LOCALS` and the 64 MiB verifier
metadata budget, and no edit could be exported.

`CompileOptions::generated_code` identifies embedding-generated code by source
identity and the byte offset where generated procedures start (0 for a wholly
generated source). The IDE marks the glue after each form's authored text and
its support classes. A generated procedure's frame aliases only the module
variables its tokens name; standard-module arrays keep their alias because they
are initialized through it. Authored procedures are unchanged, so Locals,
watches, declaration checks and the existing nominal-local ceiling tests keep
their behavior. Generated procedures are not authored breakpoint sites.

The verifier matches WithEvents handlers through a per-class name map, charged
once per class, with a bounded charge per field/event lookup. Ceilings and
budgets are unchanged.

Rejected: raising the limits; lazy aliases for all procedures, which changes
debugger inspection and declaration validation; skipping the compile before an
edited-copy export, which changes ADR 0098 and leaves such forms unrunnable.
