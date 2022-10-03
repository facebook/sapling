/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::errors;
use clidispatch::ReqCtx;
use cliparser::define_flags;

use super::ConfigSet;
use super::MergeToolOpts;
use super::Result;

define_flags! {
    pub struct GotoOpts {
        /// discard uncommitted changes (no backup)
        #[short('C')]
        clean: bool,

        /// require clean working directory
        #[short('c')]
        check: bool,

        /// merge uncommitted changes
        #[short('m')]
        merge: bool,

        /// tipmost revision matching date (ADVANCED)
        #[short('d')]
        #[argtype("DATE")]
        date: String,

        /// revision
        #[short('r')]
        #[argtype("REV")]
        rev: String,

        /// update without activating bookmarks
        inactive: bool,

        /// resume interrupted update --merge (ADVANCED)
        r#continue: bool,

        merge_opts: MergeToolOpts,

        /// create new bookmark
        #[short('B')]
        #[argtype("VALUE")]
        bookmark: String,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(_ctx: ReqCtx<GotoOpts>, _config: &mut ConfigSet) -> Result<u8> {
    Err(errors::FallbackToPython("not yet implemented".to_owned()).into())
}

pub fn aliases() -> &'static str {
    "update|up|checkout|co|upd|upda|updat|che|chec|check|checko|checkou|goto|go"
}

pub fn doc() -> &'static str {
    r#"check out a specific commit

Update your checkout to the given destination commit. More precisely, make
the destination commit the current commit and update the contents of all
files in your checkout to match their state in the destination commit.

By default, if you attempt to check out a commit while you have pending
changes, and the destination commit is not an ancestor or descendant of
the current commit, the checkout will abort. However, if the destination
commit is an ancestor or descendant of the current commit, the pending
changes will be merged into the new checkout.

Use one of the following flags to modify this behavior:

--check: abort if there are pending changes

--clean: permanently discard any pending changes (use with caution)

--merge: attempt to merge the pending changes into the new checkout, even
if the destination commit is not an ancestor or descendant of the current
commit

If merge conflicts occur during checkout, Mercurial enters an unfinished
merge state. If this happens, fix the conflicts manually and then run
hg commit to exit the unfinished merge state and save your changes in a
new commit. Alternatively, run hg checkout --clean to discard your pending
changes.

Specify null as the destination commit to get an empty checkout (sometimes
known as a bare repository).

Returns 0 on success, 1 if there are unresolved files."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[-C|-c|-m] [[-r] REV]")
}
