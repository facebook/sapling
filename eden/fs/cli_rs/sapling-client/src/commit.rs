/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_process_traits::Command;
use async_process_traits::CommandSpawner;
use async_process_traits::Output;
use async_process_traits::TokioCommandSpawner;

use crate::error::Result;
use crate::utils::get_sapling_executable_path;
use crate::utils::get_sapling_options;

pub async fn get_current_commit_id() -> Result<String> {
    get_current_commit_id_impl(&TokioCommandSpawner).await
}

async fn get_current_commit_id_impl<Spawner>(spawner: &Spawner) -> Result<String>
where
    Spawner: CommandSpawner,
{
    let mut command = Spawner::Command::new(get_sapling_executable_path());
    command
        .envs(get_sapling_options())
        .args(["whereami", "--traceback"]);
    let output = spawner.output(&mut command).await?;

    Ok(String::from_utf8(output.stdout().to_vec())?)
}

pub async fn get_commit_timestamp(commit_id: &str) -> Result<u64> {
    let output = tokio::process::Command::new(get_sapling_executable_path())
        .envs(get_sapling_options())
        .args(["log", "--traceback", "-T", "{date}", "-r", commit_id])
        .output()
        .await?;
    let date = String::from_utf8(output.stdout)?;
    let date = date.parse::<f64>()?;
    // NOTE: Sapling returns fractional seconds, but we only want whole seconds.
    //       Truncate to the nearest second.
    Ok(date as u64)
}

pub async fn is_commit_in_repo(commit_id: &str) -> Result<bool> {
    let output = tokio::process::Command::new(get_sapling_executable_path())
        .envs(get_sapling_options())
        .args([
            "log",
            "--traceback",
            "-r",
            commit_id,
            // Disable fbsource <-> www sync during lookup
            "--config",
            "megarepo.transparent-lookup=",
        ])
        .output()
        .await?;
    Ok(output.status.success())
}

#[cfg(test)]
mod tests {
    use edenfs_client::utils::get_mount_point;

    use crate::commit::*;
    use crate::utils::tests::get_mock_spawner;
    use crate::utils::*;

    const FBSOURCE_COMMIT_ID: &str = "5496dd87e5fe7430a1a399530cc339a479097524";
    const WWW_COMMIT_ID: &str = "1061662d6db2072dd30308d1626a45ac11db3467";

    #[tokio::test]
    pub async fn test_current_commit_id() -> Result<()> {
        let hash = "0000111122223333444455556666777788889999".to_owned();
        let spawner = get_mock_spawner(
            get_sapling_executable_path(),
            Some((0, Some(hash.as_bytes().to_vec()))),
        );
        let commit_id = get_current_commit_id_impl(&spawner).await?;
        assert_eq!(commit_id, hash);
        Ok(())
    }

    #[tokio::test]
    pub async fn test_is_commit_in_repo() -> Result<()> {
        let mount_point = get_mount_point(&None)?;
        let commit_id = get_current_commit_id().await?;
        assert!(is_commit_in_repo(&commit_id).await?);
        assert_eq!(
            is_commit_in_repo(FBSOURCE_COMMIT_ID).await?,
            is_fbsource_checkout(&mount_point)
        );
        assert_eq!(
            is_commit_in_repo(WWW_COMMIT_ID).await?,
            !is_fbsource_checkout(&mount_point)
        );

        Ok(())
    }

    #[tokio::test]
    pub async fn test_get_commit_timestamp() -> Result<()> {
        // sl log of commit in fbsource:
        //   changeset:   5496dd87e5fe7430a1a399530cc339a479097524  D68746950
        //   user:        John Elliott <jdelliot@fb.com>
        //   date:        Tue, 28 Jan 2025 10:35:17 -0800
        //   summary:     [meerkat] Improve saved state support
        // timestamp should be 1738089317.028800, but we truncate to the nearest second
        let timestamp = get_commit_timestamp(FBSOURCE_COMMIT_ID).await?;
        assert_eq!(timestamp, 1738089317);
        Ok(())
    }
}
