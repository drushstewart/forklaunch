#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, Path, parse_macro_input};

#[proc_macro_derive(ComplianceMetadata, attributes(compliance))]
pub fn derive_compliance_metadata(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn assign<T>(
    slot: &mut Option<T>,
    value: T,
    meta: &syn::meta::ParseNestedMeta<'_>,
) -> syn::Result<()> {
    if slot.replace(value).is_some() {
        return Err(meta.error("duplicate compliance option"));
    }
    Ok(())
}

fn checked_name(value: LitStr) -> syn::Result<LitStr> {
    if value.value().trim().is_empty() || value.value().chars().any(char::is_control) {
        return Err(syn::Error::new(
            value.span(),
            "expected a nonempty compliance name without control characters",
        ));
    }
    Ok(value)
}

fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let mut entity = None;
    let mut crate_path = None;
    for attr in &input.attrs {
        if attr.path().is_ident("compliance") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("entity") {
                    let value = checked_name(meta.value()?.parse()?)?;
                    assign(&mut entity, value, &meta)
                } else if meta.path.is_ident("crate_path") {
                    let value: LitStr = meta.value()?.parse()?;
                    assign(&mut crate_path, value.parse::<Path>()?, &meta)
                } else {
                    Err(meta.error("expected entity or crate_path on a struct"))
                }
            })?;
        }
    }
    let name = input.ident;
    let entity = entity
        .unwrap_or_else(|| LitStr::new(name.to_string().trim_start_matches("r#"), name.span()));
    let crate_path = crate_path.unwrap_or_else(|| syn::parse_quote!(::forklaunch_core));
    let Data::Struct(data) = input.data else {
        return Err(syn::Error::new(
            name.span(),
            "ComplianceMetadata requires a named struct",
        ));
    };
    let Fields::Named(fields) = data.fields else {
        return Err(syn::Error::new(
            name.span(),
            "ComplianceMetadata requires named fields",
        ));
    };
    let mut storage_names = BTreeSet::new();
    let mut entries = Vec::new();
    for field in fields.named {
        let ident = field.ident.expect("named field");
        let rust_name = LitStr::new(ident.to_string().trim_start_matches("r#"), ident.span());
        let mut level = None;
        let mut storage_name = None;
        let mut nullable = None;
        for attr in &field.attrs {
            if attr.path().is_ident("compliance") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("level") {
                        let value: LitStr = meta.value()?.parse()?;
                        let variant = match value.value().as_str() {
                            "none" => quote!(None),
                            "pii" => quote!(Pii),
                            "phi" => quote!(Phi),
                            "pci" => quote!(Pci),
                            _ => {
                                return Err(syn::Error::new(
                                    value.span(),
                                    "expected none, pii, phi, or pci",
                                ));
                            }
                        };
                        assign(&mut level, variant, &meta)
                    } else if meta.path.is_ident("name") {
                        let value = checked_name(meta.value()?.parse()?)?;
                        assign(&mut storage_name, value, &meta)
                    } else if meta.path.is_ident("nullable") {
                        assign(&mut nullable, true, &meta)
                    } else {
                        Err(meta.error("expected level, name, or nullable on a field"))
                    }
                })?;
            }
        }
        let storage_name = storage_name.unwrap_or_else(|| rust_name.clone());
        if !storage_names.insert(storage_name.value()) {
            return Err(syn::Error::new(
                storage_name.span(),
                "duplicate compliance storage name",
            ));
        }
        let level = level.unwrap_or_else(|| quote!(None));
        let nullable = nullable.unwrap_or(false);
        entries.push(quote! {
            #crate_path::persistence::compliance::ComplianceField {
                rust_name: #rust_name, storage_name: #storage_name,
                level: #crate_path::persistence::compliance::ComplianceLevel::#level,
                nullable: #nullable,
            }
        });
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #crate_path::persistence::compliance::ComplianceMetadata
            for #name #ty_generics #where_clause {
            const ENTITY: &'static str = #entity;
            const FIELDS: &'static [#crate_path::persistence::compliance::ComplianceField] = &[#(#entries),*];
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_metadata_fails_closed() {
        for source in [
            "enum Entity { A }",
            "struct Entity(String);",
            "struct Entity;",
            "#[compliance(level = \"pii\")] struct Entity { id: String }",
            "#[compliance(entity = \"\")] struct Entity { id: String }",
            "#[compliance(entity = \"One\", entity = \"Two\")] struct Entity { id: String }",
            "struct Entity { #[compliance(level = \"unknown\")] id: String }",
            "struct Entity { #[compliance(nullable, nullable)] id: String }",
            "struct Entity { #[compliance(nullable = false)] id: String }",
            "struct Entity { #[compliance(encrypted)] id: String }",
            "struct Entity { #[compliance(name = \"\")] id: String }",
            "struct Entity { id: String, #[compliance(name = \"id\")] other: String }",
        ] {
            assert!(expand(syn::parse_str(source).unwrap()).is_err(), "{source}");
        }
    }
}
