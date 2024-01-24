/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Error;
use anyhow::Result;
use fbinit::FacebookInit;
use git_symbolic_refs::GitSymbolicRefs;
use git_symbolic_refs::GitSymbolicRefsEntry;
use git_symbolic_refs::SqlGitSymbolicRefsBuilder;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql_construct::SqlConstruct;

#[test]
fn test_symref_entry_creation() -> Result<()> {
    let symref_name = "HEAD".to_string();
    let ref_name = "master".to_string();
    let ref_type = "branch".to_string();
    let entry = GitSymbolicRefsEntry::new(symref_name, ref_name, ref_type);
    // Validate the symref entry gets created successfully
    assert!(entry.is_ok());
    let entry = entry.unwrap();
    // Creating an entry with invalid ref type should fail
    let entry = GitSymbolicRefsEntry::new(
        entry.symref_name,
        entry.ref_name,
        "invalid_ref_type".to_string(),
    );
    assert!(entry.is_err());
    Ok(())
}

#[fbinit::test]
async fn test_add_and_get(_: FacebookInit) -> Result<(), Error> {
    let symrefs = SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let symref_name = "HEAD";
    let ref_name = "master";
    let ref_type = "branch";
    let entry = GitSymbolicRefsEntry::new(
        symref_name.to_string(),
        ref_name.to_string(),
        ref_type.to_string(),
    )?;
    symrefs.add_or_update_entries(vec![entry.clone()]).await?;

    let result = symrefs.get_ref_by_symref(entry.symref_name.clone()).await?;
    assert_eq!(result, Some(entry.clone()));

    let result = symrefs
        .get_symrefs_by_ref(entry.ref_name.clone(), entry.ref_type.clone())
        .await?;
    assert_eq!(result, Some(vec![entry.symref_name]));
    Ok(())
}

#[fbinit::test]
async fn test_add_and_delete(_: FacebookInit) -> Result<(), Error> {
    let symrefs = SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let symref_name = "HEAD";
    let ref_name = "master";
    let ref_type = "branch";
    let entry = GitSymbolicRefsEntry::new(
        symref_name.to_string(),
        ref_name.to_string(),
        ref_type.to_string(),
    )?;
    symrefs.add_or_update_entries(vec![entry.clone()]).await?;

    let result = symrefs.get_ref_by_symref(entry.symref_name.clone()).await?;
    assert_eq!(result, Some(entry.clone()));

    symrefs
        .delete_symrefs(vec![entry.symref_name.clone()])
        .await?;
    let result = symrefs.get_ref_by_symref(entry.symref_name.clone()).await?;
    assert_eq!(result, None);
    Ok(())
}

#[fbinit::test]
async fn test_update_and_get(_: FacebookInit) -> Result<(), Error> {
    let symrefs = SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let symref_name = "HEAD";
    let ref_name = "master";
    let ref_type = "branch";
    let entry = GitSymbolicRefsEntry::new(
        symref_name.to_string(),
        ref_name.to_string(),
        ref_type.to_string(),
    )?;
    symrefs.add_or_update_entries(vec![entry.clone()]).await?;

    let result = symrefs.get_ref_by_symref(entry.symref_name.clone()).await?;
    assert_eq!(result, Some(entry.clone()));

    let new_ref_name = "main";
    let entry = GitSymbolicRefsEntry::new(
        symref_name.to_string(),
        new_ref_name.to_string(),
        ref_type.to_string(),
    )?;
    symrefs.add_or_update_entries(vec![entry.clone()]).await?;

    let result = symrefs.get_ref_by_symref(entry.symref_name.clone()).await?;
    assert_eq!(result, Some(entry.clone()));
    Ok(())
}

#[fbinit::test]
async fn test_get_without_add(_: FacebookInit) -> Result<(), Error> {
    let symrefs = SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let result = symrefs.get_ref_by_symref("HEAD".to_string()).await?;
    assert_eq!(result, None);

    let result = symrefs
        .get_symrefs_by_ref("master".to_string(), "branch".try_into()?)
        .await?;
    assert_eq!(result, None);
    Ok(())
}

#[fbinit::test]
async fn test_get_multiple(_: FacebookInit) -> Result<(), Error> {
    let symrefs = SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let entry = GitSymbolicRefsEntry::new(
        "HEAD".to_string(),
        "master".to_string(),
        "branch".to_string(),
    )?;
    let tag_entry = GitSymbolicRefsEntry::new(
        "TAG_HEAD".to_string(),
        "master".to_string(),
        "tag".to_string(),
    )?;
    let adhoc_entry =
        GitSymbolicRefsEntry::new("ADHOC".to_string(), "master".to_string(), "tag".to_string())?;
    symrefs
        .add_or_update_entries(vec![entry, tag_entry, adhoc_entry])
        .await?;

    let result = symrefs
        .get_symrefs_by_ref("master".to_string(), "tag".try_into()?)
        .await?
        .expect("None symrefs returned for the input ref name and type");
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from(["ADHOC".to_string(), "TAG_HEAD".to_string()])
    );

    let result = symrefs
        .get_symrefs_by_ref("master".to_string(), "branch".try_into()?)
        .await?
        .expect("None symrefs returned for the input ref name and type");
    assert_eq!(
        HashSet::from_iter(result),
        HashSet::from(["HEAD".to_string()])
    );
    Ok(())
}

#[fbinit::test]
async fn test_list_all(_: FacebookInit) -> Result<(), Error> {
    let symrefs = SqlGitSymbolicRefsBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let entry = GitSymbolicRefsEntry::new(
        "HEAD".to_string(),
        "master".to_string(),
        "branch".to_string(),
    )?;
    let tag_entry = GitSymbolicRefsEntry::new(
        "TAG_HEAD".to_string(),
        "master".to_string(),
        "tag".to_string(),
    )?;
    let adhoc_entry =
        GitSymbolicRefsEntry::new("ADHOC".to_string(), "master".to_string(), "tag".to_string())?;
    symrefs
        .add_or_update_entries(vec![entry.clone(), tag_entry.clone(), adhoc_entry.clone()])
        .await?;

    let result: HashSet<GitSymbolicRefsEntry> =
        symrefs.list_all_symrefs().await?.into_iter().collect();
    assert_eq!(
        result,
        HashSet::from_iter(vec![entry.clone(), tag_entry.clone(), adhoc_entry.clone()].into_iter())
    );
    Ok(())
}
