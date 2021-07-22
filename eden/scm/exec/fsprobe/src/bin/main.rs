/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    path: PathBuf,
}

fn main() {
    let args = Cli::from_args();
    let plan = ProbePlan::load(&args.path).expect("Failed to load fsprobe plan");
    println!("Loaded {} actions", plan.0.len());
}

struct ProbePlan(Vec<ProbeAction>);

enum ProbeAction {
    Read(PathBuf),
}

// Probe plan file format is a new line separated list of actions
// Each action has a format <action> [<params>]
// Currently supported actions:
//   * cat <path> - read full file at <path>
impl ProbePlan {
    fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mut actions = vec![];
        for line in BufReader::new(file).lines() {
            let line = line?;
            let action = ProbeAction::parse(&line)?;
            actions.push(action);
        }
        Ok(Self(actions))
    }
}

impl ProbeAction {
    pub fn parse(s: &str) -> Result<Self> {
        let space = s.find(' ');
        if let Some(space) = space {
            let cmd = &s[..space];
            match cmd {
                "cat" => {
                    let path = &s[space..];
                    if path.len() == 0 {
                        bail!("cat requires path");
                    }
                    Ok(ProbeAction::Read(path.into()))
                }
                _ => bail!("Unknown command {}", cmd),
            }
        } else {
            bail!("Invalid action {}", s);
        }
    }
}
