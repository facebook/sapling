/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use commit_id_types::CommitIdArgs;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::map_commit_id;
use crate::args::commit_id::resolve_commit_id;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::render::Render;

#[derive(clap::Parser)]
/// Show git mutation history (predecessor tracking) for a commit
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
}

#[derive(Serialize)]
struct GitMutationOutput {
    successor: String,
    predecessors: Vec<String>,
    op: String,
}

impl GitMutationOutput {
    fn from_thrift(mutation: &thrift::GitMutation) -> Result<Self> {
        let successor = map_commit_id(&mutation.successor)
            .map(|(_, id)| id)
            .ok_or_else(|| anyhow::anyhow!("Invalid successor commit ID"))?;

        let predecessors = mutation
            .predecessors
            .iter()
            .filter_map(|commit_id| map_commit_id(commit_id).map(|(_, id)| id))
            .collect();

        Ok(GitMutationOutput {
            successor,
            predecessors,
            op: mutation.op.clone(),
        })
    }
}

impl Render for GitMutationOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        writeln!(w, "Successor: {}", self.successor)?;
        writeln!(w, "Predecessors: {}", self.predecessors.join(", "))?;
        writeln!(w, "Operation: {}", self.op)?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

#[derive(Serialize)]
struct GitMutationListOutput {
    mutations: Vec<GitMutationOutput>,
}

impl Render for GitMutationListOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if self.mutations.is_empty() {
            writeln!(w, "No git mutation history found")?;
        } else {
            for (i, mutation) in self.mutations.iter().enumerate() {
                if i > 0 {
                    writeln!(w)?;
                }
                mutation.render(args, w)?;
            }
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
    let conn = app.get_connection(Some(&repo.name)).await?;
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    };
    let response = conn
        .commit_git_mutation_history(
            &commit,
            &thrift::CommitGitMutationHistoryParams {
                format: thrift::MutationHistoryFormat::GIT_MUTATION,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.handle_selection_error(&commit.repo))?;

    match response.git_mutation_history {
        thrift::GitMutationHistory::git_mutations(git_mutations) => {
            // Sort by topological order for stable output, matching
            // the hg_mutation_history command's behavior.
            let mut mutations = BTreeMap::new();
            let mut mutation_dag = BTreeMap::new();
            for git_mutation in &git_mutations {
                let mutation = GitMutationOutput::from_thrift(git_mutation)?;
                mutation_dag.insert(mutation.successor.clone(), mutation.predecessors.clone());
                mutations.insert(mutation.successor.clone(), mutation);
            }
            // Pin T and V explicitly: on Apple targets, leaving V to be inferred
            // makes the solver probe `&V: IntoIterator` while V is still a
            // variable, recursing into objc2's `impl IntoIterator for
            // &Retained<T>` until overflow (E0275).
            let mutation_order =
                topo_sort::sort_topological::<String, _, Vec<String>>(&mutation_dag)
                    .context("No topological order found")?;
            let mutations = mutation_order
                .iter()
                .flat_map(|id| mutations.remove(id.as_str()))
                .collect();
            let output = GitMutationListOutput { mutations };
            app.target.render_one(&args, output).await
        }
        _ => bail!("Unexpected response format from commit_git_mutation_history"),
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_mutation_output_json_serialization() {
        let output = GitMutationOutput {
            successor: "abc123".to_string(),
            predecessors: vec!["def456".to_string()],
            op: "amend".to_string(),
        };
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["successor"], "abc123");
        assert_eq!(json["predecessors"][0], "def456");
        assert_eq!(json["op"], "amend");
    }

    #[mononoke::test]
    fn test_mutation_output_multiple_predecessors() {
        let output = GitMutationOutput {
            successor: "abc".to_string(),
            predecessors: vec!["p1".to_string(), "p2".to_string(), "p3".to_string()],
            op: "fold".to_string(),
        };
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["predecessors"].as_array().unwrap().len(), 3);
        assert_eq!(json["op"], "fold");
    }

    #[mononoke::test]
    fn test_mutation_list_json_serialization() {
        let output = GitMutationListOutput {
            mutations: vec![
                GitMutationOutput {
                    successor: "aaa".to_string(),
                    predecessors: vec!["bbb".to_string()],
                    op: "amend".to_string(),
                },
                GitMutationOutput {
                    successor: "ccc".to_string(),
                    predecessors: vec!["ddd".to_string()],
                    op: "rebase".to_string(),
                },
            ],
        };
        let json = serde_json::to_value(&output).unwrap();
        let mutations = json["mutations"].as_array().unwrap();
        assert_eq!(mutations.len(), 2);
        assert_eq!(mutations[0]["op"], "amend");
        assert_eq!(mutations[1]["op"], "rebase");
    }

    #[mononoke::test]
    fn test_from_thrift_valid() {
        let mutation = thrift::GitMutation {
            successor: thrift::CommitId::git(vec![0xab; 20]),
            predecessors: vec![thrift::CommitId::git(vec![0xcd; 20])],
            op: "amend".to_string(),
            ..Default::default()
        };
        let output = GitMutationOutput::from_thrift(&mutation).unwrap();
        assert_eq!(output.op, "amend");
        assert_eq!(output.predecessors.len(), 1);
        assert!(!output.successor.is_empty());
    }
}
