# aquamarine shim

Teloxide 0.17 applies `aquamarine::aquamarine` only while building its API
documentation. Aquamarine 0.6 depends on the abandoned `proc-macro-error2`
crate, so nswarm patches that docs-only macro to this pass-through attribute.

Runtime teloxide code is unchanged. Rustdoc retains the annotated item but does
not transform Mermaid blocks. Remove this patch when teloxide drops or replaces
the upstream dependency.
