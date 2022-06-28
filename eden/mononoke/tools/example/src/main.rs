/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use mononoke_app::args::MultiRepoArgs;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;

/// Display the repo identity of the chosen repo.
#[derive(Parser)]
struct ExampleArgs {
    #[clap(flatten)]
    repos: MultiRepoArgs,
}

#[facet::container]
struct Repo {
    #[facet]
    repo_identity: RepoIdentity,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let ext = additional::TestAppExtension { default: Some(42) };
    MononokeAppBuilder::new(fb)
        .with_app_extension(ext)
        .build::<ExampleArgs>()?
        .run(async_main)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    let args: ExampleArgs = app.args()?;
    let test_args = app.extension_args::<additional::TestAppExtension>()?;

    let repos: Vec<Repo> = app.open_repos(&args.repos).await?;

    for repo in repos {
        println!(
            "Repo Id: {} Name: {}",
            repo.repo_identity().id(),
            repo.repo_identity().name(),
        );
    }
    if let Some(test_arg) = test_args.test_arg {
        println!("Test arg: {}", test_arg);
    }

    Ok(())
}

/// Example module for hooking into command line arguments that modify the
/// environment.
mod additional {
    use clap::Args;
    use mononoke_app::AppExtension;

    // This struct defines the command line arguments, and is a normal
    // implementor of `clap::Args`.
    #[derive(Args)]
    pub struct TestArgs {
        /// Test argument.
        #[clap(long, help_heading = "TEST OPTIONS")]
        pub test_arg: Option<u32>,
    }

    // This struct defines the extension.  It can be used to store default
    // values and other items necessary for the arguments to operate.
    pub struct TestAppExtension {
        pub default: Option<u32>,
    }

    impl AppExtension for TestAppExtension {
        type Args = TestArgs;

        fn arg_defaults(&self) -> Vec<(&'static str, String)> {
            if let Some(default) = self.default {
                vec![("test-arg", default.to_string())]
            } else {
                Vec::new()
            }
        }
    }
}
