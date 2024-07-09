/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_recursion::async_recursion;
use clap::Args;
use context::CoreContext;
use megarepo_config::CfgrMononokeMegarepoConfigs;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::SyncTargetConfig;
use mononoke_api::RepositoryId;
use mononoke_app::MononokeApp;
use slog::info;
use slog::warn;

const CONFIGERATOR_SOURCE: &str = "materialized_configs";
const CONFIGERATOR_PATH: &str = "scm/mononoke/megarepo/configstore";
const CONFIGERATOR_SUFFIX: &str = "materialized_JSON";

#[derive(Args)]
pub(super) struct ImportArgs {
    #[clap(long, value_name = "DIR")]
    configerator_dir: Option<PathBuf>,
}

pub(super) async fn import(ctx: &CoreContext, app: MononokeApp, args: ImportArgs) -> Result<()> {
    #[allow(deprecated)]
    let configerator_dir = args
        .configerator_dir
        .unwrap_or_else(|| env::home_dir().unwrap().join("configerator"));
    let configerator_dir = configerator_dir
        .join(CONFIGERATOR_SOURCE)
        .join(CONFIGERATOR_PATH);
    info!(
        ctx.logger(),
        "importing all configs from {}",
        configerator_dir.display()
    );

    let configs = find_all_configs(&configerator_dir).await?;
    for config in configs {
        match import_config(ctx, &app, &config).await {
            Ok(_) => {}
            Err(e) => {
                warn!(ctx.logger(), "importing {:?}: {:?}", config, e);
            }
        }
    }

    Ok(())
}

#[async_recursion]
async fn find_all_configs(dir: &Path) -> Result<Vec<PathBuf>>
where
{
    let mut ret = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let mut r = find_all_configs(&path).await?;
                ret.append(&mut r);
            } else {
                let metadata = fs::metadata(&path)?;
                if metadata.is_file()
                    && path.extension().unwrap().to_str().unwrap() == CONFIGERATOR_SUFFIX
                {
                    ret.push(path);
                }
            }
        }
    }
    Ok(ret)
}

// async closure are unstable, using a regular function instead
async fn import_config(ctx: &CoreContext, app: &MononokeApp, path: &PathBuf) -> Result<()> {
    let mut file = File::open(path)?;
    let mut buffer = vec![];
    file.read_to_end(&mut buffer)?;
    let config: SyncTargetConfig = fbthrift::simplejson_protocol::deserialize(buffer)?;
    info!(
        ctx.logger(),
        "path {} target {} {}",
        path.display(),
        config.target.repo_id,
        config.target.bookmark,
    );

    let repo_id = RepositoryId::new(config.target.repo_id.try_into().unwrap());
    let repo_configs = app.repo_configs();
    let (_, repo_config) = repo_configs
        .get_repo_config(repo_id)
        .ok_or_else(|| anyhow!("unknown repoid: {:?}", repo_id))?;

    let env = app.environment();
    let megarepo_cfg = CfgrMononokeMegarepoConfigs::new(
        ctx.fb,
        ctx.logger(),
        env.mysql_options.clone(),
        env.readonly_storage,
        None,
    )
    .await
    .context("loading megarepo config")?;

    megarepo_cfg
        .add_config_version(ctx.clone(), Arc::new(repo_config.clone()), config)
        .await
        .context("importing config")?;

    Ok(())
}
