#![forbid(unsafe_code)]

use proc_macro::{Delimiter, TokenStream, TokenTree};

use forklaunch_common::to_pretty_camel_case;

#[proc_macro_derive(ForklaunchContract)]
pub fn derive_forklaunch_contract(input: TokenStream) -> TokenStream {
    let Some(type_name) = find_type_name(input) else {
        return compile_error(
            "ForklaunchContract can only be derived for structs, enums, or unions",
        );
    };

    parse_or_compile_error(&format!(
        "impl forklaunch_contract::ContractSchema for {type_name} {{
            fn schema_ref() -> forklaunch_contract::SchemaRef {{
                forklaunch_contract::SchemaRef::named(::core::stringify!({type_name}))
            }}
        }}"
    ))
}

#[proc_macro]
pub fn route_operation_name(input: TokenStream) -> TokenStream {
    let Some(name) = first_string_literal(input) else {
        return compile_error("route_operation_name! expects a single string literal");
    };

    parse_or_compile_error(&format!("{:?}", to_pretty_camel_case(&name)))
}

#[proc_macro]
pub fn assert_route_params(input: TokenStream) -> TokenStream {
    let tokens = input.into_iter().collect::<Vec<_>>();
    let Some(path) = tokens.iter().find_map(string_literal_from_token) else {
        return compile_error("assert_route_params! expects a route path string literal");
    };
    let Some(expected_group) = tokens.iter().find_map(|token| match token {
        TokenTree::Group(group) if group.delimiter() == Delimiter::Bracket => Some(group),
        _ => None,
    }) else {
        return compile_error("assert_route_params! expects expected params in brackets");
    };

    let actual = route_params(&path);
    let expected = expected_group
        .stream()
        .into_iter()
        .filter_map(|token| string_literal_from_token(&token))
        .collect::<Vec<_>>();

    if actual == expected {
        TokenStream::new()
    } else {
        compile_error(&format!(
            "route params do not match path: expected {actual:?}, found {expected:?}"
        ))
    }
}

fn find_type_name(input: TokenStream) -> Option<String> {
    let tokens = input.into_iter().collect::<Vec<_>>();
    tokens
        .windows(2)
        .find_map(|window| match (&window[0], &window[1]) {
            (TokenTree::Ident(keyword), TokenTree::Ident(name))
                if keyword.to_string() == "struct"
                    || keyword.to_string() == "enum"
                    || keyword.to_string() == "union" =>
            {
                Some(name.to_string())
            }
            _ => None,
        })
}

fn first_string_literal(input: TokenStream) -> Option<String> {
    input
        .into_iter()
        .find_map(|token| string_literal_from_token(&token))
}

fn string_literal_from_token(token: &TokenTree) -> Option<String> {
    let TokenTree::Literal(literal) = token else {
        return None;
    };

    unquote_string_literal(&literal.to_string())
}

fn unquote_string_literal(input: &str) -> Option<String> {
    let input = input.strip_prefix('"')?.strip_suffix('"')?;
    let mut output = String::new();
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }

        match chars.next()? {
            '"' => output.push('"'),
            '\\' => output.push('\\'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            escaped => output.push(escaped),
        }
    }

    Some(output)
}

fn route_params(path: &str) -> Vec<String> {
    path.split('/')
        .filter_map(|segment| {
            if let Some(name) = segment.strip_prefix(':') {
                return Some(name.to_owned());
            }

            segment
                .strip_prefix('{')
                .and_then(|name| name.strip_suffix('}'))
                .map(str::to_owned)
        })
        .collect()
}

fn parse_or_compile_error(input: &str) -> TokenStream {
    input
        .parse()
        .unwrap_or_else(|_| compile_error("failed to parse generated ForkLaunch macro output"))
}

fn compile_error(message: &str) -> TokenStream {
    format!("compile_error!({message:?});")
        .parse()
        .unwrap_or_else(|_| TokenStream::new())
}
