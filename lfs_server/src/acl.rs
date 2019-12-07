/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use if_ as acl_constants;

use aclchecker::{AclChecker, Identity};
use anyhow::{bail, Error};
use fbinit::FacebookInit;
use futures_ext::asynchronize;
use futures_util::compat::Future01CompatExt;
use slog::{info, Logger};

const ACL_CHECKER_TIMEOUT_MS: u32 = 10_000;

#[derive(Clone)]
pub enum LfsAclChecker {
    AclChecker(Option<AclChecker>),
    TestAclChecker(Vec<Identity>),
}

impl LfsAclChecker {
    pub async fn new_acl_checker(
        fb: FacebookInit,
        repo_name: &str,
        logger: &Logger,
        acl_name: Option<String>,
    ) -> Result<LfsAclChecker, Error> {
        if let Some(acl) = acl_name {
            info!(
                logger,
                "{}: Actions will be checked against {} ACL", repo_name, acl
            );
            let identity = Identity::new(acl_constants::REPO, &acl);

            asynchronize(move || {
                let acl_checker = AclChecker::new(fb, &identity)?;
                if acl_checker.do_wait_updated(ACL_CHECKER_TIMEOUT_MS) {
                    Ok(Self::AclChecker(Some(acl_checker)))
                } else {
                    bail!("Failed to update AclChecker")
                }
            })
            .compat()
            .await
        } else {
            Ok(Self::AclChecker(None))
        }
    }

    pub fn is_allowed(&self, identities: &Vec<Identity>, actions: &[&str]) -> bool {
        match self {
            Self::AclChecker(Some(aclchecker)) => aclchecker.check(identities.as_ref(), actions),
            Self::TestAclChecker(allowed_idents) => allowed_idents
                .iter()
                .any(|ident| identities.contains(ident)),
            // If no ACL checking is configured, allow everything.
            _ => true,
        }
    }
}
