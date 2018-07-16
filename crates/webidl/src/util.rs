use std::iter::{self, FromIterator};

use backend;
use backend::util::{ident_ty, leading_colon_path_ty, raw_ident, rust_ident, simple_path_ty};
use heck::SnakeCase;
use proc_macro2::Ident;
use syn;
use webidl;
use webidl::ast::ExtendedAttribute;

fn shared_ref(ty: syn::Type) -> syn::Type {
    syn::TypeReference {
        and_token: Default::default(),
        lifetime: None,
        mutability: None,
        elem: Box::new(ty),
    }.into()
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TypePosition {
    Argument,
    Return,
}

pub fn webidl_ty_to_syn_ty(ty: &webidl::ast::Type, pos: TypePosition) -> Option<syn::Type> {
    // nullable types are not yet supported (see issue #14)
    if ty.nullable {
        return Some(match ty.kind {
            // `DOMString -> `&str` for arguments
            webidl::ast::TypeKind::DOMString if pos == TypePosition::Argument => {
                option_ty(shared_ref(ident_ty(raw_ident("str"))))
            }
            webidl::ast::TypeKind::DOMString => {
                option_ty(simple_path_ty(vec![rust_ident("wasm_bindgen"), rust_ident("JsValue")]))
            },

            // Support for these types is not yet implemented, so skip
            // generating any bindings for this function.
            _=> {
                return None;
            }
        });
    }
    Some(match ty.kind {
        // `any` becomes `::wasm_bindgen::JsValue`.
        webidl::ast::TypeKind::Any => {
            simple_path_ty(vec![rust_ident("wasm_bindgen"), rust_ident("JsValue")])
        }

        // A reference to a type by name becomes the same thing in the
        // bindings.
        webidl::ast::TypeKind::Identifier(ref id) => ident_ty(rust_ident(id)),

        // Scalars.
        webidl::ast::TypeKind::Boolean => ident_ty(raw_ident("bool")),
        webidl::ast::TypeKind::Byte => ident_ty(raw_ident("i8")),
        webidl::ast::TypeKind::Octet => ident_ty(raw_ident("u8")),
        webidl::ast::TypeKind::RestrictedDouble | webidl::ast::TypeKind::UnrestrictedDouble => {
            ident_ty(raw_ident("f64"))
        }
        webidl::ast::TypeKind::RestrictedFloat | webidl::ast::TypeKind::UnrestrictedFloat => {
            ident_ty(raw_ident("f32"))
        }
        webidl::ast::TypeKind::SignedLong => ident_ty(raw_ident("i32")),
        webidl::ast::TypeKind::SignedLongLong => ident_ty(raw_ident("i64")),
        webidl::ast::TypeKind::SignedShort => ident_ty(raw_ident("i16")),
        webidl::ast::TypeKind::UnsignedLong => ident_ty(raw_ident("u32")),
        webidl::ast::TypeKind::UnsignedLongLong => ident_ty(raw_ident("u64")),
        webidl::ast::TypeKind::UnsignedShort => ident_ty(raw_ident("u16")),

        // `DOMString -> `&str` for arguments
        webidl::ast::TypeKind::DOMString if pos == TypePosition::Argument => {
            shared_ref(ident_ty(raw_ident("str")))
        }
        webidl::ast::TypeKind::DOMString => {
            simple_path_ty(vec![rust_ident("wasm_bindgen"), rust_ident("JsValue")])
        },

        // Support for these types is not yet implemented, so skip
        // generating any bindings for this function.
        webidl::ast::TypeKind::ArrayBuffer
        | webidl::ast::TypeKind::ByteString
        | webidl::ast::TypeKind::DataView
        | webidl::ast::TypeKind::Error
        | webidl::ast::TypeKind::Float32Array
        | webidl::ast::TypeKind::Float64Array
        | webidl::ast::TypeKind::FrozenArray(_)
        | webidl::ast::TypeKind::Int16Array
        | webidl::ast::TypeKind::Int32Array
        | webidl::ast::TypeKind::Int8Array
        | webidl::ast::TypeKind::Object
        | webidl::ast::TypeKind::Promise(_)
        | webidl::ast::TypeKind::Record(..)
        | webidl::ast::TypeKind::Sequence(_)
        | webidl::ast::TypeKind::Symbol
        | webidl::ast::TypeKind::USVString
        | webidl::ast::TypeKind::Uint16Array
        | webidl::ast::TypeKind::Uint32Array
        | webidl::ast::TypeKind::Uint8Array
        | webidl::ast::TypeKind::Uint8ClampedArray
        | webidl::ast::TypeKind::Union(_) => {
            return None;
        }
    })
}

fn simple_fn_arg(ident: Ident, ty: syn::Type) -> syn::ArgCaptured {
    syn::ArgCaptured {
        pat: syn::Pat::Ident(syn::PatIdent {
            by_ref: None,
            mutability: None,
            ident,
            subpat: None,
        }),
        colon_token: Default::default(),
        ty,
    }
}

fn webidl_arguments_to_syn_arg_captured<'a, I>(
    arguments: I,
    kind: &backend::ast::ImportFunctionKind,
) -> Option<Vec<syn::ArgCaptured>>
where
    I: Iterator<Item = (&'a str, &'a webidl::ast::Type, bool)>,
{
    let estimate = arguments.size_hint();
    let len = estimate.1.unwrap_or(estimate.0);
    let mut res = if let backend::ast::ImportFunctionKind::Method {
        ty,
        kind:
            backend::ast::MethodKind::Operation(backend::ast::Operation {
                is_static: false, ..
            }),
        ..
    } = kind
    {
        let mut res = Vec::with_capacity(len + 1);
        res.push(simple_fn_arg(raw_ident("self_"), shared_ref(ty.clone())));
        res
    } else {
        Vec::with_capacity(len)
    };

    for (name, ty, variadic) in arguments {
        if variadic {
            warn!("Variadic arguments are not supported yet",);
            return None;
        }

        match webidl_ty_to_syn_ty(ty, TypePosition::Argument) {
            None => {
                warn!("Argument's type is not yet supported: {:?}", ty);
                return None;
            }
            Some(ty) => res.push(simple_fn_arg(rust_ident(&name.to_snake_case()), ty)),
        }
    }

    Some(res)
}

