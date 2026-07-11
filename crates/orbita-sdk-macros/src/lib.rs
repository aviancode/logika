//! Derive macros for `orbita-sdk`.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Attribute, Data, DataEnum, DataStruct, DeriveInput, Error, Fields, GenericArgument, Generics,
    LitInt, LitStr, PathArguments, Type, TypePath, parse_macro_input, parse_quote,
};

/// Derives `orbita_sdk::Schema` from a Rust struct or enum.
#[proc_macro_derive(Schema, attributes(schema))]
pub fn derive_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_schema(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand_schema(input: DeriveInput) -> syn::Result<TokenStream2> {
    let SchemaAttributes { name, version } = schema_attributes(&input.attrs)?;
    let name = name.ok_or_else(|| {
        Error::new_spanned(
            &input.ident,
            "missing #[schema(name = \"namespace.type\", version = 1)] attribute",
        )
    })?;
    let version = version.ok_or_else(|| {
        Error::new_spanned(
            &input.ident,
            "missing `version` in #[schema(name = ..., version = ...)]",
        )
    })?;
    let version_value = version.base10_parse::<u32>()?;
    if version_value == 0 {
        return Err(Error::new_spanned(
            version,
            "schema version must be greater than zero",
        ));
    }

    let definition = match &input.data {
        Data::Struct(data) => struct_definition(data)?,
        Data::Enum(data) => enum_definition(data)?,
        Data::Union(data) => {
            return Err(Error::new_spanned(
                data.union_token,
                "Schema cannot be derived for unions",
            ));
        }
    };
    let ident = &input.ident;
    let generics = schema_generics(input.generics);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::orbita_sdk::Schema for #ident #type_generics #where_clause {
            const NAME: &'static str = #name;
            const VERSION: u32 = #version_value;

            fn definition() -> ::orbita_sdk::_private::SchemaDefinition {
                #definition
            }
        }
    })
}

fn schema_generics(mut generics: Generics) -> Generics {
    for parameter in generics.type_params_mut() {
        parameter.bounds.push(parse_quote!(::orbita_sdk::Schema));
    }
    generics
}

#[derive(Default)]
struct SchemaAttributes {
    name: Option<LitStr>,
    version: Option<LitInt>,
}

fn schema_attributes(attributes: &[Attribute]) -> syn::Result<SchemaAttributes> {
    let mut parsed = SchemaAttributes::default();
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("schema"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                if parsed.name.is_some() {
                    return Err(meta.error("duplicate schema name"));
                }
                parsed.name = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("version") {
                if parsed.version.is_some() {
                    return Err(meta.error("duplicate schema version"));
                }
                parsed.version = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("unsupported schema attribute"))
            }
        })?;
    }
    Ok(parsed)
}

fn renamed(attributes: &[Attribute], default: String) -> syn::Result<String> {
    let mut name = None;
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("schema"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                if name.is_some() {
                    return Err(meta.error("duplicate schema rename"));
                }
                name = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else {
                Err(meta.error("only `rename` is supported here"))
            }
        })?;
    }
    Ok(name.unwrap_or(default))
}

fn struct_definition(data: &DataStruct) -> syn::Result<TokenStream2> {
    fields_definition(&data.fields)
}

fn fields_definition(fields: &Fields) -> syn::Result<TokenStream2> {
    let fields = schema_fields(fields)?;
    Ok(quote! {
        ::orbita_sdk::_private::SchemaDefinition::Struct(
            ::orbita_sdk::_private::StructSchema::new([#(#fields),*])
        )
    })
}

fn schema_fields(fields: &Fields) -> syn::Result<Vec<TokenStream2>> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let default = field
                .ident
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| index.to_string());
            let name = renamed(&field.attrs, default)?;
            if let Some(inner) = option_inner(&field.ty) {
                let definition = type_definition(inner)?;
                Ok(quote! {
                    ::orbita_sdk::_private::SchemaField::optional(#name, #definition)
                })
            } else {
                let definition = type_definition(&field.ty)?;
                Ok(quote! {
                    ::orbita_sdk::_private::SchemaField::required(#name, #definition)
                })
            }
        })
        .collect()
}

