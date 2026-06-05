//! Derive macros for the Ogham typed-bindings boundary.
//!
//! - `#[derive(OghamRecord)]` — for structs that map to a `record`
//!   declared in a `.ogh` module. Generates an `OghamRecord` impl
//!   plus an `OghamField` impl so the type can be used inside
//!   `Vec<T>`, `Option<T>`, `HashMap<String, T>`, etc.
//! - `#[derive(OghamState)]` — for the top-level struct that
//!   mirrors a module's `host_state {}` block. Adds snapshot/diff
//!   methods that drive `inject_host_state_if_changed`.
//! - `#[derive(OghamMsg)]` — for the enum that mirrors a module's
//!   `events {}` block.
//!
//! Emitted code references types in the `ogham` crate via
//! `::ogham::runtime::schema::*` absolute paths. Users add both
//! `ogham` and `ogham-derive` as dependencies.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

mod attrs;
mod field_meta;
mod manifest_emit;

use field_meta::collect_struct_fields;

#[proc_macro_derive(OghamRecord, attributes(ogham))]
pub fn derive_ogham_record(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_ogham_record(input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

#[proc_macro_derive(OghamState, attributes(ogham))]
pub fn derive_ogham_state(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_ogham_state(input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

#[proc_macro_derive(OghamMsg, attributes(ogham))]
pub fn derive_ogham_msg(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_ogham_msg(input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

// -------------------------------------------------------------------
// OghamRecord
// -------------------------------------------------------------------

fn expand_ogham_record(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name_ident = &input.ident;
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => collect_struct_fields(&named.named)?,
            _ => {
                return Err(syn::Error::new_spanned(
                    name_ident,
                    "`OghamRecord` requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name_ident,
                "`OghamRecord` can only be derived for structs",
            ));
        }
    };
    let record_name_str =
        attrs::record_name_override(&input.attrs)?.unwrap_or_else(|| name_ident.to_string());

    let field_inserts = fields.iter().map(|f| {
        let ogham_name = &f.ogham_name;
        let ty = &f.ty;
        quote! {
            fields.insert(
                #ogham_name.to_string(),
                ::ogham::runtime::schema::FieldSchema {
                    ty: <#ty as ::ogham::runtime::schema::OghamField>::ogham_type_ref(),
                    default: None,
                    decl_span: ::ogham::parser::span::Span::zero(),
                },
            );
        }
    });

    let map_inserts = fields.iter().map(|f| {
        let rust_ident = &f.rust_ident;
        let ogham_name = &f.ogham_name;
        quote! {
            map.insert(
                #ogham_name.to_string(),
                <_ as ::ogham::runtime::schema::OghamField>::into_ogham_value(&self.#rust_ident),
            );
        }
    });

    Ok(quote! {
        impl ::ogham::runtime::schema::OghamRecord for #name_ident {
            const OGHAM_RECORD_NAME: &'static str = #record_name_str;

            fn ogham_record_schema() -> ::ogham::runtime::schema::RecordSchema {
                let mut fields: ::std::collections::BTreeMap<
                    ::std::string::String,
                    ::ogham::runtime::schema::FieldSchema,
                > = ::std::collections::BTreeMap::new();
                #(#field_inserts)*
                ::ogham::runtime::schema::RecordSchema {
                    fields,
                    decl_span: ::core::option::Option::None,
                }
            }
        }

        impl ::ogham::runtime::schema::OghamField for #name_ident {
            fn ogham_type_ref() -> ::ogham::runtime::schema::TypeRef {
                ::ogham::runtime::schema::TypeRef::Record(
                    <Self as ::ogham::runtime::schema::OghamRecord>::OGHAM_RECORD_NAME.to_string(),
                )
            }
            fn into_ogham_value(&self) -> ::ogham::runtime::value::Value {
                let mut map: ::std::collections::HashMap<
                    ::std::string::String,
                    ::ogham::runtime::value::Value,
                > = ::std::collections::HashMap::new();
                #(#map_inserts)*
                ::ogham::runtime::value::Value::Map(map)
            }
        }
    })
}

// -------------------------------------------------------------------
// OghamState — extends OghamRecord with snapshot/diff
// -------------------------------------------------------------------

fn expand_ogham_state(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name_ident = &input.ident;
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => collect_struct_fields(&named.named)?,
            _ => {
                return Err(syn::Error::new_spanned(
                    name_ident,
                    "`OghamState` requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name_ident,
                "`OghamState` can only be derived for structs",
            ));
        }
    };
    let record_name_str =
        attrs::record_name_override(&input.attrs)?.unwrap_or_else(|| name_ident.to_string());
    // P0-M3: when binding_module is set, emit a JSON manifest
    // describing the host_state shape for the schema-diagnostic
    // backend to consume.
    if let Some(binding_module) = attrs::binding_module_path(&input.attrs)? {
        let emit_fields: Vec<manifest_emit::StateField> = fields
            .iter()
            .map(|f| manifest_emit::StateField {
                name: f.ogham_name.clone(),
                ty: f.ty.clone(),
            })
            .collect();
        manifest_emit::emit_state_manifest(&name_ident.to_string(), &binding_module, &emit_fields);
    }

    // OghamState reuses the OghamRecord derive's schema/value
    // emission verbatim, so the user only writes #[derive(OghamState)]
    // and gets both impls.
    let field_inserts = fields.iter().map(|f| {
        let ogham_name = &f.ogham_name;
        let ty = &f.ty;
        quote! {
            fields.insert(
                #ogham_name.to_string(),
                ::ogham::runtime::schema::FieldSchema {
                    ty: <#ty as ::ogham::runtime::schema::OghamField>::ogham_type_ref(),
                    default: None,
                    decl_span: ::ogham::parser::span::Span::zero(),
                },
            );
        }
    });
    let snapshots = fields.iter().map(|f| {
        let rust_ident = &f.rust_ident;
        let ogham_name = &f.ogham_name;
        quote! {
            sink.set_value(
                #ogham_name,
                <_ as ::ogham::runtime::schema::OghamField>::into_ogham_value(&self.#rust_ident),
            );
        }
    });
    let diffs = fields.iter().map(|f| {
        let rust_ident = &f.rust_ident;
        let ogham_name = &f.ogham_name;
        quote! {
            if self.#rust_ident != prev.#rust_ident {
                sink.set_value(
                    #ogham_name,
                    <_ as ::ogham::runtime::schema::OghamField>::into_ogham_value(
                        &self.#rust_ident,
                    ),
                );
            }
        }
    });

    Ok(quote! {
        impl ::ogham::runtime::schema::OghamRecord for #name_ident {
            const OGHAM_RECORD_NAME: &'static str = #record_name_str;
            fn ogham_record_schema() -> ::ogham::runtime::schema::RecordSchema {
                let mut fields: ::std::collections::BTreeMap<
                    ::std::string::String,
                    ::ogham::runtime::schema::FieldSchema,
                > = ::std::collections::BTreeMap::new();
                #(#field_inserts)*
                ::ogham::runtime::schema::RecordSchema {
                    fields,
                    decl_span: ::core::option::Option::None,
                }
            }
        }

        impl ::ogham::runtime::schema::OghamState for #name_ident {
            fn ogham_snapshot_into(
                &self,
                sink: &mut dyn ::ogham::runtime::schema::HostStateSinkErased,
            ) {
                #(#snapshots)*
            }

            fn ogham_diff_apply(
                &self,
                prev: &Self,
                sink: &mut dyn ::ogham::runtime::schema::HostStateSinkErased,
            ) {
                #(#diffs)*
            }
        }
    })
}

// -------------------------------------------------------------------
// OghamMsg — for events enums
// -------------------------------------------------------------------

fn expand_ogham_msg(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name_ident = &input.ident;
    let variants = match &input.data {
        Data::Enum(e) => &e.variants,
        _ => {
            return Err(syn::Error::new_spanned(
                name_ident,
                "`OghamMsg` can only be derived for enums",
            ));
        }
    };
    let binding_module_opt = attrs::binding_module_path(&input.attrs)?;

    let mut variant_metas: Vec<VariantMeta> = Vec::new();
    for v in variants {
        let event_name = attrs::variant_event_name(v)?;
        let arg_types: Vec<syn::Type> = match &v.fields {
            Fields::Unit => Vec::new(),
            Fields::Unnamed(unnamed) => unnamed.unnamed.iter().map(|f| f.ty.clone()).collect(),
            Fields::Named(_) => {
                return Err(syn::Error::new_spanned(
                    v,
                    "OghamMsg variants must use tuple-style fields, not named fields",
                ));
            }
        };
        variant_metas.push(VariantMeta {
            rust_ident: v.ident.clone(),
            event_name,
            arg_types,
        });
    }

    // P0-M3: when binding_module is set, emit a JSON manifest
    // describing the events shape for the schema-diagnostic
    // backend to consume.
    if let Some(binding_module) = binding_module_opt {
        let emit_events: Vec<manifest_emit::EventSig> = variant_metas
            .iter()
            .map(|v| manifest_emit::EventSig {
                name: v.event_name.clone(),
                args: v.arg_types.clone(),
            })
            .collect();
        manifest_emit::emit_events_manifest(&name_ident.to_string(), &binding_module, &emit_events);
    }

    // const OGHAM_EVENTS — list of (name, arg-type-refs) pairs.
    let event_decls = variant_metas.iter().map(|v| {
        let event_name = &v.event_name;
        let arg_iters = v.arg_types.iter().map(|ty| {
            quote! { <#ty as ::ogham::runtime::schema::OghamField>::ogham_type_ref() }
        });
        quote! {
            (
                #event_name.to_string(),
                ::ogham::runtime::schema::EventSig {
                    args: vec![ #(#arg_iters),* ],
                    decl_span: ::ogham::parser::span::Span::zero(),
                },
            )
        }
    });

    // try_from_event match arms
    let try_arms = variant_metas.iter().map(|v| {
        let event_name = &v.event_name;
        let variant = &v.rust_ident;
        if v.arg_types.is_empty() {
            quote! {
                #event_name => {
                    if !args.is_empty() {
                        return ::core::option::Option::None;
                    }
                    ::core::option::Option::Some(Self::#variant)
                }
            }
        } else {
            let parses = v.arg_types.iter().enumerate().map(|(i, ty)| {
                quote! {
                    {
                        let v = args.get(#i)?;
                        <#ty as ::ogham::runtime::schema::FromOghamValue>::from_ogham_value(v)?
                    }
                }
            });
            let arity = v.arg_types.len();
            quote! {
                #event_name => {
                    if args.len() != #arity {
                        return ::core::option::Option::None;
                    }
                    ::core::option::Option::Some(Self::#variant( #(#parses),* ))
                }
            }
        }
    });

    Ok(quote! {
        impl ::ogham::runtime::schema::OghamMsg for #name_ident {
            fn ogham_events() -> ::std::collections::BTreeMap<
                ::std::string::String,
                ::ogham::runtime::schema::EventSig,
            > {
                let mut m: ::std::collections::BTreeMap<
                    ::std::string::String,
                    ::ogham::runtime::schema::EventSig,
                > = ::std::collections::BTreeMap::new();
                let pairs: Vec<(::std::string::String, ::ogham::runtime::schema::EventSig)> =
                    vec![ #(#event_decls),* ];
                for (k, v) in pairs {
                    m.insert(k, v);
                }
                m
            }

            fn try_from_ogham_event(
                name: &str,
                args: &[::ogham::runtime::value::Value],
            ) -> ::core::option::Option<Self> {
                match name {
                    #(#try_arms,)*
                    _ => ::core::option::Option::None,
                }
            }
        }
    })
}

struct VariantMeta {
    rust_ident: syn::Ident,
    event_name: String,
    arg_types: Vec<syn::Type>,
}

// `FromOghamValue` (the trait used by `try_from_ogham_event`)
// lives in the `ogham` crate at `ogham::runtime::schema` — the
// derive emits `<T as ::ogham::runtime::schema::FromOghamValue>::from_ogham_value(v)`
// calls. Keeping the trait + impls there means proc-macro builds
// stay free of any runtime deps.
