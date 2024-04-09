// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

use fbwhoami::FbWhoAmI;

#[derive(PartialEq)]
#[repr(u8)]
pub enum Env {
    Prod = 0,
    Corp = 1,
    Lab = 2,
}

pub fn get_env() -> Env {
    let whoami = if let Ok(whoami) = FbWhoAmI::get() {
        whoami
    } else {
        // default to corp env if no fbwhoami
        return Env::Corp;
    };

    // Corp host e.g. a lab
    if let Some(hostname_scheme) = &whoami.hostname_scheme {
        if hostname_scheme.starts_with("corp_") {
            return Env::Corp;
        }
        if hostname_scheme.starts_with("lab_") {
            return Env::Lab;
        }
    }

    // Cloud hosts e.g. AWS are treated like corp
    if whoami.cloud_provider.is_some() {
        return Env::Corp;
    }

    // Default to prod
    Env::Prod
}

/// Returns true if the running host is on the production network.
pub fn is_prod() -> bool {
    get_env() == Env::Prod
}

/// Returns true if the running host is on the corp network.
pub fn is_corp() -> bool {
    get_env() == Env::Corp
}

/// Returns true if the running host is on the lab network.
pub fn is_lab() -> bool {
    get_env() == Env::Lab
}
