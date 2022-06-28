/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use clap::App;
use clap::AppSettings;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream::StreamExt;
use futures::stream::{self};
use maplit::btreeset;
use serde_derive::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::add_multiple_commit_id_args;
use crate::args::commit_id::add_scheme_args;
use crate::args::commit_id::get_commit_ids;
use crate::args::commit_id::get_request_schemes;
use crate::args::commit_id::get_schemes;
use crate::args::commit_id::map_commit_id;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::pushvars::add_pushvar_args;
use crate::args::pushvars::get_pushvars;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::args::service_id::add_service_id_args;
use crate::args::service_id::get_service_id;
use crate::connection::Connection;
use crate::lib::commit_id::render_commit_id;
use crate::render::Render;
use crate::render::RenderStream;

pub(super) const NAME: &str = "land-stack";

const ARG_NAME: &str = "BOOKMARK_NAME";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Land a stack of commits")
        .long_about(concat!(
            "Land a stack of commits\n\n",
            "Provide two commits: the first is the head of a stack, and the second is ",
            "public commit the stack is based on.  The stack of commits between these ",
            "two commits will be landed onto the named bookmark via pushrebase.",
        ))
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_scheme_args(cmd);
    let cmd = add_multiple_commit_id_args(cmd);
    let cmd = add_service_id_args(cmd);
    let cmd = add_pushvar_args(cmd);
    cmd.arg(
        Arg::with_name(ARG_NAME)
            .short("n")
            .long("name")
            .takes_value(true)
            .help("Name of the bookmark to land to")
            .required(true),
    )
}

#[derive(Serialize)]
struct PushrebaseRebasedCommit {
    old_bonsai_id: String,
    new_ids: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct PushrebaseOutcomeOutput {
    bookmark: String,
    head: BTreeMap<String, String>,
    rebased_commits: Vec<PushrebaseRebasedCommit>,
    retries: i64,
    distance: i64,
}

impl Render for PushrebaseOutcomeOutput {
    fn render(&self, matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        let schemes = get_schemes(matches);
        write!(
            w,
            "In {} retries across distance {}\n",
            self.retries, self.distance
        )?;
        write!(w, "{} updated to", self.bookmark)?;
        render_commit_id(
            Some(("", "    ")),
            "\n",
            &self.bookmark,
            &self.head,
            &schemes,
            w,
        )?;
        write!(w, "\n")?;
        for rebase in self.rebased_commits.iter() {
            write!(w, "{} => ", rebase.old_bonsai_id)?;
            render_commit_id(None, ", ", "new commit", &rebase.new_ids, &schemes, w)?;
            write!(w, "\n")?;
        }
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(matches: &ArgMatches<'_>, connection: Connection) -> Result<RenderStream> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_ids = get_commit_ids(matches)?;
    if commit_ids.len() != 2 {
        bail!("expected 2 commit_ids (got {})", commit_ids.len())
    }
    let ids = resolve_commit_ids(&connection, &repo, &commit_ids).await?;
    let bookmark: String = matches.value_of(ARG_NAME).expect("name is required").into();
    let service_identity = get_service_id(matches).map(String::from);
    let pushvars = get_pushvars(&matches)?;

    let (head, base) = match ids.as_slice() {
        [head_id, base_id] => (head_id.clone(), base_id.clone()),
        _ => bail!("expected 1 or 2 commit_ids (got {})", ids.len()),
    };

    let params = thrift::RepoLandStackParams {
        bookmark: bookmark.clone(),
        head,
        base,
        identity_schemes: get_request_schemes(&matches),
        old_identity_schemes: Some(btreeset! { thrift::CommitIdentityScheme::BONSAI }),
        service_identity,
        pushvars,
        ..Default::default()
    };
    let outcome = connection
        .repo_land_stack(&repo, &params)
        .await?
        .pushrebase_outcome;
    let head = map_commit_ids(outcome.head.values());
    let mut rebased_commits = outcome
        .rebased_commits
        .into_iter()
        .map(|rebase| {
            let (_, old_bonsai_id) = map_commit_id(
                rebase
                    .old_ids
                    .get(&thrift::CommitIdentityScheme::BONSAI)
                    .ok_or_else(|| anyhow!("bonsai id missing from response"))?,
            )
            .ok_or_else(|| anyhow!("bonsai id should be mappable"))?;
            let new_ids = map_commit_ids(rebase.new_ids.values());
            Ok(PushrebaseRebasedCommit {
                old_bonsai_id,
                new_ids,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    rebased_commits.sort_unstable_by(|a, b| a.old_bonsai_id.cmp(&b.old_bonsai_id));
    let output = Box::new(PushrebaseOutcomeOutput {
        bookmark,
        head,
        rebased_commits,
        distance: outcome.pushrebase_distance,
        retries: outcome.retry_num,
    });
    Ok(stream::once(async move { Ok(output as Box<dyn Render>) }).boxed())
}