fn enum_definition(data: &DataEnum) -> syn::Result<TokenStream2> {
    if data
        .variants
        .iter()
        .all(|variant| matches!(variant.fields, Fields::Unit))
    {
        let variants = data
            .variants
            .iter()
            .map(|variant| renamed(&variant.attrs, variant.ident.to_string()))
            .collect::<syn::Result<Vec<_>>>()?;
        return Ok(quote! {
            ::orbita_sdk::_private::SchemaDefinition::Enum(
                ::orbita_sdk::_private::EnumSchema::new([#(#variants),*])
            )
        });
    }

    let variants = data
        .variants
        .iter()
        .map(|variant| {
            let tag = renamed(&variant.attrs, variant.ident.to_string())?;
            let definition = match &variant.fields {
                Fields::Unit => fields_definition(&Fields::Unit)?,
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    type_definition(&fields.unnamed[0].ty)?
                }
                fields => fields_definition(fields)?,
            };
            Ok(quote! {
                ::orbita_sdk::_private::TaggedVariant::new(#tag, #definition)
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        ::orbita_sdk::_private::SchemaDefinition::TaggedUnion(
            ::orbita_sdk::_private::TaggedUnionSchema::new([#(#variants),*])
        )
    })
}

fn option_inner(value: &Type) -> Option<&Type> {
    type_arguments(value, "Option")
        .ok()
        .and_then(|arguments| arguments.first().copied())
}

fn type_definition(value: &Type) -> syn::Result<TokenStream2> {
    match value {
        Type::Array(array) => {
            let item = type_definition(&array.elem)?;
            Ok(quote!(::orbita_sdk::_private::SchemaDefinition::array(#item)))
        }
        Type::Tuple(tuple) => {
            let fields = tuple
                .elems
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let name = index.to_string();
                    let definition = type_definition(value)?;
                    Ok(quote! {
                        ::orbita_sdk::_private::SchemaField::required(#name, #definition)
                    })
                })
                .collect::<syn::Result<Vec<_>>>()?;
            Ok(quote! {
                ::orbita_sdk::_private::SchemaDefinition::Struct(
                    ::orbita_sdk::_private::StructSchema::new([#(#fields),*])
                )
            })
        }
        Type::Path(path) => path_definition(path),
        _ => Err(Error::new_spanned(
            value,
            "unsupported field type; use a primitive, container, or another Schema type",
        )),
    }
}

fn path_definition(path: &TypePath) -> syn::Result<TokenStream2> {
    let Some(segment) = path.path.segments.last() else {
        return Err(Error::new_spanned(path, "empty type path"));
    };
    let ident = segment.ident.to_string();
    let primitive = match ident.as_str() {
        "bool" => Some(quote!(::orbita_sdk::_private::PrimitiveType::Bool)),
        "i8" | "i16" | "i32" | "i64" | "isize" => {
            Some(quote!(::orbita_sdk::_private::PrimitiveType::I64))
        }
        "u8" | "u16" | "u32" | "u64" | "usize" => {
            Some(quote!(::orbita_sdk::_private::PrimitiveType::U64))
        }
        "f32" | "f64" => Some(quote!(::orbita_sdk::_private::PrimitiveType::F64)),
        "String" | "str" => Some(quote!(::orbita_sdk::_private::PrimitiveType::String)),
        _ => None,
    };
    if let Some(primitive) = primitive {
        return Ok(quote!(::orbita_sdk::_private::SchemaDefinition::primitive(#primitive)));
    }

    match ident.as_str() {
        "Vec" => {
            let arguments = type_arguments_from_segment(segment, 1)?;
            let item = type_definition(arguments[0])?;
            Ok(quote!(::orbita_sdk::_private::SchemaDefinition::array(#item)))
        }
        "Option" => {
            let arguments = type_arguments_from_segment(segment, 1)?;
            let item = type_definition(arguments[0])?;
            Ok(quote!(::orbita_sdk::_private::SchemaDefinition::optional(#item)))
        }
        "Box" => {
            let arguments = type_arguments_from_segment(segment, 1)?;
            type_definition(arguments[0])
        }
        "BTreeMap" | "HashMap" => {
            let arguments = type_arguments_from_segment(segment, 2)?;
            if !is_string_type(arguments[0]) {
                return Err(Error::new_spanned(
                    arguments[0],
                    "canonical schema maps require String keys",
                ));
            }
            let value = type_definition(arguments[1])?;
            Ok(quote!(::orbita_sdk::_private::SchemaDefinition::map(#value)))
        }
        _ => Ok(quote!(<#path as ::orbita_sdk::Schema>::definition())),
    }
}

fn type_arguments<'a>(value: &'a Type, expected: &str) -> syn::Result<Vec<&'a Type>> {
    let Type::Path(path) = value else {
        return Err(Error::new_spanned(value, format!("expected {expected}")));
    };
    let Some(segment) = path.path.segments.last() else {
        return Err(Error::new_spanned(value, format!("expected {expected}")));
    };
    if segment.ident != expected {
        return Err(Error::new_spanned(value, format!("expected {expected}")));
    }
    type_arguments_from_segment(segment, 1)
}

fn type_arguments_from_segment(
    segment: &syn::PathSegment,
    expected: usize,
) -> syn::Result<Vec<&Type>> {
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(Error::new_spanned(
            segment,
            format!("{} requires {expected} type argument(s)", segment.ident),
        ));
    };
    let types = arguments
        .args
        .iter()
        .filter_map(|argument| match argument {
            GenericArgument::Type(value) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    if types.len() != expected || arguments.args.len() != expected {
        return Err(Error::new_spanned(
            arguments,
            format!("{} requires {expected} type argument(s)", segment.ident),
        ));
    }
    Ok(types)
}

fn is_string_type(value: &Type) -> bool {
    matches!(
        value,
        Type::Path(path)
            if path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "String")
    )
}
