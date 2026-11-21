# 0107 — Incremental project instance lifetime

Date: 2026-11-21. Status: adopted for issue #16 (option f).

The VM checked for unreferenced project instances by scanning frames, locations
and the instance heap at statement checkpoints. Initializing a large UserForm
repeated that work for each control and exhausted the instruction budget.

The canonical location store updates counts on allocation, replacement and
invalidation. This includes individual array element writes and committed
foreign ByRef patches. The VM counts transient references in operand stacks,
receivers, captured aliases, enumeration snapshots, event deliveries and
callback state. Field aliases keep their owning instance alive.

The VM processes zero-count instances before the next statement or normal
completion. Class_Terminate executes as an ordinary VM frame. Retiring an
instance releases its fields, which can make other instances eligible.
Strong cycles keep their counts until a link is explicitly broken or the
project state is torn down. Reset and abnormal teardown remain inert. Retired
instance identities cannot address new instances after Reset.

The instruction limit still applies to lifecycle bytecode and the existing
instance work counter still bounds class lookups, events and host inspection.
References are tracked on mutation with work proportional to changed values.
Unchanged live instances and their fields do not contribute checkpoint work.
The generated form tab-order pass has its own cost and is outside this decision.
