/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::task;
use gotham::handler::Handler;
use gotham::handler::HandlerFuture;
use gotham::handler::IntoResponse;
use gotham::handler::NewHandler;
use gotham::state::State;
use hyper::service::Service;
use hyper::Body;
use hyper::Request;
use hyper::Response;
use std::net::SocketAddr;
use std::panic::RefUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;

use crate::middleware::Middleware;
use crate::socket_data::TlsSocketData;

#[derive(Clone)]
pub struct MononokeHttpHandler<H> {
    inner: H,
    middleware: Arc<Vec<Box<dyn Middleware>>>,
}

impl<H> MononokeHttpHandler<H> {
    pub fn into_service(
        self,
        addr: SocketAddr,
        tls_socket_data: Option<TlsSocketData>,
    ) -> MononokeHttpHandlerAsService<H> {
        MononokeHttpHandlerAsService {
            handler: self,
            addr,
            tls_socket_data,
        }
    }
}

// This trait is boilerplate to let us bind this in Gotham as the service. This is essentially
// patterned match off of how the Router Handler works in Gotham.
impl<H: Handler + Clone + Send + Sync + 'static + RefUnwindSafe> NewHandler
    for MononokeHttpHandler<H>
{
    type Instance = MononokeHttpHandler<H>;

    fn new_handler(&self) -> Result<Self::Instance, anyhow::Error> {
        Ok(self.clone())
    }
}

/// Executes a stack middleware. If any Middleware instance preempts the request, it returns the
/// index of the first middleware that didn't run and the response.
async fn run_middleware(
    middleware: &[Box<dyn Middleware>],
    state: &mut State,
) -> Option<(usize, Response<Body>)> {
    for (i, m) in middleware.iter().enumerate() {
        if let Some(response) = m.inbound(state).await {
            return Some((i + 1, response));
        }
    }

    None
}

// Gotham's router only runs middleware on route matches, which means we don't get any visibility
// into e.g. 404's. So, we use this handler to wrap Gotham's router and run our own middleware
// stack. Another reason not to use Gotham's middleware stack is that it places all middleware
// instances in the poll chain, so for e.g. an upload, we'd chain poll() calls through every single
// instance of middleware. In contrast, our middleware stack is completely out of the way once it
// has finished executing.
impl<H: Handler + Send + Sync + 'static> Handler for MononokeHttpHandler<H> {
    fn handle(self, mut state: State) -> Pin<Box<HandlerFuture>> {
        let fut = async move {
            // On request, middleware is called in order, then called the other way around on
            // response (this is what regular Router middleware in Gotham would do).
            let (idx, mut state, mut response) =
                match run_middleware(self.middleware.as_ref(), &mut state).await {
                    Some((idx, r)) => (idx, state, r),
                    None => {
                        // NOTE: It's a bit unfortunate that we have to return a HandlerFuture here
                        // when really we'd rather be working with just (State, HttpResponse<Body>)
                        // everywhere, but that's how it is in Gotham.
                        let (state, res) = match self.inner.handle(state).await {
                            Ok((state, res)) => (state, res),
                            Err((state, err)) => {
                                let response = err.into_response(&state);
                                (state, response)
                            }
                        };

                        let idx = self.middleware.len();
                        (idx, state, res)
                    }
                };

            // On outbound, only run middleware that did run in inbound.
            for m in self.middleware[0..idx].iter().rev() {
                m.outbound(&mut state, &mut response).await;
            }

            Ok((state, response))
        };

        fut.boxed()
    }
}

// NOTE: This is just syntactic sugar for MononokeHttpHandler::builder(), which is why the
// "handler" type is () here.
impl MononokeHttpHandler<()> {
    pub fn builder() -> MononokeHttpHandlerBuilder {
        MononokeHttpHandlerBuilder { middleware: vec![] }
    }
}

pub struct MononokeHttpHandlerBuilder {
    middleware: Vec<Box<dyn Middleware>>,
}

impl MononokeHttpHandlerBuilder {
    pub fn add<T: Middleware>(mut self, middleware: T) -> Self {
        self.middleware.push(Box::new(middleware));
        self
    }

    pub fn build<H: Handler + Send + Sync + 'static>(self, inner: H) -> MononokeHttpHandler<H> {
        MononokeHttpHandler {
            inner,
            middleware: Arc::new(self.middleware),
        }
    }
}

/// This is an instance of MononokeHttpHandlerAsService that is connected to a client. We can use
/// it to call into Gotham explicitly, or use it as a Hyper service.
#[derive(Clone)]
pub struct MononokeHttpHandlerAsService<H> {
    handler: MononokeHttpHandler<H>,
    addr: SocketAddr,
    tls_socket_data: Option<TlsSocketData>,
}

impl<H: Handler + Clone + Send + Sync + 'static + RefUnwindSafe> MononokeHttpHandlerAsService<H> {
    pub async fn call_gotham(self, req: Request<Body>) -> Response<Body> {
        let mut state = State::from_request(req, self.addr);
        if let Some(tls_socket_data) = self.tls_socket_data {
            tls_socket_data.populate_state(&mut state);
        }

        let res = match self.handler.handle(state).await {
            Ok((_state, res)) => res,
            Err((state, err)) => err.into_response(&state),
        };

        res
    }
}

