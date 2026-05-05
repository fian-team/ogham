//! Resolve per-field metadata for `#[derive(OghamRecord)]` and
//! `#[derive(OghamState)]`. Filters out `#[ogham(skip)]` fields
//! and applies `#[ogham(rename = "...")]`.

use syn::{punctuated::Punctuated, token::Comma, Field, Ident, Type};

use crate::attrs::{field_name_override, field_skip};

pub(crate) struct FieldMeta {
    /// The field's identifier on the Rust side.
    pub rust_ident: Ident,
    /// The field's name on the `.ogh` side (after `#[ogham(rename)]`).
    pub ogham_name: String,
    /// The field's Rust type (used to look up `OghamField::ogham_type_ref()`).
    pub ty: Type,
}

pub(crate) fn collect_struct_fields(
    fields: &Punctuated<Field, Comma>,
) -> syn::Result<Vec<FieldMeta>> {
    let mut out = Vec::new();
    for field in fields {
        if field_skip(&field.attrs)? {
            continue;
        }
        let rust_ident = field
            .ident
            .clone()
            .ok_or_else(|| syn::Error::new_spanned(field, "named fields required"))?;
        let ogham_name = field_name_override(&field.attrs)?
            .unwrap_or_else(|| rust_ident.to_string());
        out.push(FieldMeta {
            rust_ident,
            ogham_name,
            ty: field.ty.clone(),
        });
    }
    Ok(out)
}
