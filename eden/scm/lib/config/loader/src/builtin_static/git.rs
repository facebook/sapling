/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use staticconfig::static_config;
use staticconfig::StaticConfig;

/// Git config applied to `sl clone` `.sl` repos.
pub static GIT_CONFIG: StaticConfig = static_config!("builtin:git" => r#"
[commands]
new-pull=true

[extensions]
commitcloud=!
github=
infinitepush=!
remotenames=
treemanifest=

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

[ghrevset]
autopull=True
"#);

/// Extra Git config applied to `git clone` `.git/` repos.
/// Overrides a subset of the above config.
/// Namely, use the name "origin" instead of "remote" to match Git's default names.
pub static DOTGIT_OVERRIDE_CONFIG: StaticConfig = static_config!("builtin:dotgit" => r#"
[remotenames]
disallowedto=^origin/
disallowhint=please don't specify 'origin/' prefix in remote bookmark's name
hoist=origin
publicheads=origin/master,origin/main
rename.default=origin

[smartlog]
repos=origin/
"#);
