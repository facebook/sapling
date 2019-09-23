// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use futures::{future, Future};
use gotham::handler::HandlerFuture;
use gotham::helpers::http::header::X_REQUEST_ID;
use gotham::middleware::Middleware;
use gotham::state::{request_id, State};
use gotham_derive::NewMiddleware;
use hyper::header::HeaderValue;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;

#[derive(Clone, NewMiddleware)]
pub struct IdentityMiddleware {
    headers: Arc<HashMap<&'static str, HeaderValue>>,
}

impl IdentityMiddleware {
    pub fn new() -> Self {
        let mut headers = HashMap::new();

        headers.insert("Server", HeaderValue::from_static("mononoke-lfs"));

        // NOTE: We ignore errors here — those will happen if environment variables are missing,
        // which is fine.
        let _ = Self::add_tw_task(&mut headers);
        let _ = Self::add_tw_task_version(&mut headers);
        let _ = Self::add_tw_canary_id(&mut headers);

        Self {
            headers: Arc::new(headers),
        }
    }

    fn add_tw_task(headers: &mut HashMap<&'static str, HeaderValue>) -> Result<(), Error> {
        let tw_job_cluster = env::var("TW_JOB_CLUSTER")?;
        let tw_job_user = env::var("TW_JOB_USER")?;
        let tw_job_name = env::var("TW_JOB_NAME")?;
        let tw_task_id = env::var("TW_TASK_ID")?;
        let task = format!(
            "{}/{}/{}/{}",
            tw_job_cluster, tw_job_user, tw_job_name, tw_task_id
        );
        let header = HeaderValue::from_str(&task)?;
        headers.insert("X-TW-Task", header);
        Ok(())
    }

    fn add_tw_task_version(headers: &mut HashMap<&'static str, HeaderValue>) -> Result<(), Error> {
        let tw_task_version = env::var("TW_TASK_VERSION")?;
        let header = HeaderValue::from_str(&tw_task_version)?;
        headers.insert("X-TW-Task-Version", header);
        Ok(())
    }

    fn add_tw_canary_id(headers: &mut HashMap<&'static str, HeaderValue>) -> Result<(), Error> {
        let tw_canary_id = env::var("TW_CANARY_ID")?;
        let header = HeaderValue::from_str(&tw_canary_id)?;
        headers.insert("X-TW-Canary-Id", header);
        Ok(())
    }
}

impl Middleware for IdentityMiddleware {
    fn call<Chain>(self, state: State, chain: Chain) -> Box<HandlerFuture>
    where
        Chain: FnOnce(State) -> Box<HandlerFuture>,
    {
        let f = chain(state).and_then(move |(state, mut response)| {
            let headers = response.headers_mut();
            for (header, value) in self.headers.iter() {
                headers.insert(*header, value.clone());
            }

            if let Ok(id) = HeaderValue::from_str(request_id(&state)) {
                headers.insert(X_REQUEST_ID, id);
            }

            future::ok((state, response))
        });

        Box::new(f)
    }
}
