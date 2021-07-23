/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Result};
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    path: PathBuf,
}

fn main() {
    let args = Cli::from_args();
    let plan = ProbePlan::load(&args.path).expect("Failed to load fsprobe plan");
    let mut stats = Stats::default();
    let start = Instant::now();
    plan.run(&mut stats);
    let duration = Instant::now() - start;
    let rate = rate(stats.bytes as f64 / (duration.as_millis() as f64 / 1000.));
    println!("{:?}: {}, {}", duration, stats, rate);
}

#[derive(Default)]
struct Stats {
    files: u64,
    bytes: u64,
    errors: u64,
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

    pub fn run(self, stats: &mut Stats) {
        for action in self.0 {
            action.run(stats);
        }
    }
}

impl ProbeAction {
    pub fn parse(s: &str) -> Result<Self> {
        let space = s.find(' ');
        if let Some(space) = space {
            let cmd = &s[..space];
            match cmd {
                "cat" => {
                    let path = &s[space + 1..];
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

    pub fn run(&self, stats: &mut Stats) {
        let r = match self {
            Self::Read(path) => Self::read(path, stats),
        };
        if let Err(err) = r {
            stats.errors += 1;
            eprintln!("{} failed: {}", self, err);
        }
    }

    fn read(path: &Path, stats: &mut Stats) -> Result<()> {
        let mut file = File::open(path)?;
        let mut v = vec![];
        file.read_to_end(&mut v)?;
        stats.bytes += v.len() as u64;
        stats.files += 1;
        Ok(())
    }
}

impl fmt::Display for ProbeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(path) => write!(f, "cat {}", path.display()),
        }
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} files, {} bytes, {} errors",
            self.files, self.bytes, self.errors
        )
    }
}

fn rate(rate: f64) -> String {
    // Guard against zero, NaN, infinity, etc.
    if !rate.is_normal() {
        return "0 b/s".into();
    }

    // Divide by the base-1000 log of the value to bring it under 1000.
    let log = (rate.log10() / 3.0).floor() as usize;
    let shifted = rate / 1000f64.powi(log as i32);

    // Determine unit and precision to display.
    let unit = ["b/s", "kb/s", "Mb/s", "Gb/s", "Tb/s", "Pb/s", "Eb/s"][log];
    let prec = if log > 1 { 2 } else { 0 };

    format!("{:.*} {}", prec, shifted, unit)
}
