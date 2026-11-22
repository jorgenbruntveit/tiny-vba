# 0107 — Instance work bounded by its own limit

Date: 2026-11-22. Status: adopted for issue #16 (option e) as an interim
measure while reference counting for project instances (option f) is assessed.

`Counters::instance_work` counts the VM's own bookkeeping for project
instances: reachability sweeps in `settle_instances`, event sink scans, class
lookups and project views passed to the host. It was checked against
`Limits::instructions`, the budget for the program's own bytecode. A sweep
visits every live instance and reference field, and it runs at each checkpoint
after a push, call, return or object write, so its cost grows with the square
of the live instance count. Loading a UserForm with 72 CheckBoxes took
187,458 bytecode instructions but 10,394,791 units of instance work, and could
not load with the default 1,000,000 budget.

`Limits::instance_work` now bounds that counter, with a default of 10,000,000.
`Limits::instructions` bounds only bytecode instructions, type checks and
enumeration work, as before. The IDE safety cap on a configured value is
1,000,000,000, as for instructions, and Immediate evaluation keeps 10,000 for
both.

The work stays bounded and deterministic, and the counter survives soft
yields. In a release build, 10,000,000 units take roughly 0.1 s. Reference
counting (option f) would make most sweeps unnecessary; cycle collection
would still need this bound.