fn unit_ty() -> syn::Type {
    syn::Type::Tuple(syn::TypeTuple {
        paren_token: Default::default(),
        elems: syn::punctuated::Punctuated::new(),
    })
}

fn result_ty(t: syn::Type) -> syn::Type {
    let js_value = leading_colon_path_ty(vec![rust_ident("wasm_bindgen"), rust_ident("JsValue")]);

    let arguments = syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
        colon2_token: None,
        lt_token: Default::default(),
        args: FromIterator::from_iter(vec![
            syn::GenericArgument::Type(t),
            syn::GenericArgument::Type(js_value),
        ]),
        gt_token: Default::default(),
    });

    let ident = raw_ident("Result");
    let seg = syn::PathSegment { ident, arguments };
    let path: syn::Path = seg.into();
    let ty = syn::TypePath { qself: None, path };
    ty.into()
}

fn option_ty(t: syn::Type) -> syn::Type {
    let arguments = syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
        colon2_token: None,
        lt_token: Default::default(),
        args: FromIterator::from_iter(vec![
            syn::GenericArgument::Type(t),
        ]),
        gt_token: Default::default(),
    });

    let ident = raw_ident("Option");
    let seg = syn::PathSegment { ident, arguments };
    let path: syn::Path = seg.into();
    let ty = syn::TypePath { qself: None, path };
    ty.into()
}

pub fn create_function<'a, I>(
    name: &str,
    arguments: I,
    mut ret: Option<syn::Type>,
    kind: backend::ast::ImportFunctionKind,
    structural: bool,
    catch: bool,
) -> Option<backend::ast::ImportFunction>
where
    I: Iterator<Item = (&'a str, &'a webidl::ast::Type, bool)>,
{
    let rust_name = rust_ident(&name.to_snake_case());
    let name = raw_ident(name);

    let arguments = webidl_arguments_to_syn_arg_captured(arguments, &kind)?;

    let js_ret = ret.clone();

    if catch {
        ret = Some(ret.map_or_else(|| result_ty(unit_ty()), |ret| result_ty(ret)))
    }

    let shim = {
        let ns = match kind {
            backend::ast::ImportFunctionKind::Normal => "",
            backend::ast::ImportFunctionKind::Method { ref class, .. } => class,
        };

        raw_ident(&format!("__widl_f_{}_{}", rust_name, ns))
    };

    Some(backend::ast::ImportFunction {
        function: backend::ast::Function {
            name,
            arguments,
            ret,
            rust_attrs: vec![],
            rust_vis: syn::Visibility::Public(syn::VisPublic {
                pub_token: Default::default(),
            }),
        },
        rust_name,
        js_ret,
        catch,
        structural,
        kind,
        shim,
    })
}

