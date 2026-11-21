# 0106 — Location store sized by `Limits::locations`

Date: 2026-11-21. Status: adopted for issue #16 (option b).

A session's owned locations were capped twice: by `Limits::locations` and by
fixed 4,096 caps in the `vba-values` store, in instance allocation and in frame
entry (`min(4096)` and `count > 4096`). A larger `Limits::locations` therefore
had no effect. Each loaded control instance takes about 75 locations, so a
UserForm with more than about 52 controls could not load in any configuration.

The store now allocates up to the capacity it is created with.
`Locations::new` keeps 4,096 for standalone callers; the VM creates its store
with `Locations::with_capacity(owner, limits.locations)` and the fixed caps are
removed, so `Limits::locations` is the single bound. The default rises from
4,096 to 65,536, room for several hundred controls. The IDE safety cap on
a configured value stays 1,000,000.

Unchanged: locations are never reused within a session, so a session that loads
and unloads forms repeatedly still uses up its budget (option c, slot reuse, was
not adopted). Instance, payload, instruction and stack limits are unchanged.
The store grows on allocation; a higher limit reserves nothing up front.
