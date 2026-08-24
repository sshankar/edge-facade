//! `#[edge_core::main]` — the Edge SDK entry macro (SPEC §6.2).
//!
//! Applied to the user's async handler:
//!
//! ```ignore
//! use edge_core::{Context, EdgeRequest, EdgeResponse, Result};
//!
//! #[edge_core::main]
//! async fn main(req: EdgeRequest, ctx: Context) -> Result<EdgeResponse> {
//!     Ok(EdgeResponse::ok("hello"))
//! }
//! ```
//!
//! The expansion is feature-selected in the *service crate's* context:
//!
//! * `--features fastly` — emits the Fastly sync entry:
//!   `fn main() -> Result<(), edge_fastly::Error>` which builds the request
//!   and [`Context`](https://docs.rs/edge-core) from the embedded `edge.toml`
//!   (`include_str!` of `<CARGO_MANIFEST_DIR>/edge.toml`), drives the async
//!   handler with the adapter's poll-loop executor (SPEC §8.3), sends the
//!   response, and converts handler errors to a 500 (SPEC §6.2, same
//!   convention as `fastly::main`).
//! * `--features cloudflare` — emits the workers-rs fetch glue (M2),
//!   embedding `edge.toml` (D9) so the adapter resolves the default KV
//!   handle from the config (SPEC §8.1).
//! * Both or neither — a `compile_error!` with a clear message.
//!
//! The user's function is renamed internally so the emitted `main`/`fetch`
//! entry points can share its name space.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, FnArg, ItemFn, ReturnType, Signature};

/// The renamed handler used by the generated entry points.
const INNER_NAME: &str = "__edge_main_impl";

/// Entry-point attribute; see the crate docs.
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let item = parse_macro_input!(input as ItemFn);
    match expand(item) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand(item: ItemFn) -> syn::Result<TokenStream> {
    validate_signature(&item.sig)?;

    let ItemFn {
        attrs,
        vis,
        sig,
        block,
    } = item;

    // Rename the handler so the generated `main`/`fetch` entry points can
    // exist alongside it.
    let inner_ident = syn::Ident::new(INNER_NAME, Span::call_site());
    let inner_sig = Signature {
        ident: inner_ident.clone(),
        ..sig
    };
    let inner = quote! {
        #(#attrs)*
        #vis #inner_sig #block
    };

    let fastly_main = quote! {
        // SPEC §6.2 fastly expansion: sync entry driving the async handler.
        // `std::result::Result` is spelled out: the downstream crate may have
        // its own `Result` alias in scope.
        #[cfg(feature = "fastly")]
        fn main() -> std::result::Result<(), edge_fastly::Error> {
            edge_fastly::serve(
                #inner_ident,
                edge_core::config::EdgeConfig::from_toml_str(
                    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/edge.toml")),
                )?,
            )
        }
    };

    // SPEC §6.2 cloudflare expansion: workers-rs fetch glue. The handler
    // runs on the JS event loop; errors become 500/404 responses (D12),
    // same convention as the fastly branch. `edge.toml` is embedded in the
    // service crate (D9) so the adapter can resolve the `default` KV handle
    // to the configured binding (SPEC §8.1); a bad config rejects the
    // fetch promise with 500.
    let cloudflare_fetch = quote! {
        #[cfg(feature = "cloudflare")]
        #[::edge_cloudflare::wasm_bindgen::prelude::wasm_bindgen]
        pub fn fetch(
            req: ::edge_cloudflare::worker_sys::web_sys::Request,
            env: ::edge_cloudflare::WorkerEnv,
            _ctx: ::edge_cloudflare::worker_sys::Context,
        ) -> ::edge_cloudflare::js_sys::Promise {
            let config = match ::edge_core::config::EdgeConfig::from_toml_str(
                include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/edge.toml")),
            ) {
                Ok(c) => c,
                Err(e) => {
                    return ::edge_cloudflare::js_sys::Promise::reject(
                        &::edge_cloudflare::wasm_bindgen::JsValue::from_str(
                            &::std::format!("edge: invalid edge.toml: {e}"),
                        ),
                    );
                }
            };
            ::edge_cloudflare::js_sys::futures::future_to_promise(
                ::std::panic::AssertUnwindSafe(async move {
                    let resp: ::edge_cloudflare::worker_sys::web_sys::Response =
                        match ::edge_cloudflare::serve_fetch(req, env, config, #inner_ident).await
                        {
                            Ok(resp) => resp,
                            Err(e) => {
                                ::edge_cloudflare::console_error!("edge: {}", &e);
                                ::edge_cloudflare::error_to_response(&e)
                            }
                        };
                    Ok(::edge_cloudflare::wasm_bindgen::JsValue::from(resp))
                }),
            )
        }
    };

    let feature_matrix = quote! {
        #[cfg(not(any(feature = "fastly", feature = "cloudflare")))]
        compile_error!(
            "edge_core::main requires exactly one platform feature; \
             enable `--features fastly` or `--features cloudflare` on the service crate"
        );
        #[cfg(all(feature = "fastly", feature = "cloudflare"))]
        compile_error!(
            "edge_core::main: the `fastly` and `cloudflare` features are mutually exclusive; \
             enable exactly one"
        );
    };

    Ok(quote! {
        #inner
        #fastly_main
        #cloudflare_fetch
        #feature_matrix
    }
    .into())
}

fn validate_signature(sig: &Signature) -> syn::Result<()> {
    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            sig,
            "`edge_core::main` requires an `async fn`",
        ));
    }
    if sig.inputs.len() != 2 {
        return Err(syn::Error::new_spanned(
            sig,
            "`edge_core::main` expects exactly two arguments: (req: EdgeRequest, ctx: Context)",
        ));
    }
    for input in &sig.inputs {
        if let FnArg::Receiver(_) = input {
            return Err(syn::Error::new_spanned(
                input,
                "`edge_core::main` cannot be applied to a method",
            ));
        }
    }
    match &sig.output {
        ReturnType::Default => Err(syn::Error::new_spanned(
            sig,
            "`edge_core::main` must return `Result<EdgeResponse, _>`",
        )),
        ReturnType::Type(_, ty) => {
            let is_result = matches!(
                ty.as_ref(),
                syn::Type::Path(tp)
                    if tp.path.segments.last().map(|s| s.ident == "Result").unwrap_or(false)
            );
            if !is_result {
                return Err(syn::Error::new_spanned(
                    ty,
                    "`edge_core::main` must return `Result<EdgeResponse, _>` (or an alias of it)",
                ));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn accepts_async_two_arg_result_fn() {
        let sig: Signature = parse_quote!(
            async fn main(req: edge_core::EdgeRequest, ctx: edge_core::Context)
                -> edge_core::Result<edge_core::EdgeResponse>
        );
        validate_signature(&sig).unwrap();
    }

    #[test]
    fn rejects_sync_fn() {
        let sig: Signature = parse_quote!(
            fn main(req: edge_core::EdgeRequest, ctx: edge_core::Context)
                -> edge_core::Result<edge_core::EdgeResponse>
        );
        assert!(validate_signature(&sig).is_err());
    }

    #[test]
    fn rejects_wrong_arg_count() {
        let sig: Signature = parse_quote!(
            async fn main(req: edge_core::EdgeRequest)
                -> edge_core::Result<edge_core::EdgeResponse>
        );
        assert!(validate_signature(&sig).is_err());
    }

    #[test]
    fn rejects_missing_result_return() {
        let sig: Signature = parse_quote!(
            async fn main(req: edge_core::EdgeRequest, ctx: edge_core::Context) -> ()
        );
        assert!(validate_signature(&sig).is_err());
    }
}
