/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::ConfigSet;
use super::Result;
use super::IO;
use anyhow::format_err;
use anyhow::Context;
use clidispatch::errors;
use cliparser::define_flags;
use dag::namedag::IndexedLogNameDagPath;
use dag::ops::DagImportCloneData;
use dag::ops::DagPersistent;
use dag::ops::Open;
use dag::CloneData;
use dag::VertexName;
use edenapi::EdenApiBlocking;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

define_flags! {
    pub struct StatusOpts {
        #[arg]
        reponame: String,

        #[arg]
        dest: String,
    }
}
pub fn run(opts: StatusOpts, _io: &mut IO, config: ConfigSet) -> Result<u8> {
    let reponame = opts.reponame;
    let destination = PathBuf::from(&opts.dest);

    if destination.exists() {
        return Err(
            errors::Abort(format!("destination {} exists", destination.display()).into()).into(),
        );
    }

    let edenapi_client = edenapi::Builder::from_config(&config)?.build()?;

    // TODO: add progress bar
    let progress_callback = None;
    let clone_data = edenapi_client
        .full_idmap_clone_data_blocking(reponame.clone(), progress_callback)
        .context("error cloning segmented changelog")?;

    let namedag_path = IndexedLogNameDagPath(destination.join(".hg/store/segments/v1"));
    let mut namedag = namedag_path
        .open()
        .context("error opening segmented changelog")?;

    let idmap: HashMap<dag::Id, dag::Vertex> = clone_data
        .idmap
        .into_iter()
        .map(|(k, v)| (k, VertexName::copy_from(&v.into_byte_array())))
        .collect();

    let master = idmap
        .get(&clone_data.head_id)
        .cloned()
        .ok_or_else(|| format_err!("head_id does not have idmap entry"))?;
    let vertex_clone_data = CloneData {
        head_id: clone_data.head_id,
        flat_segments: clone_data.flat_segments,
        idmap,
    };

    namedag
        .import_clone_data(vertex_clone_data)
        .context("error importing segmented changelog")?;

    namedag
        .flush(&[master.clone()])
        .context("error writing segmented changelog to disk")?;

    fs::write(
        destination.join(".hg/requires"),
        b"dotencode\n\
          fncache\n\
          generaldelta\n\
          remotefilelog\n\
          store\n\
          treestate\n",
    )
    .context("error writing to hg requires")?;

    fs::write(
        destination.join(".hg/store/requires"),
        b"hybridchangelog\n\
          narrowheads\n\
          visibleheads\n",
    )
    .context("error writing to hg store requires")?;

    fs::write(
        destination.join(".hg/hgrc"),
        format!(
            "[paths]\n\
             default = ssh://hg.vip.facebook.com//data/scm/{0}\n\
             %include /etc/mercurial/repo-specific/{0}.rc\n",
            reponame
        )
        .as_bytes(),
    )
    .context("error writing to hg store requires")?;

    fs::write(
        destination.join(".hg/store/remotenames"),
        format!("{} bookmarks remote/master\n", master.to_hex()).as_bytes(),
    )
    .context("error writing to hg store requires")?;

    Ok(0)
}

pub fn name() -> &'static str {
    "debugsegmentclone"
}

pub fn doc() -> &'static str {
    "clone a repository using segmented changelog"
}
