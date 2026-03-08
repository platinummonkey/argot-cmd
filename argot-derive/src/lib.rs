#![forbid(unsafe_code)]
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, LitChar, LitStr};

/// Derive macro that implements [`argot::ArgotCommand`] for a struct.
///
/// Annotate a struct with `#[derive(ArgotCommand)]` to automatically implement
/// `ArgotCommand::command()` using `#[argot(...)]` attributes on the struct
/// and its fields.
///
/// The generated `command()` implementation calls [`argot::Command::builder`]
/// and chains builder methods derived from the attributes, then calls
/// `.build().unwrap()`. The `unwrap` will **panic** at *call time* (not at
/// compile time) if the generated canonical name is somehow empty — this
/// should not occur in practice because the name defaults to the kebab-case
/// struct name.
///
/// ## Struct-level attributes (`#[argot(...)]`)
///
/// | Key | Type | Description |
/// |-----|------|-------------|
/// | `canonical = "name"` | string | Override the canonical command name. Default: struct name converted to kebab-case (e.g. `DeployApp` → `deploy-app`). |
/// | `summary = "text"` | string | One-line summary. |
/// | `description = "text"` | string | Long prose description. |
/// | `alias = "a"` | string | Add an alias (repeat the attribute to add more). |
/// | `best_practice = "text"` | string | Add a best-practice tip (repeatable). |
/// | `anti_pattern = "text"` | string | Add an anti-pattern warning (repeatable). |
///
/// ## Field-level attributes (`#[argot(...)]`)
///
/// Fields **without** an `#[argot(...)]` attribute are skipped entirely.
/// Every annotated field must include either `positional` or `flag`.
///
/// | Key | Description |
/// |-----|-------------|
/// | `positional` | Treat as a positional [`argot::Argument`]. |
/// | `flag` | Treat as a named [`argot::Flag`]. |
/// | `required` | Mark the argument or flag as required. |
/// | `short = 'c'` | Short character for a flag (e.g. `short = 'v'`). |
/// | `takes_value` | Flag consumes the next token as its value. |
/// | `description = "text"` | Human-readable description. |
/// | `default = "value"` | Default value string. |
///
/// ## Example
///
/// ```rust,ignore
/// use argot::ArgotCommand;
///
/// #[derive(ArgotCommand)]
/// #[argot(
///     summary = "Deploy the application",
///     alias = "d",
///     best_practice = "always dry-run first"
/// )]
/// struct Deploy {
///     #[argot(positional, required, description = "Target environment")]
///     env: String,
///
///     #[argot(flag, short = 'n', description = "Simulate without changes")]
///     dry_run: bool,
/// }
///
/// let cmd = Deploy::command();
/// assert_eq!(cmd.canonical, "deploy");
/// assert_eq!(cmd.aliases, vec!["d"]);
/// ```
#[proc_macro_derive(ArgotCommand, attributes(argot))]
pub fn derive_argot_command(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_impl(input)
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

// ---------------------------------------------------------------------------
// Attribute data structures
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StructAttrs {
    canonical: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    aliases: Vec<String>,
    best_practices: Vec<String>,
    anti_patterns: Vec<String>,
}

#[derive(Default)]
struct FieldAttrs {
    positional: bool,
    flag: bool,
    required: bool,
    short: Option<char>,
    takes_value: bool,
    description: Option<String>,
    default: Option<String>,
}

// ---------------------------------------------------------------------------
// Attribute parsers
// ---------------------------------------------------------------------------

fn parse_struct_attrs(attrs: &[syn::Attribute]) -> syn::Result<StructAttrs> {
    let mut out = StructAttrs::default();
    for attr in attrs {
        if !attr.path().is_ident("argot") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("canonical") {
                let val: LitStr = meta.value()?.parse()?;
                out.canonical = Some(val.value());
            } else if meta.path.is_ident("summary") {
                let val: LitStr = meta.value()?.parse()?;
                out.summary = Some(val.value());
            } else if meta.path.is_ident("description") {
                let val: LitStr = meta.value()?.parse()?;
                out.description = Some(val.value());
            } else if meta.path.is_ident("alias") {
                let val: LitStr = meta.value()?.parse()?;
                out.aliases.push(val.value());
            } else if meta.path.is_ident("best_practice") {
                let val: LitStr = meta.value()?.parse()?;
                out.best_practices.push(val.value());
            } else if meta.path.is_ident("anti_pattern") {
                let val: LitStr = meta.value()?.parse()?;
                out.anti_patterns.push(val.value());
            } else {
                return Err(meta.error(format!(
                    "unknown struct-level argot attribute `{}` — valid keys are: canonical, summary, description, alias, best_practice, anti_pattern",
                    meta.path
                        .get_ident()
                        .map(|i| i.to_string())
                        .unwrap_or_default()
                )));
            }
            Ok(())
        })?;
    }
    Ok(out)
}

