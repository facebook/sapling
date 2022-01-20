/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Builtin hgrc templates

static GIT_RC: &str = r#"
[extensions]
remotenames=
treemanifest=
autopullhoisthotfix=!

[commitcloud]
automigrate=false

[remotenames]
autopullhoistpattern=
autopullpattern=
disallowedto=^origin/
disallowhint=please don't specify 'origin/' prefix in remote bookmark's name
hoist=origin

[smartlog]
names=main,master
repos=origin/

[experimental]
copytrace=off

%include /etc/mercurial/git_overrides.rc
%include %PROGRAMDATA%/Facebook/Mercurial/git_overrides.rc
"#;

pub(crate) fn get(name: &str) -> Option<&'static str> {
    if name == "builtin:git.rc" {
        Some(GIT_RC)
    } else {
        None
    }
}
