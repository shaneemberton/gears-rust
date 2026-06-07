# cyberware-modkit-gts-macros

Proc-macros backing the [`cyberware-modkit-gts`](../modkit-gts/README.md) crate. Not intended for direct use — depend on `cyberware-modkit-gts` instead; it re-exports everything below and carries the `inventory` collectors the macros target.

## What's here

- **`#[gts_type_schema(type_id = "...", …)]`** — attribute macro. Applies
  `#[gts_macros::struct_to_gts_schema(...)]` to the struct, emits an
  `InventorySchema` entry (the GTS Type Schema record) into the
  process-wide inventory, and — for derived unit structs
  (`base = ParentStruct`) — auto-emits `impl Default` so generic helpers
  can construct the marker without the caller re-spelling the type.
- **`gts_instance! { … }`** — function-like macro for **typed** Instance
  declarations. Takes a single struct literal `Struct { id: "<full>", … }`,
  optionally preceded by `#[gts_static(NAME)]`. Upstream rewrites the
  `id`-field string literal into a `GtsInstanceId` and asserts at compile
  time that its prefix equals `<Struct as GtsSchema>::TYPE_ID`. With
  `#[gts_static(NAME)]`, additionally emits `pub static NAME: LazyLock<T>`
  for typed runtime access.
- **`gts_instance_raw!({ … });`** — function-like macro for **raw-JSON**
  declarations. Takes a single brace-delimited JSON object literal whose
  top-level `"id"` key holds the full Instance Identifier. Use when no
  Rust struct corresponds to the instance.

Both macros resolve the `cyberware-modkit-gts` crate path at expansion time via
`proc_macro_crate`, so callers only need `cyberware-modkit-gts` as a dependency
— no separate dep on this crate.

## Full docs & examples

See **[`cyberware-modkit-gts` README](../modkit-gts/README.md)**:

- [Adding a platform base Type Schema](../modkit-gts/README.md#adding-a-platform-base-type-schema-inside-this-crate)
- [Declaring a well-known GTS instance](../modkit-gts/README.md#declaring-a-well-known-gts-instance) — preferred `segment` form, `instance_id` fallback, generic base types
- [Boundary with `types-registry`](../modkit-gts/README.md#boundary-with-types-registry)

Integration tests covering both macros live in
[`libs/modkit-gts/tests/macro_integration.rs`](../modkit-gts/tests/macro_integration.rs).
