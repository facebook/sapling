/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;
use format_util::commit_text_to_fields;
use format_util::CommitFields as NativeCommitFields;
use format_util::GitCommitFields;
use format_util::HgCommitFields;
use format_util::HgTime;
use minibytes::Text;
use storemodel::SerializationFormat;
use types::Id20;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "formatutil"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "hg_commit_fields_to_text",
        py_fn!(py, hg_commit_fields_to_text(fields: Serde<HgCommitFields>)),
    )?;
    m.add(
        py,
        "git_commit_fields_to_text",
        py_fn!(py, git_commit_fields_to_text(fields: Serde<GitCommitFields>)),
    )?;
    m.add_class::<CommitFields>(py)?;
    Ok(m)
}

py_class!(pub class CommitFields |py| {
    data inner: Box<dyn NativeCommitFields>;

    /// Root tree SHA1.
    def root_tree(&self) -> PyResult<Serde<Id20>> {
        let inner = self.inner(py);
        inner.root_tree().map_pyerr(py).map(Serde)
    }

    /// Author name and email, like "Foo bar <foo@example.com>"
    def author_name(&self) -> PyResult<String> {
        let inner = self.inner(py);
        inner.author_name().map_pyerr(py).map(|s| s.to_owned())
    }

    /// Committer name and email, like "Foo bar <foo@example.com>"
    /// Returns `None` if committer is not explicitly tracked
    /// (i.e. hg format without committer_date extra).
    def committer_name(&self) -> PyResult<Option<String>> {
        let inner = self.inner(py);
        inner.committer_name().map_pyerr(py).map(|s| s.map(|s| s.to_owned()))
    }

    /// Author (creation) date.
    /// (UTC seconds since UNIX epoch, timezone offset in seconds)
    def author_date(&self) -> PyResult<Serde<HgTime>> {
        let inner = self.inner(py);
        inner.author_date().map_pyerr(py).map(Serde)
    }

    /// Committer (modified) date.
    /// Returns `None` if committer is not explicitly tracked
    /// (i.e. hg format without committer_date extra).
    /// (UTC seconds since UNIX epoch, timezone offset in seconds)
    def committer_date(&self) -> PyResult<Serde<Option<HgTime>>> {
        let inner = self.inner(py);
        inner.committer_date().map_pyerr(py).map(Serde)
    }

    /// Parent information. Order-preserved.
    /// Returns `None` if not tracked in the commit text (i.e. hg format).
    def parents(&self) -> PyResult<Option<Serde<Vec<Id20>>>> {
        let inner = self.inner(py);
        let parents = inner.parents().map_pyerr(py)?;
        Ok(parents.map(|v| Serde(v.to_vec())))
    }

    /// Changed files list, separated by space.
    /// Returns `None` if not tracked in the commit text (i.e. git format).
    def files(&self) -> PyResult<Option<Serde<Vec<Text>>>> {
        let inner = self.inner(py);
        let files = inner.files().map_pyerr(py)?;
        Ok(files.map(|v| Serde(v.to_vec())))
    }

    /// extras() -> Dict[str, str]
    def extras(&self) -> PyResult<Serde<BTreeMap<Text, Text>>> {
        let inner = self.inner(py);
        inner.extras().map_pyerr(py).map(|v| Serde(v.clone()))
    }

    /// Commit message encoded in UTF-8.
    def description(&self) -> PyResult<String> {
        let inner = self.inner(py);
        inner.description().map_pyerr(py).map(|s| s.to_owned())
    }

    /// Format of the commit.
    def format(&self) -> PyResult<Serde<SerializationFormat>> {
        let inner = self.inner(py);
        Ok(Serde(inner.format()))
    }

    /// Raw text of the commit object.
    def raw_text(&self) -> PyResult<PyBytes> {
        let inner = self.inner(py);
        let text = inner.raw_text();
        Ok(PyBytes::new(py, text))
    }

    @staticmethod
    def from_text(text: Serde<Text>, format: Serde<SerializationFormat>) -> PyResult<Self> {
        let inner = commit_text_to_fields(text.0, format.0);
        Self::create_instance(py, inner)
    }
});

fn hg_commit_fields_to_text(py: Python, fields: Serde<HgCommitFields>) -> PyResult<String> {
    fields.0.to_text().map_pyerr(py)
}

fn git_commit_fields_to_text(py: Python, fields: Serde<GitCommitFields>) -> PyResult<String> {
    fields.0.to_text().map_pyerr(py)
}
