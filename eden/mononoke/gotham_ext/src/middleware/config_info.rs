/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::Mutex;

use gotham::state::State;
use gotham::state::StateData;
use hyper::body::Body;
use hyper::Response;
use mononoke_configs::MononokeConfigs;

use crate::middleware::Middleware;

#[derive(StateData, Default)]
pub struct ConfigInfo {
    pub version: String,
    pub last_updated_at: u64,
}

#[derive(Clone)]
pub struct MononokeConfigsWrapper {
    inner: Arc<Mutex<Arc<MononokeConfigs>>>,
}

#[derive(Clone)]
pub struct ConfigInfoMiddleware {
    config: MononokeConfigsWrapper,
}

impl ConfigInfoMiddleware {
    pub fn new(config: Arc<MononokeConfigs>) -> Self {
        Self {
            config: MononokeConfigsWrapper {
                inner: Arc::new(Mutex::new(config)),
            },
        }
    }
}

#[async_trait::async_trait]
impl Middleware for ConfigInfoMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let config = self.config.inner.lock().expect("poisoned lock");
        if let Some(config_info) = config.config_info().as_ref() {
            state.put(ConfigInfo {
                version: config_info.content_hash.clone(),
                last_updated_at: config_info.last_updated_at,
            });
        }

        None
    }
}
