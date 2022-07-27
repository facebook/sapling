/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use crate::AclProvider;
use crate::BoxMembershipChecker;
use crate::BoxPermissionChecker;
use crate::MembershipChecker;
use crate::MononokeIdentitySet;
use crate::PermissionChecker;

pub struct InternalAclProvider {
    acls: Acls,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Acls {
    #[serde(default)]
    pub repos: HashMap<String, Arc<Acl>>,

    #[serde(default)]
    pub repo_regions: HashMap<String, Arc<Acl>>,

    #[serde(default)]
    pub tiers: HashMap<String, Arc<Acl>>,

    #[serde(default)]
    pub groups: HashMap<String, Arc<MononokeIdentitySet>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Acl {
    pub actions: HashMap<String, MononokeIdentitySet>,
}

pub struct AclMembershipChecker {
    group: Arc<MononokeIdentitySet>,
}

#[async_trait]
impl MembershipChecker for AclMembershipChecker {
    async fn is_member(&self, identities: &MononokeIdentitySet) -> Result<bool> {
        Ok(!self.group.is_disjoint(identities))
    }
}

pub struct AclPermissionChecker {
    acl: Arc<Acl>,
}

#[async_trait]
impl PermissionChecker for AclPermissionChecker {
    async fn check_set(&self, identities: &MononokeIdentitySet, actions: &[&str]) -> Result<bool> {
        for action in actions {
            // AclChecker uses the first action that exists
            if let Some(granted) = self.acl.actions.get(*action) {
                return Ok(!granted.is_disjoint(identities));
            }
        }
        // If none of the actions were present, the check fails.
        Ok(false)
    }
}

impl InternalAclProvider {
    pub fn new(acls: Acls) -> Box<dyn AclProvider> {
        Box::new(InternalAclProvider { acls })
    }

    pub fn from_file(path: impl AsRef<Path>) -> Result<Box<dyn AclProvider>> {
        let file = File::open(path.as_ref())?;
        let reader = BufReader::new(file);
        let acls = serde_json::from_reader(reader)?;
        Ok(Self::new(acls))
    }
}

#[async_trait]
impl AclProvider for InternalAclProvider {
    async fn repo_acl(&self, name: &str) -> Result<BoxPermissionChecker> {
        Ok(Box::new(AclPermissionChecker {
            acl: self.acls.repos.get(name).cloned().unwrap_or_default(),
        }))
    }

    async fn repo_region_acl(&self, name: &str) -> Result<BoxPermissionChecker> {
        Ok(Box::new(AclPermissionChecker {
            acl: self
                .acls
                .repo_regions
                .get(name)
                .cloned()
                .unwrap_or_default(),
        }))
    }

    async fn tier_acl(&self, name: &str) -> Result<BoxPermissionChecker> {
        Ok(Box::new(AclPermissionChecker {
            acl: self.acls.tiers.get(name).cloned().unwrap_or_default(),
        }))
    }

    async fn group(&self, name: &str) -> Result<BoxMembershipChecker> {
        Ok(Box::new(AclMembershipChecker {
            group: self.acls.groups.get(name).cloned().unwrap_or_default(),
        }))
    }

    async fn admin_group(&self) -> Result<BoxMembershipChecker> {
        self.group("admin").await
    }

    async fn reviewers_group(&self) -> Result<BoxMembershipChecker> {
        self.group("reviewers").await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;

    fn ids(ids: &[&str]) -> Result<MononokeIdentitySet> {
        let mut set = MononokeIdentitySet::new();
        for id in ids {
            set.insert(id.parse()?);
        }
        Ok(set)
    }

    #[fbinit::test]
    async fn json_acls(_fb: FacebookInit) -> Result<()> {
        let json = r##"
            {
                "repos": {
                    "repo1": {
                        "actions": {
                            "read": ["USER:user1"]
                        }
                    }
                },
                "groups": {
                    "users": [
                        "USER:user1",
                        "USER:user2"
                    ]
                }
            }
        "##;
        let acls = serde_json::from_str(json)?;
        let prov = InternalAclProvider::new(acls);
        let users_group = prov.group("users").await?;
        assert!(users_group.is_member(&ids(&["USER:user1"])?).await?);
        assert!(!users_group.is_member(&ids(&["USER:impostor"])?).await?);
        let repo1 = prov.repo_acl("repo1").await?;
        assert!(
            repo1
                .check_set(&ids(&["IP:localhost", "USER:user1"])?, &["access", "read"])
                .await?
        );
        assert!(
            !repo1
                .check_set(&ids(&["IP:localhost", "USER:impostor"])?, &["read"])
                .await?
        );
        assert!(
            !repo1
                .check_set(&ids(&["IP:localhost", "USER:user1"])?, &["write"])
                .await?
        );
        Ok(())
    }
}