pub fn create_basic_method(
    arguments: &[webidl::ast::Argument],
    name: Option<&String>,
    return_type: &webidl::ast::ReturnType,
    self_name: &str,
    is_static: bool,
    catch: bool,
) -> Option<backend::ast::ImportFunction> {
    let name = match name {
        None => {
            warn!("Operations without a name are unsupported");
            return None;
        }
        Some(ref name) => name,
    };

    let kind = backend::ast::ImportFunctionKind::Method {
        class: self_name.to_string(),
        ty: ident_ty(rust_ident(self_name)),
        kind: backend::ast::MethodKind::Operation(backend::ast::Operation {
            is_static,
            kind: backend::ast::OperationKind::Regular,
        }),
    };

    let ret = match return_type {
        webidl::ast::ReturnType::Void => None,
        webidl::ast::ReturnType::NonVoid(ty) => match webidl_ty_to_syn_ty(ty, TypePosition::Return)
        {
            None => {
                warn!("Operation's return type is not yet supported: {:?}", ty);
                return None;
            }
            Some(ty) => Some(ty),
        },
    };

    create_function(
        &name,
        arguments
            .iter()
            .map(|arg| (&*arg.name, &*arg.type_, arg.variadic)),
        ret,
        kind,
        false,
        catch,
    )
}

pub fn create_getter(
    name: &str,
    ty: &webidl::ast::Type,
    self_name: &str,
    is_static: bool,
    is_structural: bool,
    catch: bool,
) -> Option<backend::ast::ImportFunction> {
    let ret = match webidl_ty_to_syn_ty(ty, TypePosition::Return) {
        None => {
            warn!("Attribute's type does not yet support reading: {:?}", ty);
            return None;
        }
        Some(ty) => Some(ty),
    };

    let kind = backend::ast::ImportFunctionKind::Method {
        class: self_name.to_string(),
        ty: ident_ty(rust_ident(self_name)),
        kind: backend::ast::MethodKind::Operation(backend::ast::Operation {
            is_static,
            kind: backend::ast::OperationKind::Getter(Some(raw_ident(name))),
        }),
    };

    create_function(name, iter::empty(), ret, kind, is_structural, catch)
}

pub fn create_setter(
    name: &str,
    ty: &webidl::ast::Type,
    self_name: &str,
    is_static: bool,
    is_structural: bool,
    catch: bool,
) -> Option<backend::ast::ImportFunction> {
    let kind = backend::ast::ImportFunctionKind::Method {
        class: self_name.to_string(),
        ty: ident_ty(rust_ident(self_name)),
        kind: backend::ast::MethodKind::Operation(backend::ast::Operation {
            is_static,
            kind: backend::ast::OperationKind::Setter(Some(raw_ident(name))),
        }),
    };

    create_function(
        &format!("set_{}", name),
        iter::once((name, ty, false)),
        None,
        kind,
        is_structural,
        catch,
    )
}

/// ChromeOnly is for things that are only exposed to priveleged code in Firefox.
pub fn is_chrome_only(ext_attrs: &[Box<ExtendedAttribute>]) -> bool {
    ext_attrs.iter().any(|external_attribute| {
        return match &**external_attribute {
            ExtendedAttribute::ArgumentList(al) => al.name == "ChromeOnly",
            ExtendedAttribute::Identifier(i) => i.lhs == "ChromeOnly",
            ExtendedAttribute::IdentifierList(il) => il.lhs == "ChromeOnly",
            ExtendedAttribute::NamedArgumentList(nal) => nal.lhs_name == "ChromeOnly",
            ExtendedAttribute::NoArguments(webidl::ast::Other::Identifier(name)) => {
                name == "ChromeOnly"
            }
            ExtendedAttribute::NoArguments(_na) => false,
        };
    })
}

pub fn is_structural(attrs: &[Box<ExtendedAttribute>]) -> bool {
    attrs.iter().any(|attr| {
        if let ExtendedAttribute::NoArguments(webidl::ast::Other::Identifier(ref name)) = **attr {
            name == "Unforgeable"
        } else {
            false
        }
    })
}

pub fn throws(attrs: &[Box<ExtendedAttribute>]) -> bool {
    attrs.iter().any(|attr| {
        if let ExtendedAttribute::NoArguments(webidl::ast::Other::Identifier(ref name)) = **attr {
            name == "Throws"
        } else {
            false
        }
    })
}
