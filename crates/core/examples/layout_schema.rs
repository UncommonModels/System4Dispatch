//! Print the layout file's JSON Schema.
//!
//! ```text
//! cargo run -p s4d-core --example layout_schema --features schema > docs/schema/layout-v1.schema.json
//! ```

fn main() {
    print!("{}", s4d_core::layout::json_schema_string());
}
