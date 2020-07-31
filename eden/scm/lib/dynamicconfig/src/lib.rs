/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::convert::TryInto;
use std::fs;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[cfg(not(feature = "fb"))]
use anyhow::Error;
use anyhow::{anyhow, bail, Result};
use hostname;
use minibytes::Text;

use configparser::config::ConfigSet;
use hgtime::HgTime;

#[cfg(feature = "fb")]
mod fb;

#[cfg(feature = "fb")]
use fb::Repo;

#[cfg(not(feature = "fb"))]
#[derive(Clone, Debug, PartialEq)]
pub enum Repo {
    Unknown,
}

#[cfg(not(feature = "fb"))]
impl FromStr for Repo {
    type Err = Error;
    fn from_str(name: &str) -> Result<Repo> {
        Ok(Repo::Unknown)
    }
}

#[cfg(not(feature = "fb"))]
impl<'a> PartialEq<Repo> for &'a Repo {
    fn eq(&self, other: &Repo) -> bool {
        *self == other
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HgGroup {
    Dev = 1,
    Alpha,
    Beta,
    Stable,
}

impl HgGroup {
    #[allow(dead_code)]
    pub(crate) fn to_str(&self) -> &'static str {
        match self {
            HgGroup::Dev => "hg_dev",
            HgGroup::Alpha => "alpha",
            HgGroup::Beta => "beta",
            HgGroup::Stable => "stable",
        }
    }

    pub(crate) fn from_str(string: &str) -> Result<HgGroup> {
        Ok(match string {
            "hg_dev" => HgGroup::Dev,
            "alpha" => HgGroup::Alpha,
            "beta" => HgGroup::Beta,
            "stable" => HgGroup::Stable,
            _ => bail!("unknown hg group: {}", string),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Platform {
    Centos,
    Fedora,
    OSX,
    Ubuntu,
    Unknown,
    Windows,
}

impl Platform {
    #[allow(dead_code)]
    pub(crate) fn to_str(&self) -> &'static str {
        match self {
            Platform::Centos => "centos",
            Platform::Fedora => "fedora",
            Platform::OSX => "osx",
            Platform::Ubuntu => "ubuntu",
            Platform::Unknown => "unknown",
            Platform::Windows => "windows",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Domain {
    Corp,
    Prod,
}

impl Domain {
    #[allow(dead_code)]
    pub(crate) fn to_str(&self) -> &'static str {
        match self {
            Domain::Corp => "corp",
            Domain::Prod => "prod",
        }
    }
}

pub struct Generator {
    repo_path: PathBuf,
    tiers: HashSet<String>,
    repo: Repo,
    group: HgGroup,
    shard: u8,
    user_shard: u8,
    config: ConfigSet,
    platform: Platform,
    domain: Domain,
    hostname: String,
}

impl Generator {
    pub fn new(repo_name: String, repo_path: PathBuf, user_name: String) -> Result<Self> {
        let repo = Repo::from_str(&repo_name)?;

        let tiers: HashSet<String> = if Path::new("/etc/smc.tiers").exists() {
            fs::read_to_string("/etc/smc.tiers")?
                .split_whitespace()
                .filter(|s| s.len() > 0)
                .map(|s| s.to_string())
                .collect()
        } else {
            HashSet::new()
        };

        let hostname = hostname::get()?
            .to_string_lossy()
            .to_string()
            .to_lowercase();

        let shard = get_shard(&hostname);
        let user_shard = get_shard(&user_name);

        let group = get_hg_group(&tiers, shard);

        let platform = get_platform();

        let domain = if Path::new("/etc/fbwhoami").exists() {
            Domain::Prod
        } else {
            Domain::Corp
        };

        Ok(Generator {
            repo_path,
            tiers,
            repo,
            group,
            shard,
            user_shard,
            config: ConfigSet::new(),
            platform,
            domain,
            hostname,
        })
    }

    pub(crate) fn repo_path(&self) -> &Path {
        self.repo_path.as_ref()
    }

    pub(crate) fn group(&self) -> HgGroup {
        self.group
    }

    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    pub fn in_repos(&self, repos: &[Repo]) -> bool {
        repos.iter().any(|r| r == self.repo)
    }

    #[cfg(test)]
    fn set_inputs(&mut self, tiers: HashSet<String>, group: HgGroup, shard: u8) {
        self.tiers = tiers;
        self.group = group;
        self.shard = shard;
    }

    #[allow(dead_code)]
    pub fn in_tier(&self, tier: impl AsRef<str>) -> bool {
        self.in_tiers(&[tier])
    }

    #[allow(dead_code)]
    pub(crate) fn in_tiers<T: AsRef<str>>(&self, tiers: impl IntoIterator<Item = T>) -> bool {
        for tier in tiers.into_iter() {
            if self.tiers.contains(tier.as_ref()) {
                return true;
            }
        }
        false
    }

    #[allow(dead_code)]
    pub(crate) fn in_group(&self, group: HgGroup) -> bool {
        self.group as u32 <= group as u32
    }

    #[allow(dead_code)]
    pub(crate) fn in_shard(&self, shard: u8) -> bool {
        self.shard < shard
    }

    #[allow(dead_code)]
    pub(crate) fn in_user_shard(&self, shard: u8) -> bool {
        self.user_shard < shard
    }

    #[allow(dead_code)]
    pub(crate) fn in_timeshard(&self, range: Range<HgTime>) -> Result<bool> {
        let now = HgTime::now()
            .ok_or_else(|| anyhow!("invalid HgTime::now()"))?
            .to_utc();
        let start = range.start.to_utc();
        let end = range.end.to_utc();

        let rollout = (end - start).num_seconds() as f64;
        let now = (now - start).num_seconds() as f64;
        let shard_ratio = self.shard as f64 / 100.0;

        Ok(now >= (rollout * shard_ratio))
    }

    #[allow(dead_code)]
    pub(crate) fn platform(&self) -> Platform {
        self.platform
    }

    #[allow(dead_code)]
    pub(crate) fn domain(&self) -> Domain {
        self.domain
    }

    #[allow(dead_code)]
    pub(crate) fn hostname(&self) -> &str {
        &self.hostname
    }

    pub(crate) fn set_config(
        &mut self,
        section: impl AsRef<str>,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
    ) {
        self.config
            .set(section, name, Some(value), &"dynamicconfigs".into())
    }

    #[allow(dead_code)]
    pub(crate) fn load_hgrc(
        &mut self,
        value: impl Into<Text> + Clone + std::fmt::Display,
    ) -> Result<()> {
        let value_copy = value.clone();
        let errors = self.config.parse(value, &"dynamicconfigs".into());
        if !errors.is_empty() {
            bail!(
                "invalid dynamic config blob: '{}'\nerrors: '{:?}'",
                value_copy,
                errors
            );
        }
        Ok(())
    }

    pub fn execute(mut self, canary_remote: Option<String>) -> Result<ConfigSet> {
        if std::env::var("HG_TEST_DYNAMICCONFIG").is_ok() {
            self._execute(test_rules, canary_remote)?;
        } else {
            #[cfg(feature = "fb")]
            self._execute(fb::fb_rules, canary_remote)?;
        }
        Ok(self.config)
    }

    fn _execute(
        &mut self,
        mut rules: impl FnMut(&mut Generator, Option<String>) -> Result<()>,
        canary_remote: Option<String>,
    ) -> Result<()> {
        (rules)(self, canary_remote)
    }
}

fn get_shard(input: &str) -> u8 {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    (hasher.finish() % 100).try_into().unwrap()
}

pub(crate) fn get_platform() -> Platform {
    let os_info = os_info::get();
    use os_info::Type;
    match os_info.os_type() {
        Type::Fedora => Platform::Fedora,
        Type::Macos => Platform::OSX,
        Type::Centos => Platform::Centos,
        Type::Ubuntu => Platform::Ubuntu,
        Type::Windows => Platform::Windows,
        _ => Platform::Unknown,
    }
}

fn get_hg_group(tiers: &HashSet<String>, shard: u8) -> HgGroup {
    let sandcastle = tiers.contains("sandcastle")
        || tiers.contains("sandcastlefog")
        || tiers.contains("sandcastle.releng")
        || tiers.contains("sandcastle.vm.linux");

    // TODO: Support Windows and corp linux alpha
    let mut alpha_file_exists = Path::new("/opt/facebook/.mercurial_alpha").exists();
    if !alpha_file_exists && sandcastle {
        alpha_file_exists = Path::new("/data/sandcastle/staging.marker").exists();
    }

    if tiers.contains("hg_release") {
        HgGroup::Stable
    } else if tiers.contains("hg_dev") {
        HgGroup::Dev
    } else if tiers.contains("hg_alpha")
        || tiers.contains("sandcastle.staging")
        || alpha_file_exists
    {
        HgGroup::Alpha
    } else if shard < 20 && !sandcastle {
        HgGroup::Beta
    } else {
        HgGroup::Stable
    }
}

/// Rules used in our integration test environment
fn test_rules(gen: &mut Generator, _canary_remote: Option<String>) -> Result<()> {
    if let Ok(path) = std::env::var("HG_TEST_DYNAMICCONFIG") {
        let hgrc = std::fs::read_to_string(path).unwrap();
        gen.load_hgrc(hgrc).unwrap();
    }

    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::iter::FromIterator;

    #[test]
    fn test_basic() {
        let repo_name = "test_repo";
        let username = "username";
        let mut generator =
            Generator::new(repo_name.to_string(), PathBuf::new(), username.to_string()).unwrap();

        let tiers = HashSet::from_iter(["in_tier1", "in_tier2"].iter().map(|s| s.to_string()));
        let group = HgGroup::Alpha;
        let shard = 10;
        generator.set_inputs(tiers, group, shard);

        fn test_rules(gen: &mut Generator, _canary_remote: Option<String>) -> Result<()> {
            if gen.in_tiers(&["in_tier1"]) {
                gen.set_config("tier_section", "tier_key", "in_tier1");
            }
            if !gen.in_tiers(&["not_in_tier3"]) {
                gen.set_config("tier_section", "tier_key2", "not_in_tier3");
            }
            if !gen.in_shard(1) {
                gen.set_config("shard_section", "shard_key", "not_in_shard1");
            }
            if gen.in_shard(75) {
                gen.set_config("shard_section", "shard_key2", "in_shard75");
            }
            if !gen.in_user_shard(1) {
                gen.set_config("user_shard_section", "user_shard_key", "not_in_shard1");
            }
            if gen.in_user_shard(80) {
                gen.set_config("user_shard_section", "user_shard_key2", "in_shard80");
            }
            if !gen.in_group(HgGroup::Dev) {
                gen.set_config("group_section", "group_key", "not_in_dev");
            }
            if gen.in_group(HgGroup::Alpha) {
                gen.set_config("group_section", "group_key2", "in_alpha");
            }
            if gen.in_group(HgGroup::Beta) {
                gen.set_config("group_section", "group_key3", "in_beta");
            }
            gen.load_hgrc(
                "[load_hgrc_section]
key=value",
            )
            .unwrap();
            Ok(())
        }

        generator._execute(test_rules, None).unwrap();
        let config_str = generator.config.to_string();

        assert_eq!(
            config_str,
            "[tier_section]
tier_key=in_tier1
tier_key2=not_in_tier3

[shard_section]
shard_key=not_in_shard1
shard_key2=in_shard75

[user_shard_section]
user_shard_key=not_in_shard1
user_shard_key2=in_shard80

[group_section]
group_key=not_in_dev
group_key2=in_alpha
group_key3=in_beta

[load_hgrc_section]
key=value

"
        );
    }
}
