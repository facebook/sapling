/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_process_traits::Command;
use async_process_traits::CommandSpawner;
use async_process_traits::ExitStatus;
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
    get_commit_timestamp_impl(&TokioCommandSpawner, commit_id).await
}

async fn get_commit_timestamp_impl<Spawner>(spawner: &Spawner, commit_id: &str) -> Result<u64>
where
    Spawner: CommandSpawner,
{
    let mut command = Spawner::Command::new(get_sapling_executable_path());
    command.envs(get_sapling_options()).args([
        "log",
        "--traceback",
        "-T",
        "{date}",
        "-r",
        commit_id,
    ]);

    let output = spawner.output(&mut command).await?;
    let date = String::from_utf8(output.stdout().to_vec())?;
    let date = date.parse::<f64>()?;
    // NOTE: Sapling returns fractional seconds, but we only want whole seconds.
    //       Truncate to the nearest second.
    Ok(date as u64)
}

pub async fn is_commit_in_repo(commit_id: &str) -> Result<bool> {
    is_commit_in_repo_impl(&TokioCommandSpawner, commit_id).await
}

async fn is_commit_in_repo_impl<Spawner>(spawner: &Spawner, commit_id: &str) -> Result<bool>
where
    Spawner: CommandSpawner,
{
    let mut command = Spawner::Command::new(get_sapling_executable_path());
    command.envs(get_sapling_options()).args([
        "log",
        "--traceback",
        "-r",
        commit_id,
        // Disable fbsource <-> www sync during lookup
        "--config",
        "megarepo.transparent-lookup=",
    ]);

    let output = spawner.output(&mut command).await?;
    Ok(output.status().success())
}

#[cfg(test)]
mod tests {
    use crate::commit::*;
    use crate::utils::tests::get_mock_spawner;

    const COMMIT_ID: &str = "0000111122223333444455556666777788889999";

    #[tokio::test]
    pub async fn test_get_current_commit_id() -> Result<()> {
        let spawner = get_mock_spawner(
            get_sapling_executable_path(),
            Some((0, Some(COMMIT_ID.as_bytes().to_vec()))),
        );
        let commit_id = get_current_commit_id_impl(&spawner).await?;
        assert_eq!(commit_id, COMMIT_ID);
        Ok(())
    }

    #[tokio::test]
    pub async fn test_is_commit_in_repo() -> Result<()> {
        let spawner = get_mock_spawner(get_sapling_executable_path(), Some((0, None)));
        assert!(is_commit_in_repo_impl(&spawner, COMMIT_ID).await?);

        let spawner = get_mock_spawner(get_sapling_executable_path(), Some((255, None)));
        assert!(!is_commit_in_repo_impl(&spawner, COMMIT_ID).await?,);

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
        let timestamp_str = "1738089317.028800".to_owned();
        let spawner = get_mock_spawner(
            get_sapling_executable_path(),
            Some((0, Some(timestamp_str.as_bytes().to_vec()))),
        );
        let timestamp = get_commit_timestamp_impl(&spawner, COMMIT_ID).await?;
        assert_eq!(timestamp, 1738089317);
        Ok(())
    }
}
