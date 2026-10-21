//! Derive macros for Martin's configuration types.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, GenericParam, Generics, Type, parse_macro_input};

/// Derives an empty `ConfigurationLivecycleHooks` impl, so the type opts into the trait's default hooks
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

/// Derives `CollectUnrecognizedKeys` for a config struct or enum.
///
/// Recurses into every field, except:
/// - `#[serde(flatten)]` fields add no path segment,
/// - `#[serde(skip)]` fields are ignored, and
/// - `#[serde(rename)]` sets a field's path segment.
#[proc_macro_derive(CollectUnrecognizedKeys)]
pub fn derive_collect_unrecognized_keys(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input).unwrap_or_else(syn::Error::into_compile_error).into()
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

/// Derives `DeserializeFields`, which reads one field's value from a map that also holds other keys.
#[proc_macro_derive(DeserializeFields)]
pub fn derive_deserialize_fields(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_deserialize_fields(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Serde field options that only affect serialization.
const SERIALIZE_ONLY_OPTIONS: &[&str] = &["skip_serializing", "skip_serializing_if", "serialize_with"];

/// Builds the `DeserializeFields` impl, one match arm per field key.
fn expand_deserialize_fields(input: &DeriveInput) -> syn::Result<TokenStream2> {
    if let Some(attr) = container_rename_all(&input.attrs) {
        return Err(syn::Error::new_spanned(
            attr,
            "DeserializeFields does not support `#[serde(rename_all)]`; rename individual fields instead",
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "DeserializeFields can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &data.fields,
            "DeserializeFields can only be derived for structs with named fields",
        ));
    };

    let mut arms = Vec::new();
    for field in &fields.named {
        reject_deserialize_options(&field.attrs)?;
        if !is_option(&field.ty) {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "DeserializeFields requires every field to be an `Option`",
            ));
        }
        let member = field.ident.as_ref().expect("named field has an ident");
        let name = serde_field_name(field, member);
        let cfgs = field.attrs.iter().filter(|attr| attr.path().is_ident("cfg"));
        arms.push(quote! {
            #(#cfgs)*
            #name => self.#member = ::serde::de::MapAccess::next_value(map)?,
        });
    }

    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics crate::config::file::DeserializeFields for #ident #ty_generics #where_clause {
            fn deserialize_field<'de, M: ::serde::de::MapAccess<'de>>(
                &mut self,
                key: &str,
                map: &mut M,
            ) -> ::core::result::Result<bool, M::Error> {
                match key {
                    #(#arms)*
                    _ => return ::core::result::Result::Ok(false),
                }
                ::core::result::Result::Ok(true)
            }
        }
    })
}

/// Fails on any `#[serde(...)]` field option other than a plain `rename` or a serialize-only one.
fn reject_deserialize_options(attrs: &[syn::Attribute]) -> syn::Result<()> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") && meta.input.peek(syn::Token![=]) {
                let _: syn::LitStr = meta.value()?.parse()?;
                return Ok(());
            }
            if SERIALIZE_ONLY_OPTIONS.iter().any(|option| meta.path.is_ident(option)) {
                if meta.input.peek(syn::Token![=]) {
                    let _: syn::Expr = meta.value()?.parse()?;
                }
                return Ok(());
            }
            Err(meta.error("DeserializeFields does not support this serde option"))
        })?;
    }
    Ok(())
}

/// Returns `true` if `ty` is spelled `Option<..>`, with or without a path prefix.
fn is_option(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Option")
}

