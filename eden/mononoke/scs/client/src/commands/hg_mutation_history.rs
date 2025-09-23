/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use chrono::DateTime;
use chrono::FixedOffset;
use commit_id_types::CommitIdArgs;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::map_commit_id;
use crate::args::commit_id::resolve_commit_id;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::library::commit_id::render_commit_id;
use crate::library::datetime;
use crate::render::Render;

#[derive(clap::Parser)]
/// Find hg mutation history for a public commit by traversing mappings
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    /// Format for the output
    #[clap(long, value_enum, default_value = "commit-id")]
    format: MutationHistoryFormat,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum MutationHistoryFormat {
    /// Show only commit IDs
    #[clap(name = "commit-id")]
    CommitId,
    /// Show full hg mutation information
    #[clap(name = "hg-mutation")]
    HgMutation,
}

impl From<MutationHistoryFormat> for thrift::MutationHistoryFormat {
    fn from(format: MutationHistoryFormat) -> Self {
        match format {
            MutationHistoryFormat::CommitId => thrift::MutationHistoryFormat::COMMIT_ID,
            MutationHistoryFormat::HgMutation => thrift::MutationHistoryFormat::HG_MUTATION,
        }
    }
}

#[derive(Serialize)]
struct CommitLookupOutput {
    #[serde(skip)]
    requested: String,
    exists: bool,
    ids: BTreeSet<String>,
}

impl Render for CommitLookupOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if self.exists {
            let commit_ids = self
                .ids
                .iter()
                .map(|id| ("hg".to_string(), id.clone()))
                .collect::<BTreeMap<_, _>>();
            let schemes = HashSet::from_iter(["hg".to_string()]);
            render_commit_id(None, "\n", &self.requested, &commit_ids, &schemes, w)?;
            write!(w, "\n")?;
        } else {
            bail!("{} does not exist\n", self.requested,);
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

#[derive(Serialize)]
struct HgMutationLookupOutput {
    commit_lookups: Vec<CommitLookupOutput>,
}

impl Render for HgMutationLookupOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        for (i, commit) in self.commit_lookups.iter().enumerate() {
            if i > 0 {
                write!(w, "--\n")?;
            }
            commit.render(args, w)?;
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

#[derive(Serialize)]
struct HgMutationOutput {
    successor: String,
    predecessors: Vec<String>,
    split: Vec<String>,
    op: String,
    user: String,
    timestamp: i64,
    tz: i32,
    date: DateTime<FixedOffset>,
}

impl HgMutationOutput {
    fn from_thrift(mutation: &thrift::HgMutation) -> Result<Self> {
        let successor = map_commit_id(&mutation.successor)
            .map(|(_, id)| id)
            .ok_or_else(|| anyhow::anyhow!("Invalid successor commit ID"))?;

        let predecessors = mutation
            .predecessors
            .iter()
            .filter_map(|commit_id| map_commit_id(commit_id).map(|(_, id)| id))
            .collect();

        let split = mutation
            .split
            .iter()
            .filter_map(|commit_id| map_commit_id(commit_id).map(|(_, id)| id))
            .collect();

        Ok(HgMutationOutput {
            successor,
            predecessors,
            split,
            op: mutation.op.clone(),
            user: mutation.user.clone(),
            timestamp: mutation.date.timestamp,
            tz: mutation.date.tz,
            date: datetime(&mutation.date),
        })
    }
}

impl Render for HgMutationOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        writeln!(w, "Successor: {}", self.successor)?;
        writeln!(w, "Predecessors: {}", self.predecessors.join(", "))?;
        if !self.split.is_empty() {
            writeln!(w, "Split: {}", self.split.join(", "))?;
        }
        writeln!(w, "Operation: {}", self.op)?;
        writeln!(w, "User: {}", self.user)?;
        writeln!(w, "Date: {}", self.date)?;

        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

#[derive(Serialize)]
struct HgMutationListOutput {
    mutations: Vec<HgMutationOutput>,
}

impl Render for HgMutationListOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        for (i, mutation) in self.mutations.iter().enumerate() {
            if i > 0 {
                writeln!(w)?;
            }
            mutation.render(args, w)?;
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let conn = app.get_connection(Some(&repo.name))?;
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    };
    let hg_mutation_history = conn
        .commit_hg_mutation_history(
            &commit,
            &thrift::CommitHgMutationHistoryParams {
                format: args.format.clone().into(),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.handle_selection_error(&commit.repo))?;

    match hg_mutation_history.hg_mutation_history {
        thrift::HgMutationHistory::commit_ids(commit_ids) => {
            let commit_lookups: Vec<_> = commit_ids
                .into_iter()
                .filter_map(|commit_id| map_commit_id(&commit_id))
                .map(|(_, id)| CommitLookupOutput {
                    requested: id.clone(),
                    exists: true,
                    ids: BTreeSet::from_iter([id]),
                })
                .collect();
            let output = HgMutationLookupOutput { commit_lookups };
            app.target.render_one(&args, output).await
        }
        thrift::HgMutationHistory::hg_mutations(hg_mutations) => {
            // Sort the mutations by topological order for stable output
            let mut mutations = BTreeMap::new();
            let mut mutation_dag = BTreeMap::new();
            for hg_mutation in hg_mutations {
                let mutation = HgMutationOutput::from_thrift(&hg_mutation)?;
                mutation_dag.insert(mutation.successor.clone(), mutation.predecessors.clone());
                mutations.insert(mutation.successor.clone(), mutation);
            }
            let mutation_order =
                topo_sort::sort_topological(&mutation_dag).context("No topological order found")?;
            let mutations = mutation_order
                .iter()
                .flat_map(|id| mutations.remove(id.as_str()))
                .collect();
            let output = HgMutationListOutput { mutations };
            app.target.render_one(&args, output).await
        }
        thrift::HgMutationHistory::UnknownField(_) => bail!("Unknown field"),
    }
}
