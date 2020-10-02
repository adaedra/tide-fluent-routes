//! Tide Fluent Routes implements a fluent api to define your tide routes.
//!
//! You can register simple endpoints like this;
//! ```rust
//! # use tide::{Request, Result};
//! # use tide_fluent_routes::prelude::*;
//! #
//! # pub async fn endpoint(_: Request<()>) -> Result {
//! #     todo!()
//! # }
//! use tide_fluent_routes::*;
//!
//! let mut server = tide::Server::new();
//!
//! server.register(
//!    root()
//!        .method(Method::Get, endpoint)
//!        .method(Method::Post, endpoint),
//! );
//! ```
//!
//! Or a more complete tree of urls and endpoints like this;
//! ```rust
//! # use tide::{Request, Result};
//! # use tide_fluent_routes::prelude::*;
//! #
//! # pub async fn endpoint(_: Request<()>) -> Result {
//! #     todo!()
//! # }
//! #
//! # let mut server = tide::Server::new();
//!
//! server.register(
//!     root()
//!         .method(Method::Get, endpoint)
//!         .method(Method::Post, endpoint)
//!         .at("api/v1", |route| {
//!             route
//!                 .method(Method::Get, endpoint)
//!                 .method(Method::Post, endpoint)
//!         })
//!         .at("api/v2", |route| {
//!             route
//!                 .method(Method::Get, endpoint)
//!                 .method(Method::Post, endpoint)
//!         }),
//! );
//! ```
//!
//! Adding middleware is easy, and its very clear where the middleware is applied and where not;
//! ```rust
//! # use std::{future::Future, pin::Pin};
//! # use tide::{Next, Request, Result};
//! # use tide_fluent_routes::prelude::*;
//! #
//! # pub async fn endpoint(_: Request<()>) -> Result {
//! #     todo!()
//! # }
//! #
//! # pub fn dummy_middleware<'a>(
//! #     request: Request<()>,
//! #     next: Next<'a, ()>,
//! # ) -> Pin<Box<dyn Future<Output = Result> + Send + 'a>> {
//! #     Box::pin(async { Ok(next.run(request).await) })
//! # }
//! # let mut server = tide::Server::new();
//! server.register(
//!     root()
//!         .method(Method::Get, endpoint)
//!         .method(Method::Post, endpoint)
//!         .at("api/v1", |route| {
//!             route
//!                 .with(dummy_middleware, |route| {
//!                     route.method(Method::Get, endpoint)
//!                 })
//!                .method(Method::Post, endpoint)
//!         })
//!         .at("api/v2", |route| {
//!             route
//!                 .method(Method::Get, endpoint)
//!                 .method(Method::Post, endpoint)
//!         }),
//! );
//! ```

// Turn on warnings for some lints
#![warn(
    missing_debug_implementations,
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_pub,
    unused_import_braces,
    unused_qualifications
)]

use std::collections::HashMap;
use tide::http::Method;
use tide::utils::async_trait;
use tide::{Endpoint, Middleware};

struct BoxedEndpoint<State>(Box<dyn Endpoint<State>>);

impl<State: Clone + Send + Sync + 'static> BoxedEndpoint<State> {
    /// Wrap an endpoint in a BoxedEndpoint
    fn new(endpoint: impl Endpoint<State>) -> Self {
        Self(Box::new(endpoint))
    }
}

#[async_trait]
impl<State: Clone + Send + Sync + 'static> Endpoint<State> for BoxedEndpoint<State> {
    async fn call(&self, req: tide::Request<State>) -> tide::Result {
        self.0.call(req).await
    }
}

/// A router is any component where routes can be registered.
pub trait Router<State: Clone + Send + Sync + 'static> {
    /// Register a single endpoint on the `Router`
    fn register_endpoint(&mut self, path: &str, method: Method, endpoint: impl Endpoint<State>);

    /// Register all routes from a RouteBuilder on the `Router`
    fn register(&mut self, routes: RouteBuilder<State>) {
        for EndpointDescriptor(path, _middleware, method, endpoint) in routes.build() {
            self.register_endpoint(&path, method, endpoint)
        }
    }
}

impl<State: Clone + Send + Sync + 'static> Router<State> for tide::Server<State> {
    fn register_endpoint(&mut self, path: &str, method: Method, endpoint: impl Endpoint<State>) {
        self.at(path).method(method, endpoint);
    }
}

/// Start building a route. Returns a RouteBuilder for the root of your route
pub fn root<State>() -> RouteBuilder<State> {
    RouteBuilder {
        route: RouteSegment::Root,
        branches: Vec::new(),
        endpoints: HashMap::new(),
    }
}

/// A Builder for Tide routes. RouteBuilders can be composed into a tree that represents the tree of
/// path segments, middleware and endpoints that defines the routes in a Tide application. This tree
/// can then be returned as a list of routes to each of the endpoints.
pub struct RouteBuilder<State> {
    route: RouteSegment<State>,

    branches: Vec<RouteBuilder<State>>,
    endpoints: HashMap<Method, BoxedEndpoint<State>>,
}

impl<State: Clone + Send + Sync + 'static> RouteBuilder<State> {
    /// Add sub-routes for a path segment
    pub fn at<R: Fn(Self) -> Self>(self, path: &str, routes: R) -> Self {
        self.add_branch(RouteSegment::Path(path.to_string()), routes)
    }

    /// Add sub-routes for a middleware
    pub fn with<M: Middleware<State>, R: Fn(Self) -> Self>(self, middleware: M, routes: R) -> Self {
        self.add_branch(RouteSegment::Middleware(Box::new(middleware)), routes)
    }

    fn add_branch<R: Fn(Self) -> Self>(mut self, spec: RouteSegment<State>, routes: R) -> Self {
        self.branches.push(routes(RouteBuilder {
            route: spec,
            branches: Vec::new(),
            endpoints: HashMap::new(),
        }));
        self
    }

    /// Add an endpoint
    pub fn method(mut self, method: Method, endpoint: impl Endpoint<State>) -> Self {
        self.endpoints.insert(method, BoxedEndpoint::new(endpoint));
        self
    }

    fn build(self) -> impl Iterator<Item = EndpointDescriptor<State>> {
        let local_endpoints: Vec<EndpointDescriptor<State>> = self.endpoints.into_iter().map(|(method, endpoint)| {
            EndpointDescriptor(String::new(), Vec::new(), method, endpoint)
        }).collect();

        let sub_endpoints: Vec<EndpointDescriptor<State>> = self.branches.into_iter().flat_map(RouteBuilder::build).collect();

        local_endpoints.into_iter().chain(sub_endpoints.into_iter())
    }
}

/// Describes an endpoint, the path to it, its middleware and its HttpMethod
struct EndpointDescriptor<State>(
    String,
    Vec<Box<dyn Middleware<State>>>,
    Method,
    BoxedEndpoint<State>,
);

enum RouteSegment<State> {
    Root,
    Path(String),
    Middleware(Box<dyn Middleware<State>>),
}

/// Import types to use tide_fluent_routes
pub mod prelude {
    pub use super::{Router, root, RouteBuilder};
    pub use tide::http::Method;
}

#[cfg(test)]
mod test {
    use super::*;

}
