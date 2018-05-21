#[macro_use]
extern crate failure;
extern crate serde;
#[macro_use]
extern crate serde_json;
extern crate watchman_client;

mod hgclient;
pub use hgclient::HgWatchmanClient;
pub use watchman_client::queries::*;
