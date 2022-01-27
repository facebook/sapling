/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use mononoke_app::args::RepoArgs;
use mononoke_app::{MononokeApp, MononokeAppBuilder};
use repo_identity::{RepoIdentity, RepoIdentityRef};

/// Display the repo identity of the chosen repo.
#[derive(Parser)]
struct ExampleArgs {
    #[clap(flatten)]
    repo: RepoArgs,
}

#[facet::container]
struct Repo {
    #[facet]
    repo_identity: RepoIdentity,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let ext = additional::TestArgExtension { default: Some(42) };
    MononokeAppBuilder::new(fb)
        .with_arg_extension(ext)
        .build::<ExampleArgs>()?
        .run(async_main)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    let args: ExampleArgs = app.args()?;

    let repo: Repo = app.open_repo(&args.repo).await?;

    println!(
        "Repo Id: {} Name: {}",
        repo.repo_identity().id(),
        repo.repo_identity().name(),
    );

    Ok(())
}

/// Example module for hooking into command line arguments that modify the
/// environment.
mod additional {
    use anyhow::Result;
    use clap::Args;
    use environment::MononokeEnvironment;
    use mononoke_app::ArgExtension;

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
    pub struct TestArgExtension {
        pub default: Option<u32>,
    }

    impl ArgExtension for TestArgExtension {
        type Args = TestArgs;

        fn arg_defaults(&self) -> Vec<(&'static str, String)> {
            if let Some(default) = self.default {
                vec![("test-arg", default.to_string())]
            } else {
                Vec::new()
            }
        }

        fn process_args(&self, args: &TestArgs, _env: &mut MononokeEnvironment) -> Result<()> {
            if let Some(value) = args.test_arg {
                println!("Test arg received: {}", value);
            }
            Ok(())
        }
    }
}
