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
github=
infinitepush=!
remotenames=
treemanifest=
autopullhoisthotfix=!

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

# Don't add more config here. Add above the %includes.
# We trim everything starting from %include in tests.

"#;

pub(crate) fn get(name: &str) -> Option<&'static str> {
    if name == "builtin:git.rc" {
        if std::env::var("TESTTMP").is_ok() {
            Some(&GIT_RC[..GIT_RC.find("%include").unwrap_or(GIT_RC.len())])
        } else {
            Some(GIT_RC)
        }
    } else {
        None
    }
}