fn parse_field_attrs(attrs: &[syn::Attribute]) -> syn::Result<Option<FieldAttrs>> {
    let mut found = false;
    let mut out = FieldAttrs::default();
    for attr in attrs {
        if !attr.path().is_ident("argot") {
            continue;
        }
        found = true;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("positional") {
                out.positional = true;
            } else if meta.path.is_ident("flag") {
                out.flag = true;
            } else if meta.path.is_ident("required") {
                out.required = true;
            } else if meta.path.is_ident("takes_value") {
                out.takes_value = true;
            } else if meta.path.is_ident("short") {
                let val: LitChar = meta.value()?.parse()?;
                out.short = Some(val.value());
            } else if meta.path.is_ident("description") {
                let val: LitStr = meta.value()?.parse()?;
                out.description = Some(val.value());
            } else if meta.path.is_ident("default") {
                let val: LitStr = meta.value()?.parse()?;
                out.default = Some(val.value());
            } else {
                return Err(meta.error(format!(
                    "unknown field-level argot attribute `{}` — valid keys are: positional, flag, required, short, takes_value, description, default",
                    meta.path
                        .get_ident()
                        .map(|i| i.to_string())
                        .unwrap_or_default()
                )));
            }
            Ok(())
        })?;
    }
    if found {
        Ok(Some(out))
    } else {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Name conversion helpers
// ---------------------------------------------------------------------------

/// Convert `CamelCase` → `kebab-case`.
///
/// Inserts `-` before each uppercase letter that follows a lowercase letter,
/// then lowercases everything.
fn camel_to_kebab(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && i > 0 && chars[i - 1].is_lowercase() {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Convert a Rust field name (`snake_case`) to a CLI name (`kebab-case`).
fn snake_to_kebab(name: &str) -> String {
    name.replace('_', "-")
}

// ---------------------------------------------------------------------------
// Core derive implementation
// ---------------------------------------------------------------------------

fn derive_impl(input: DeriveInput) -> syn::Result<TokenStream2> {
    let fields = match &input.data {
        Data::Struct(s) => &s.fields,
        Data::Enum(_) => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                format!(
                    "`#[derive(ArgotCommand)]` cannot be used on enum `{}` — only structs are supported",
                    input.ident
                ),
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                format!(
                    "`#[derive(ArgotCommand)]` cannot be used on union `{}` — only structs are supported",
                    input.ident
                ),
            ));
        }
    };

    let named = match fields {
        Fields::Named(n) => &n.named,
        Fields::Unit => &syn::punctuated::Punctuated::new(),
        Fields::Unnamed(_) => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                format!(
                    "`{}` uses tuple fields — `#[derive(ArgotCommand)]` requires named fields (e.g., `struct Foo {{ name: String }}`)",
                    input.ident
                ),
            ));
        }
    };

    let struct_attrs = parse_struct_attrs(&input.attrs)?;

    let canonical = struct_attrs
        .canonical
        .clone()
        .unwrap_or_else(|| camel_to_kebab(&input.ident.to_string()));

    let mut builder_tokens = quote! {
        ::argot::Command::builder(#canonical)
    };

    if let Some(ref s) = struct_attrs.summary {
        builder_tokens = quote! { #builder_tokens .summary(#s) };
    }
    if let Some(ref d) = struct_attrs.description {
        builder_tokens = quote! { #builder_tokens .description(#d) };
    }
    for alias in &struct_attrs.aliases {
        builder_tokens = quote! { #builder_tokens .alias(#alias) };
    }
    for bp in &struct_attrs.best_practices {
        builder_tokens = quote! { #builder_tokens .best_practice(#bp) };
    }
    for ap in &struct_attrs.anti_patterns {
        builder_tokens = quote! { #builder_tokens .anti_pattern(#ap) };
    }

    for field in named.iter() {
        let field_ident = field.ident.as_ref().expect("named field has ident");
        let fa = match parse_field_attrs(&field.attrs)? {
            None => continue,
            Some(fa) => fa,
        };

        if fa.positional && fa.flag {
            return Err(syn::Error::new_spanned(
                field_ident,
                "a field cannot be both `positional` and `flag` — choose one",
            ));
        }

        if fa.positional {
            let arg_name = snake_to_kebab(&field_ident.to_string());
            let mut arg_builder = quote! { ::argot::Argument::builder(#arg_name) };
            if fa.required {
                arg_builder = quote! { #arg_builder .required() };
            }
            if let Some(ref desc) = fa.description {
                arg_builder = quote! { #arg_builder .description(#desc) };
            }
            if let Some(ref def) = fa.default {
                arg_builder = quote! { #arg_builder .default_value(#def) };
            }
            builder_tokens = quote! { #builder_tokens .argument(#arg_builder .build().unwrap()) };
        } else if fa.flag {
            let flag_name = snake_to_kebab(&field_ident.to_string());
            let mut flag_builder = quote! { ::argot::Flag::builder(#flag_name) };
            if let Some(c) = fa.short {
                flag_builder = quote! { #flag_builder .short(#c) };
            }
            if fa.required {
                flag_builder = quote! { #flag_builder .required() };
            }
            if fa.takes_value {
                flag_builder = quote! { #flag_builder .takes_value() };
            }
            if let Some(ref desc) = fa.description {
                flag_builder = quote! { #flag_builder .description(#desc) };
            }
            if let Some(ref def) = fa.default {
                flag_builder = quote! { #flag_builder .default_value(#def) };
            }
            builder_tokens = quote! { #builder_tokens .flag(#flag_builder .build().unwrap()) };
        } else {
            return Err(syn::Error::new_spanned(
                field_ident,
                format!(
                    "field `{}` has `#[argot(...)]` but is missing a kind — add `positional` or `flag`",
                    field_ident
                ),
            ));
        }
    }

    builder_tokens = quote! { #builder_tokens .build().unwrap() };

    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::argot::ArgotCommand for #ident #ty_generics #where_clause {
            fn command() -> ::argot::Command {
                #builder_tokens
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Unit tests for name conversion helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_to_kebab() {
        assert_eq!(camel_to_kebab("Deploy"), "deploy");
        assert_eq!(camel_to_kebab("DeployCommand"), "deploy-command");
        assert_eq!(camel_to_kebab("RemoteAdd"), "remote-add");
        assert_eq!(camel_to_kebab("SomeOtherCommand"), "some-other-command");
    }

    #[test]
    fn test_snake_to_kebab() {
        assert_eq!(snake_to_kebab("dry_run"), "dry-run");
        assert_eq!(snake_to_kebab("output"), "output");
        assert_eq!(snake_to_kebab("env"), "env");
    }

    #[test]
    fn test_camel_to_kebab_single_word() {
        assert_eq!(camel_to_kebab("Deploy"), "deploy");
    }

    #[test]
    fn test_snake_to_kebab_multi_word() {
        assert_eq!(snake_to_kebab("dry_run_mode"), "dry-run-mode");
    }
}
