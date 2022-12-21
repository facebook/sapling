/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Builtin hgrc templates

static GIT_RC: &str = r#"
[commands]
new-pull=true

[extensions]
commitcloud=!
infinitepush=!
remotenames=
treemanifest=
autopullhoisthotfix=!
prmarker=

[commitcloud]
automigrate=false

[remotenames]
autopullhoistpattern=re:tags/\S+$
autopullpattern=re:^[A-Za-z0-9._/-]+/\S+$
disallowedto=^remote/
disallowhint=please don't specify 'remote/' prefix in remote bookmark's name
hoist=remote
publicheads=remote/master,remote/main
rename.default=remote
selectivepulldefault=main,master
selectivepull=true

[smartlog]
names=main,master
repos=remote/

[experimental]
copytrace=off

[tweakdefaults]
defaultdest=

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
