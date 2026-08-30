//! Minimal pass-through for teloxide's docs-only `aquamarine` attribute.

use proc_macro::TokenStream;

/// Returns the annotated item unchanged.
///
/// Teloxide applies this attribute only under `cfg(doc)`. The Step 2 spike
/// does not render Mermaid diagrams, so retaining the item is the complete
/// required behavior while avoiding aquamarine's abandoned macro dependency.
#[proc_macro_attribute]
pub fn aquamarine(_attributes: TokenStream, item: TokenStream) -> TokenStream {
    item
}
