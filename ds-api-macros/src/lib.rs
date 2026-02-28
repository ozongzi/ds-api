use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Expr, FnArg, ImplItem, ItemImpl, Lit, Meta, Pat, Type, parse_macro_input};

fn extract_doc(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("doc") {
                return None;
            }
            if let Meta::NameValue(nv) = &attr.meta
                && let Expr::Lit(el) = &nv.value
                && let Lit::Str(s) = &el.lit
            {
                return Some(s.value().trim().to_string());
            }
            None
        })
        .collect()
}

fn parse_doc(lines: &[String]) -> (String, std::collections::HashMap<String, String>) {
    let mut desc_lines = vec![];
    let mut params = std::collections::HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim().to_string();
            let val = val.trim().to_string();
            if key.chars().all(|c| c.is_alphanumeric() || c == '_') && !val.is_empty() {
                params.insert(key, val);
                continue;
            }
        }
        if params.is_empty() {
            desc_lines.push(line.clone());
        }
    }
    (desc_lines.join(" ").trim().to_string(), params)
}

fn type_to_json_schema(ty: &Type) -> TokenStream2 {
    let type_str = quote!(#ty).to_string().replace(" ", "");
    match type_str.as_str() {
        "String" | "&str" => quote!(serde_json::json!({"type": "string"})),
        "bool" => quote!(serde_json::json!({"type": "boolean"})),
        "f32" | "f64" => quote!(serde_json::json!({"type": "number"})),
        s if s.starts_with("Option<") => {
            let inner = &type_str[7..type_str.len() - 1];
            match inner {
                "String" | "&str" => quote!(serde_json::json!({"type": "string"})),
                "bool" => quote!(serde_json::json!({"type": "boolean"})),
                "f32" | "f64" => quote!(serde_json::json!({"type": "number"})),
                _ => quote!(serde_json::json!({"type": "integer"})),
            }
        }
        _ => quote!(serde_json::json!({"type": "integer"})),
    }
}

fn is_option(ty: &Type) -> bool {
    quote!(#ty)
        .to_string()
        .replace(" ", "")
        .starts_with("Option<")
}

struct ToolMethod {
    tool_name: String,
    description: String,
    params: Vec<ParamInfo>,
    body: syn::Block,
}

struct ParamInfo {
    name: String,
    ty: Type,
    desc: String,
    optional: bool,
}

#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_impl = parse_macro_input!(item as ItemImpl);

    let override_name: Option<String> = if !attr.is_empty() {
        let s = TokenStream2::from(attr).to_string();
        s.find('"').and_then(|start| {
            s.rfind('"')
                .filter(|&end| end > start)
                .map(|end| s[start + 1..end].to_string())
        })
    } else {
        None
    };

    let mut tool_methods: Vec<ToolMethod> = vec![];

    for item in &item_impl.items {
        if let ImplItem::Fn(method) = item {
            if method.sig.asyncness.is_none() {
                continue;
            }
            let fn_name = method.sig.ident.to_string();
            let tool_name = override_name.clone().unwrap_or_else(|| fn_name.clone());
            let doc_lines = extract_doc(&method.attrs);
            let (description, param_docs) = parse_doc(&doc_lines);

            let mut params = vec![];
            for arg in &method.sig.inputs {
                if let FnArg::Typed(pt) = arg {
                    let name = if let Pat::Ident(pi) = &*pt.pat {
                        pi.ident.to_string()
                    } else {
                        continue;
                    };
                    let ty = (*pt.ty).clone();
                    let desc = param_docs.get(&name).cloned().unwrap_or_default();
                    let optional = is_option(&ty);
                    params.push(ParamInfo {
                        name,
                        ty,
                        desc,
                        optional,
                    });
                }
            }
            tool_methods.push(ToolMethod {
                tool_name,
                description,
                params,
                body: method.block.clone(),
            });
        }
    }

    let raw_tools_body = tool_methods.iter().map(|m| {
        let tool_name = &m.tool_name;
        let description = &m.description;
        let prop_inserts = m.params.iter().map(|p| {
            let pname = &p.name;
            let pdesc = &p.desc;
            let schema = type_to_json_schema(&p.ty);
            quote! {{
                let mut prop = #schema;
                prop["description"] = serde_json::json!(#pdesc);
                properties.insert(#pname.to_string(), prop);
            }}
        });
        let required: Vec<&str> = m
            .params
            .iter()
            .filter(|p| !p.optional)
            .map(|p| p.name.as_str())
            .collect();
        quote! {{
            let mut properties = serde_json::Map::new();
            #(#prop_inserts)*
            let required: Vec<&str> = vec![#(#required),*];
            ds_api::raw::request::tool::Tool {
                r#type: ds_api::raw::request::message::ToolType::Function,
                function: ds_api::raw::request::tool::Function {
                    name: #tool_name.to_string(),
                    description: Some(#description.to_string()),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": properties,
                        "required": required,
                    }),
                    strict: None,
                },
            }
        }}
    });

    let call_arms = tool_methods.iter().map(|m| {
        let tool_name = &m.tool_name;
        let body = &m.body;
        let arg_parses = m.params.iter().map(|p| {
            let pname = syn::Ident::new(&p.name, Span::call_site());
            let pname_str = &p.name;
            let ty = &p.ty;
            quote! {
                let #pname: #ty = serde_json::from_value(
                    args.get(#pname_str).cloned().unwrap_or(serde_json::Value::Null)
                ).expect(concat!("failed to parse param: ", #pname_str));
            }
        });
        quote! {
            #tool_name => {
                #(#arg_parses)*
                { #body }
            }
        }
    });

    let self_ty = &item_impl.self_ty;

    let expanded = quote! {
        #[async_trait::async_trait]
        impl ds_api::tool_trait::Tool for #self_ty {
            fn raw_tools(&self) -> Vec<ds_api::raw::request::tool::Tool> {
                vec![#(#raw_tools_body),*]
            }

            async fn call(&self, name: &str, args: serde_json::Value) -> serde_json::Value {
                match name {
                    #(#call_arms)*
                    _ => serde_json::json!({"error": format!("unknown tool: {}", name)}),
                }
            }
        }
    };

    expanded.into()
}
