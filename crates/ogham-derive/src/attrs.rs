//! Attribute parsing helpers for the `#[ogham(...)]` macro
//! attributes on struct fields and enum variants.
//!
//! The parsing helpers walk every `#[ogham(...)]` list on the
//! given `attrs` slice in a single pass. This matters because
//! `syn`'s `parse_nested_meta` expects every nested item to be
//! consumed — if `flag_from_ogham_attr` saw `rename = "x"` and
//! didn't drain the `= "x"`, syn would error with "expected `,`"
//! at the `=`.

use syn::{Attribute, Lit, Variant};

#[derive(Default)]
pub(crate) struct OghamAttrs {
    pub rename: Option<String>,
    pub skip: bool,
}

/// Parse all `#[ogham(...)]` attributes on the given list. Each
/// nested entry is recognized exhaustively (currently `rename =
/// "..."` and `skip`); unknown entries are an error so typos like
/// `#[ogham(skipp)]` surface immediately.
pub(crate) fn parse_ogham_attrs(attrs: &[Attribute]) -> syn::Result<OghamAttrs> {
    let mut out = OghamAttrs::default();
    for attr in attrs {
        if !attr.path().is_ident("ogham") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    out.rename = Some(s.value());
                    return Ok(());
                }
                return Err(meta.error("`rename` requires a string literal"));
            }
            if meta.path.is_ident("skip") {
                out.skip = true;
                return Ok(());
            }
            // Tolerate `#[ogham(default)]` for forward-compat with
            // the design's documented field attribute (it has no
            // codegen effect today; it just documents that the
            // matching `.ogh`-side field has a default).
            if meta.path.is_ident("default") {
                return Ok(());
            }
            Err(meta.error("unknown ogham attribute"))
        })?;
    }
    Ok(out)
}

/// Parse `#[ogham(rename = "Foo")]` at the struct/enum level.
pub(crate) fn record_name_override(attrs: &[Attribute]) -> syn::Result<Option<String>> {
    Ok(parse_ogham_attrs(attrs)?.rename)
}

/// Parse `#[ogham(rename = "...")]` on a single field.
pub(crate) fn field_name_override(attrs: &[Attribute]) -> syn::Result<Option<String>> {
    Ok(parse_ogham_attrs(attrs)?.rename)
}

/// Parse `#[ogham(skip)]` on a field.
pub(crate) fn field_skip(attrs: &[Attribute]) -> syn::Result<bool> {
    Ok(parse_ogham_attrs(attrs)?.skip)
}

/// Resolve an enum variant's event name. Defaults to the snake_case
/// form of the variant ident; can be overridden via
/// `#[ogham(rename = "...")]`.
pub(crate) fn variant_event_name(v: &Variant) -> syn::Result<String> {
    if let Some(s) = parse_ogham_attrs(&v.attrs)?.rename {
        return Ok(s);
    }
    Ok(to_snake_case(&v.ident.to_string()))
}

/// Convert a CamelCase identifier to snake_case. Used to default
/// `OghamMsg` variant names to the `.ogh`-side convention without
/// requiring `#[ogham(rename = ...)]` on every variant.
fn to_snake_case(camel: &str) -> String {
    let mut out = String::with_capacity(camel.len() + 4);
    for (i, ch) in camel.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            for low in ch.to_lowercase() {
                out.push(low);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::to_snake_case;

    #[test]
    fn snake_case_basic() {
        assert_eq!(to_snake_case("Foo"), "foo");
        assert_eq!(to_snake_case("FooBar"), "foo_bar");
        assert_eq!(to_snake_case("SetMasterVolume"), "set_master_volume");
        assert_eq!(to_snake_case("Close"), "close");
    }
}