/// Adds `T: CollectUnrecognizedKeys` to every generic type parameter.
fn add_trait_bounds(mut generics: Generics) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(type_param) = param {
            type_param.bounds.push(syn::parse_quote!(CollectUnrecognizedKeys));
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
        if serde_flag_is_set(&field.attrs, "skip") {
            continue;
        }
        let member = field.ident.as_ref().expect("named field has an ident");
        if serde_flag_is_set(&field.attrs, "flatten") {
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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use syn::parse::Parser as _;
    use syn::{DeriveInput, Field};

    use crate::{expand, expand_deserialize_fields, serde_field_name, serde_flag_is_set};

    fn parse_field(src: &str) -> Field {
        Field::parse_named.parse_str(src).expect("field parses")
    }

    #[rstest]
    #[case::rename_all_on_struct(
        r#"#[serde(rename_all = "kebab-case")] struct S { a: bool }"#,
        "does not support `#[serde(rename_all)]`"
    )]
    #[case::rename_all_on_enum(
        r#"#[serde(rename_all = "kebab-case")] enum E { A }"#,
        "does not support `#[serde(rename_all)]`"
    )]
    #[case::rename_all_beside_other_options(
        r#"#[serde(deny_unknown_fields, rename_all = "kebab-case")] struct S { a: bool }"#,
        "does not support `#[serde(rename_all)]`"
    )]
    #[case::rename_all_in_a_second_attribute(
        r#"#[serde(default)] #[serde(rename_all = "kebab-case")] struct S { a: bool }"#,
        "does not support `#[serde(rename_all)]`"
    )]
    #[case::union("union U { a: bool }", "cannot be derived for unions")]
    #[case::tuple_struct("struct S(bool);", "cannot be derived for tuple structs")]
    #[case::newtype_struct("struct S(Inner);", "cannot be derived for tuple structs")]
    fn expand_rejects(#[case] src: &str, #[case] expected: &str) {
        let input: DeriveInput = syn::parse_str(src).expect("input parses");
        let err = expand(&input).expect_err("input is rejected").to_string();
        assert!(err.contains(expected), "unexpected error: {err}");
    }

    #[rstest]
    #[case::unit_struct("struct S;")]
    #[case::empty_struct("struct S {}")]
    #[case::rename_all_fields_is_a_different_option(
        r#"#[serde(rename_all_fields = "kebab-case")] enum E { A { b: bool } }"#
    )]
    #[case::rename_all_on_a_variant(r#"enum E { #[serde(rename_all = "kebab-case")] A { b: bool } }"#)]
    #[case::non_serde_rename_all(r#"#[schemars(rename_all = "kebab-case")] struct S { a: bool }"#)]
    fn expand_accepts(#[case] src: &str) {
        let input: DeriveInput = syn::parse_str(src).expect("input parses");
        expand(&input).expect("input is accepted");
    }

    fn deserialize_fields_error(src: &str) -> String {
        let input: DeriveInput = syn::parse_str(src).expect("input parses");
        expand_deserialize_fields(&input)
            .expect_err("input is rejected")
            .to_string()
    }

    #[test]
    fn deserialize_fields_rejects_rename_all() {
        insta::assert_snapshot!(
            deserialize_fields_error(r#"#[serde(rename_all = "kebab-case")] struct S { a: Option<bool> }"#),
            @"DeserializeFields does not support `#[serde(rename_all)]`; rename individual fields instead"
        );
    }

    #[test]
    fn deserialize_fields_rejects_an_enum() {
        insta::assert_snapshot!(
            deserialize_fields_error("enum E { A }"),
            @"DeserializeFields can only be derived for structs"
        );
    }

    #[test]
    fn deserialize_fields_rejects_a_tuple_struct() {
        insta::assert_snapshot!(
            deserialize_fields_error("struct S(Option<bool>);"),
            @"DeserializeFields can only be derived for structs with named fields"
        );
    }

    #[test]
    fn deserialize_fields_rejects_a_non_option_field() {
        insta::assert_snapshot!(
            deserialize_fields_error("struct S { a: bool }"),
            @"DeserializeFields requires every field to be an `Option`"
        );
    }

    #[test]
    fn deserialize_fields_rejects_alias() {
        insta::assert_snapshot!(
            deserialize_fields_error(r#"struct S { #[serde(alias = "b")] a: Option<bool> }"#),
            @"DeserializeFields does not support this serde option"
        );
    }

    #[test]
    fn deserialize_fields_rejects_deserialize_with() {
        insta::assert_snapshot!(
            deserialize_fields_error(r#"struct S { #[serde(deserialize_with = "f")] a: Option<bool> }"#),
            @"DeserializeFields does not support this serde option"
        );
    }

    #[test]
    fn deserialize_fields_rejects_with() {
        insta::assert_snapshot!(
            deserialize_fields_error(r#"struct S { #[serde(with = "m")] a: Option<bool> }"#),
            @"DeserializeFields does not support this serde option"
        );
    }

    #[test]
    fn deserialize_fields_rejects_flatten() {
        insta::assert_snapshot!(
            deserialize_fields_error("struct S { #[serde(flatten)] a: Option<bool> }"),
            @"DeserializeFields does not support this serde option"
        );
    }

    #[test]
    fn deserialize_fields_rejects_skip() {
        insta::assert_snapshot!(
            deserialize_fields_error("struct S { #[serde(skip)] a: Option<bool> }"),
            @"DeserializeFields does not support this serde option"
        );
    }

    #[test]
    fn deserialize_fields_rejects_a_per_direction_rename() {
        insta::assert_snapshot!(
            deserialize_fields_error(r#"struct S { #[serde(rename(deserialize = "b"))] a: Option<bool> }"#),
            @"DeserializeFields does not support this serde option"
        );
    }

    #[test]
    fn deserialize_fields_accepts_an_empty_struct() {
        let input: DeriveInput = syn::parse_str("struct S {}").expect("input parses");
        expand_deserialize_fields(&input).expect("input is accepted");
    }

    #[test]
    fn deserialize_fields_accepts_rename() {
        let input: DeriveInput =
            syn::parse_str(r#"struct S { #[serde(rename = "b")] a: Option<bool> }"#).expect("input parses");
        expand_deserialize_fields(&input).expect("input is accepted");
    }

    #[test]
    fn deserialize_fields_accepts_serialize_only_options() {
        let input: DeriveInput = syn::parse_str(
            r#"struct S { #[serde(skip_serializing_if = "Option::is_none", serialize_with = "f")] a: Option<bool> }"#,
        )
        .expect("input parses");
        expand_deserialize_fields(&input).expect("input is accepted");
    }

    #[test]
    fn deserialize_fields_accepts_a_qualified_option() {
        let input: DeriveInput = syn::parse_str("struct S { a: std::option::Option<bool> }").expect("input parses");
        expand_deserialize_fields(&input).expect("input is accepted");
    }

    #[test]
    fn deserialize_fields_accepts_a_cfg_field() {
        let input: DeriveInput =
            syn::parse_str(r#"struct S { #[cfg(feature = "x")] a: Option<bool> }"#).expect("input parses");
        expand_deserialize_fields(&input).expect("input is accepted");
    }

    #[rstest]
    #[case::no_attributes("a: bool", "a")]
    #[case::rename(r#"#[serde(rename = "renamed")] a: bool"#, "renamed")]
    #[case::rename_after_a_valued_option(r#"#[serde(default = "d", rename = "renamed")] a: bool"#, "renamed")]
    #[case::rename_after_a_bare_flag(r#"#[serde(default, rename = "renamed")] a: bool"#, "renamed")]
    #[case::rename_in_a_second_attribute(r#"#[serde(default)] #[serde(rename = "renamed")] a: bool"#, "renamed")]
    #[case::rename_without_a_value("#[serde(rename)] a: bool", "a")]
    #[case::rename_with_a_non_string_value("#[serde(rename = 7)] a: bool", "a")]
    #[case::other_namespace(r#"#[schemars(rename = "renamed")] a: bool"#, "a")]
    #[case::unrelated_option(r#"#[serde(alias = "renamed")] a: bool"#, "a")]
    fn field_name(#[case] src: &str, #[case] expected: &str) {
        let field = parse_field(src);
        let ident = field.ident.clone().expect("field is named");
        assert_eq!(serde_field_name(&field, &ident), expected);
    }

    #[rstest]
    #[case::bare_flag("#[serde(flatten)] a: bool", "flatten", true)]
    #[case::flag_after_a_valued_option(r#"#[serde(default = "d", flatten)] a: bool"#, "flatten", true)]
    #[case::flag_before_a_valued_option(r#"#[serde(flatten, default = "d")] a: bool"#, "flatten", true)]
    #[case::flag_in_a_second_attribute("#[serde(default)] #[serde(skip)] a: bool", "skip", true)]
    #[case::absent("#[serde(default)] a: bool", "flatten", false)]
    #[case::prefix_of_another_option(r#"#[serde(skip_serializing_if = "f")] a: bool"#, "skip", false)]
    #[case::other_namespace("#[schemars(flatten)] a: bool", "flatten", false)]
    #[case::no_attributes("a: bool", "flatten", false)]
    fn flag_is_set(#[case] src: &str, #[case] name: &str, #[case] expected: bool) {
        assert_eq!(serde_flag_is_set(&parse_field(src).attrs, name), expected);
    }
}