impl<H: Handler + Clone + Send + Sync + 'static + RefUnwindSafe> Service<Request<Body>>
    for MononokeHttpHandlerAsService<H>
{
    type Response = Response<Body>;
    type Error = anyhow::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> task::Poll<Result<(), Self::Error>> {
        task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let this = self.clone();

        async move {
            let res = this.call_gotham(req).await;
            Ok(res)
        }
        .boxed()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::future;
    use gotham::test::TestServer;
    use gotham_derive::StateData;
    use hyper::http::StatusCode;
    use hyper::Body;

    // Basic response handler for tests

    #[derive(Clone)]
    struct TestHandler;

    impl Handler for TestHandler {
        fn handle(self, state: State) -> Pin<Box<HandlerFuture>> {
            let response = Response::builder()
                .status(StatusCode::OK)
                .body(Body::empty())
                .unwrap();

            future::ready(Ok((state, response))).boxed()
        }
    }

    // Handler that expects to not be called

    #[derive(Clone)]
    struct PanicHandler;

    impl Handler for PanicHandler {
        fn handle(self, _state: State) -> Pin<Box<HandlerFuture>> {
            panic!("PanicHandler::handle was called")
        }
    }

    // Basic middleware for tests

    struct NoopMiddleware;

    #[async_trait::async_trait]
    impl Middleware for NoopMiddleware {}

    // MiddlewareValueMiddleware is used below for asserting that middleware is called in the right
    // order.

    #[derive(StateData)]
    pub struct MiddlewareValue(u64);

    struct MiddlewareValueMiddleware(Option<u64>, u64);

    #[async_trait::async_trait]
    impl Middleware for MiddlewareValueMiddleware {
        async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
            assert_eq!(state.try_borrow::<MiddlewareValue>().map(|v| v.0), self.0);
            state.put(MiddlewareValue(self.1));
            None
        }

        async fn outbound(&self, state: &mut State, _response: &mut Response<Body>) {
            assert_eq!(state.take::<MiddlewareValue>().0, self.1);

            if let Some(v) = self.0 {
                state.put(MiddlewareValue(v))
            }
        }
    }

    // InterceptMiddleware is used to check that middleware can intercept requests.

    struct InterceptMiddleware;

    #[async_trait::async_trait]
    impl Middleware for InterceptMiddleware {
        async fn inbound(&self, _state: &mut State) -> Option<Response<Body>> {
            let response = Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap();
            Some(response)
        }

        async fn outbound(&self, _state: &mut State, response: &mut Response<Body>) {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
    }

    // PanicMiddleware is used to check that intercepting a request interrupts the call chain.

    struct PanicMiddleware;

    #[async_trait::async_trait]
    impl Middleware for PanicMiddleware {
        async fn inbound(&self, _state: &mut State) -> Option<Response<Body>> {
            panic!("PanicMiddleware::inbound was called");
        }

        async fn outbound(&self, _state: &mut State, _response: &mut Response<Body>) {
            panic!("PanicMiddleware::outbound was called");
        }
    }

    #[test]
    fn test_empty() -> Result<(), anyhow::Error> {
        let handler = MononokeHttpHandler::builder().build(TestHandler);
        let server = TestServer::new(handler)?;
        let res = server.client().get("http://host/").perform()?;
        assert_eq!(res.status(), StatusCode::OK);
        Ok(())
    }

    #[test]
    fn test_noop() -> Result<(), anyhow::Error> {
        let handler = MononokeHttpHandler::builder()
            .add(NoopMiddleware)
            .add(NoopMiddleware)
            .build(TestHandler);
        let server = TestServer::new(handler)?;
        let res = server.client().get("http://host/").perform()?;
        assert_eq!(res.status(), StatusCode::OK);
        Ok(())
    }

    #[test]
    fn test_chain() -> Result<(), anyhow::Error> {
        let handler = MononokeHttpHandler::builder()
            .add(MiddlewareValueMiddleware(None, 1))
            .add(MiddlewareValueMiddleware(Some(1), 2))
            .build(TestHandler);
        let server = TestServer::new(handler)?;
        let res = server.client().get("http://host/").perform()?;
        assert_eq!(res.status(), StatusCode::OK);
        Ok(())
    }

    #[test]
    fn test_intercept_alone() -> Result<(), anyhow::Error> {
        let handler = MononokeHttpHandler::builder()
            .add(InterceptMiddleware)
            .build(TestHandler);
        let server = TestServer::new(handler)?;
        let res = server.client().get("http://host/").perform()?;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        Ok(())
    }

    #[test]
    fn test_intercept_chain() -> Result<(), anyhow::Error> {
        let handler = MononokeHttpHandler::builder()
            .add(MiddlewareValueMiddleware(None, 1))
            .add(MiddlewareValueMiddleware(Some(1), 2))
            .add(InterceptMiddleware)
            .add(PanicMiddleware)
            .build(PanicHandler);
        let server = TestServer::new(handler)?;
        let res = server.client().get("http://host/").perform()?;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        Ok(())
    }
}
