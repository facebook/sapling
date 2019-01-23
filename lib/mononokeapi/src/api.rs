// Copyright Facebook, Inc. 2019

use failure::{ensure, Error, Fallible};
use futures::{Future, Stream};
use http::uri::Uri;
use tokio::runtime::Runtime;

use crate::client::MononokeClient;

mod paths {
    pub const HEALTH_CHECK: &str = "/health_check";
}

pub trait MononokeApi {
    fn health_check(&self) -> Fallible<()>;
}

impl MononokeApi for MononokeClient {
    /// Hit the API server's /health_check endpoint.
    /// Returns Ok(()) if the expected response is received, or an Error otherwise
    /// (e.g., if there was a connection problem or an unexpected repsonse).
    fn health_check(&self) -> Fallible<()> {
        let url = self
            .base_url
            .join(paths::HEALTH_CHECK)?
            .as_str()
            .parse::<Uri>()?;

        let fut = self.client.get(url).map_err(Error::from).and_then(|res| {
            let status = res.status();
            res.into_body()
                .concat2()
                .from_err()
                .and_then(|body| Ok(String::from_utf8(body.into_bytes().to_vec())?))
                .map(move |body| (status, body))
        });

        let mut runtime = Runtime::new()?;
        let (status, body) = runtime.block_on(fut)?;

        ensure!(
            status.is_success(),
            "Request failed (status code: {:?}): {:?}",
            &status,
            &body
        );
        ensure!(body == "I_AM_ALIVE", "Unexpected response: {:?}", &body);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use users::{get_current_uid, get_user_by_uid};

    use std::path::PathBuf;

    use crate::MononokeClientBuilder;

    const HOST: &str = "http://127.0.0.1:8000/";

    fn configure_client() -> Fallible<MononokeClient> {
        MononokeClientBuilder::new()
            .base_url_str(HOST)?
            .client_creds(get_creds_path())
            .build()
    }

    fn get_creds_path() -> PathBuf {
        let uid = get_current_uid();
        let user = get_user_by_uid(uid).expect(&format!("uid {} not found", uid));
        let name = user
            .name()
            .to_str()
            .expect(&format!("username {:?} is not valid UTF-8", user.name()));
        PathBuf::from(format!(
            "/var/facebook/credentials/{user}/x509/{user}.pem",
            user = &name
        ))
    }

    #[test]
    #[ignore] // Talks to production Mononoke; ignore by default.
    fn health_check() -> Fallible<()> {
        configure_client()?.health_check()
    }
}
