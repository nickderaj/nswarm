# aquamarine shim

Teloxide 0.17 applies `aquamarine::aquamarine` only while building its API
documentation. Aquamarine 0.6 depends on the abandoned `proc-macro-error2`
crate, so nswarm patches that docs-only macro to this pass-through attribute.

Runtime teloxide code is unchanged. Rustdoc retains the annotated item but does
not transform Mermaid blocks.

This is a repository-local Cargo patch, not a replacement dependency that
published nswarm crates can impose on consumers: Cargo does not propagate a
package's `[patch.crates-io]` table. A downstream build of a published crate
would therefore resolve Teloxide's upstream Aquamarine dependency unless that
downstream workspace applies its own patch. The patch is also graph-wide inside
this workspace, so every future use of `aquamarine` would receive this
pass-through macro. Cargo Vet and Cargo Deny consequently inspect this shim in
place of the displaced upstream Aquamarine package.

Remove the patch as soon as Teloxide drops or replaces Aquamarine. Track that
upstream work in [teloxide/teloxide#1475](https://github.com/teloxide/teloxide/issues/1475),
then remove both this directory and the root patch and re-run the complete
supply-chain and rustdoc gates.
