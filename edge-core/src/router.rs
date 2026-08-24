//! A small, platform-agnostic router (SPEC §6.6).
//!
//! Patterns use `matchit` syntax: `/users/:id` for params, `*rest` for
//! wildcards. Handlers receive owned [`RouteParams`] and a cloned [`Context`].

use std::collections::HashMap;
use std::fmt;
use std::future::Future;

use futures_util::future::BoxFuture;

use crate::context::Context;
use crate::error::{Error, PathError};
use crate::types::{EdgeRequest, EdgeResponse, Method};
use crate::Result;

/// A boxed route handler.
///
/// Takes the request, extracted path params, and the platform context (by
/// value — a cheap `Arc` clone).
pub type Handler = Box<
    dyn Fn(EdgeRequest, RouteParams, Context) -> BoxFuture<'static, Result<EdgeResponse>>
        + Send
        + Sync,
>;

/// Wrap an async function (or closure returning a future) into a [`Handler`].
///
/// ```ignore
/// router.get("/hello/:name", handler(|req, params, ctx| async move {
///     Ok(EdgeResponse::ok("hi"))
/// }))?;
/// ```
pub fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(EdgeRequest, RouteParams, Context) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<EdgeResponse>> + Send + 'static,
{
    Box::new(move |req, params, ctx| Box::pin(f(req, params, ctx)))
}

/// Extracted path parameters, owned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteParams {
    params: HashMap<String, String>,
}

impl RouteParams {
    /// Build from matchit's borrowed params (crate-internal).
    pub(crate) fn from_matchit(params: &matchit::Params<'_, '_>) -> Self {
        let mut map = HashMap::new();
        for (k, v) in params.iter() {
            map.insert(k.to_string(), v.to_string());
        }
        Self { params: map }
    }

    /// Get a parameter by name.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    /// True if no parameters were captured.
    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// Iterate over `(name, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.params.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

type Entry = (Option<Method>, Handler);

/// A method-aware path router.
#[derive(Default)]
pub struct Router {
    inner: matchit::Router<Entry>,
}

impl fmt::Debug for Router {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Router { .. }")
    }
}

impl Router {
    /// Create an empty router.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a handler for any HTTP method.
    pub fn route(&mut self, pattern: &str, handler: Handler) -> Result<()> {
        self.insert(pattern, None, handler)
    }

    /// Register a handler for `GET` requests.
    pub fn get(&mut self, pattern: &str, handler: Handler) -> Result<()> {
        self.method_route(Method::GET, pattern, handler)
    }

    /// Register a handler for `POST` requests.
    pub fn post(&mut self, pattern: &str, handler: Handler) -> Result<()> {
        self.method_route(Method::POST, pattern, handler)
    }

    /// Register a handler for `PUT` requests.
    pub fn put(&mut self, pattern: &str, handler: Handler) -> Result<()> {
        self.method_route(Method::PUT, pattern, handler)
    }

    /// Register a handler for `DELETE` requests.
    pub fn delete(&mut self, pattern: &str, handler: Handler) -> Result<()> {
        self.method_route(Method::DELETE, pattern, handler)
    }

    /// Register a handler for `PATCH` requests.
    pub fn patch(&mut self, pattern: &str, handler: Handler) -> Result<()> {
        self.method_route(Method::PATCH, pattern, handler)
    }

    /// Register a handler for `HEAD` requests.
    pub fn head(&mut self, pattern: &str, handler: Handler) -> Result<()> {
        self.method_route(Method::HEAD, pattern, handler)
    }

    /// Register a handler for `OPTIONS` requests.
    pub fn options(&mut self, pattern: &str, handler: Handler) -> Result<()> {
        self.method_route(Method::OPTIONS, pattern, handler)
    }

    /// Dispatch a request, or return [`PathError::NotFound`] if no route
    /// matches (or the method does not match).
    pub async fn handle(&self, req: EdgeRequest, ctx: &mut Context) -> Result<EdgeResponse> {
        let path = req.uri().path().to_string();
        let matched = self
            .inner
            .at(&path)
            .map_err(|_| Error::Router(PathError::NotFound))?;

        let (method, handler) = matched.value;
        if let Some(expected) = method {
            if req.method() != expected {
                return Err(Error::Router(PathError::NotFound));
            }
        }

        let params = RouteParams::from_matchit(&matched.params);
        handler(req, params, ctx.clone()).await
    }

    fn method_route(&mut self, method: Method, pattern: &str, handler: Handler) -> Result<()> {
        self.insert(pattern, Some(method), handler)
    }

    fn insert(&mut self, pattern: &str, method: Option<Method>, handler: Handler) -> Result<()> {
        self.inner
            .insert(pattern, (method, handler))
            .map_err(|e| Error::Router(PathError::InvalidPattern(e.to_string())))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MockContextBuilder;
    use crate::Body;

    async fn greet(_req: EdgeRequest, params: RouteParams, _ctx: Context) -> Result<EdgeResponse> {
        Ok(crate::ResponseExt::ok(format!(
            "hi {}",
            params.get("name").unwrap_or("?")
        )))
    }

    #[tokio::test]
    async fn params_are_extracted() {
        let mut router = Router::new();
        router.get("/hello/:name", handler(greet)).unwrap();

        let mut ctx = MockContextBuilder::new().build().context();
        let req = http::Request::builder()
            .method("GET")
            .uri("/hello/alice")
            .body(Body::new())
            .unwrap();
        let resp = router.handle(req, &mut ctx).await.unwrap();
        assert_eq!(resp.body().as_bytes(), Some(&b"hi alice"[..]));
    }

    #[tokio::test]
    async fn method_mismatch_is_not_found() {
        let mut router = Router::new();
        router.get("/only-get", handler(greet)).unwrap();

        let mut ctx = MockContextBuilder::new().build().context();
        let req = http::Request::builder()
            .method("POST")
            .uri("/only-get")
            .body(Body::new())
            .unwrap();
        let err = router.handle(req, &mut ctx).await.unwrap_err();
        assert!(matches!(err, Error::Router(PathError::NotFound)));
    }

    #[tokio::test]
    async fn unknown_path_is_not_found() {
        let router = Router::new();
        let mut ctx = MockContextBuilder::new().build().context();
        let req = http::Request::builder()
            .uri("/nope")
            .body(Body::new())
            .unwrap();
        let err = router.handle(req, &mut ctx).await.unwrap_err();
        assert!(matches!(err, Error::Router(PathError::NotFound)));
    }

    #[test]
    fn invalid_pattern_is_rejected() {
        let mut router = Router::new();
        // Catch-all segments are only allowed at the end of a route.
        let err = router.get("/foo/*rest/bar", handler(greet)).unwrap_err();
        assert!(matches!(err, Error::Router(PathError::InvalidPattern(_))));
    }
}
