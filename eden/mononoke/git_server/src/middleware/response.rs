/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::State;
use gotham_ext::middleware::Middleware;
use http::header::CONTENT_TYPE;
use http::HeaderValue;
use hyper::body::Body;
use hyper::Response;

use crate::model::ResponseType;
use crate::model::Service;

#[derive(Clone)]
pub struct ResponseContentTypeMiddleware {}

#[async_trait::async_trait]
impl Middleware for ResponseContentTypeMiddleware {
    async fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        if let (Some(service_type), Some(response_type)) = (
            state.try_borrow::<Service>(),
            state.try_borrow::<ResponseType>(),
        ) {
            let content_type_header = format!("application/x-{}-{}", service_type, response_type);
            response.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_str(&content_type_header).unwrap(),
            );
        }
    }
}
