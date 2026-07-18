//! Derive macros for Martin's configuration types.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, GenericParam, Generics, parse_macro_input};

/// Derives `CollectUnrecognizedKeys` for a config struct or enum.
///
/// Recurses into every field; `#[serde(flatten)]` fields add no path segment, `#[serde(skip)]`
/// fields are ignored, and `#[serde(rename)]` sets a field's path segment.
#[proc_macro_derive(CollectUnrecognizedKeys)]
pub fn derive_collect_unrecognized_keys(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives an empty `ConfigurationLivecycleHooks` impl, so the type opts into the trait's default hooks.
///
/// Types that need custom finalization implement the trait by hand instead of deriving it.
#[proc_macro_derive(ConfigurationLivecycleHooks)]
pub fn derive_configuration_livecycle_hooks(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        #[automatically_derived]
        impl #impl_generics crate::config::file::ConfigurationLivecycleHooks
            for #ident #ty_generics #where_clause {}
    }
    .into()
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    if let Some(attr) = container_rename_all(&input.attrs) {
        return Err(syn::Error::new_spanned(
            attr,
            "CollectUnrecognizedKeys does not support `#[serde(rename_all)]` on recursed types; \
             rename individual fields with `#[serde(rename = \"…\")]` instead",
        ));
    }

    let body = match &input.data {
        Data::Struct(data) => struct_body(&data.fields)?,
        Data::Enum(data) => enum_body(data),
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "CollectUnrecognizedKeys cannot be derived for unions",
            ));
        }
    };

    let ident = &input.ident;
    let generics = add_trait_bounds(input.generics.clone());
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        const _: () = {
            use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys};

            #[automatically_derived]
            #[allow(unused_variables)]
            impl #impl_generics CollectUnrecognizedKeys for #ident #ty_generics #where_clause {
                fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
                    #body
                }
            }
        };
    })
}

/// Adds `T: CollectUnrecognizedKeys` to every generic type parameter.
fn add_trait_bounds(mut generics: Generics) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(type_param) = param {
            type_param
                .bounds
                .push(syn::parse_quote!(CollectUnrecognizedKeys));
        }
    }
    generics
}

fn struct_body(fields: &Fields) -> syn::Result<TokenStream2> {
    let fields = match fields {
        Fields::Named(fields) => fields,
        Fields::Unit => return Ok(quote! {}),
        Fields::Unnamed(_) => {
            return Err(syn::Error::new_spanned(
                fields,
                "CollectUnrecognizedKeys cannot be derived for tuple structs; implement it manually",
            ));
        }
    };

    let mut stmts = Vec::new();
    for field in &fields.named {
        if has_serde_skip(&field.attrs) {
            continue;
        }
        let member = field.ident.as_ref().expect("named field has an ident");
        if has_serde_flatten(&field.attrs) {
            stmts.push(quote! {
                CollectUnrecognizedKeys::collect_unrecognized(&self.#member, path, out);
            });
        } else {
            let name = serde_field_name(field, member);
            stmts.push(quote! {
                CollectUnrecognizedKeys::collect_unrecognized(
                    &self.#member,
                    &format!("{path}{}.", #name),
                    out,
                );
            });
        }
    }
    Ok(quote! { #(#stmts)* })
}

fn enum_body(data: &syn::DataEnum) -> TokenStream2 {
    let mut arms = Vec::new();
    for variant in &data.variants {
        let variant_ident = &variant.ident;
        match &variant.fields {
            Fields::Unit => arms.push(quote! { Self::#variant_ident => {} }),
            Fields::Unnamed(fields) => {
                let bindings: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| quote::format_ident!("field{i}"))
                    .collect();
                let recurse = bindings.iter().map(|binding| {
                    quote! {
                        CollectUnrecognizedKeys::collect_unrecognized(#binding, path, out);
                    }
                });
                arms.push(quote! {
                    Self::#variant_ident(#(#bindings),*) => { #(#recurse)* }
                });
            }
            Fields::Named(fields) => {
                let members: Vec<_> = fields
                    .named
                    .iter()
                    .map(|f| f.ident.as_ref().expect("named field has an ident"))
                    .collect();
                let recurse = fields.named.iter().map(|f| {
                    let member = f.ident.as_ref().expect("named field has an ident");
                    let name = member.to_string();
                    quote! {
                        CollectUnrecognizedKeys::collect_unrecognized(
                            #member,
                            &format!("{path}{}.", #name),
                            out,
                        );
                    }
                });
                arms.push(quote! {
                    Self::#variant_ident { #(#members),* } => { #(#recurse)* }
                });
            }
        }
    }
    quote! { match self { #(#arms)* } }
}

/// Whether a field carries `#[serde(flatten)]`, meaning its keys live at the parent level.
fn has_serde_flatten(attrs: &[syn::Attribute]) -> bool {
    serde_flag_is_set(attrs, "flatten")
}

fn has_serde_skip(attrs: &[syn::Attribute]) -> bool {
    serde_flag_is_set(attrs, "skip")
}

/// Returns `true` if any `#[serde(...)]` attribute contains the bare flag `name`.
fn serde_flag_is_set(attrs: &[syn::Attribute], name: &str) -> bool {
    let mut found = false;
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident(name) {
                found = true;
            }
            if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            }
            Ok(())
        });
    }
    found
}

fn serde_field_name(field: &syn::Field, member: &syn::Ident) -> String {
    for attr in &field.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut rename = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                rename = Some(lit.value());
            } else if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            }
            Ok(())
        });
        if let Some(rename) = rename {
            return rename;
        }
    }
    member.to_string()
}

/// Returns the offending `#[serde(...)]` attribute if it sets `rename_all`, so the derive can reject it (unsupported).
fn container_rename_all(attrs: &[syn::Attribute]) -> Option<&syn::Attribute> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                found = true;
            }
            if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            }
            Ok(())
        });
        if found {
            return Some(attr);
        }
    }
    None
}
