/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use staticconfig::StaticConfig;
use staticconfig::static_config;

/// Git config applied to `sl clone` `.sl` repos.
pub static GIT_CONFIG: StaticConfig = static_config!("builtin:git" => r#"
[extensions]
commitcloud=!
github=
treemanifest=

[commitcloud]
automigrate=false

[remotenames]
autopullhoistpattern=re:tags/\S+$
autopullpattern=re:^[A-Za-z0-9._/-]+/\S+$
disallowedto=^remote/
disallowhint=please don't specify 'remote/' prefix in remote bookmark's name
hoist=remote
publicheads=remote/master,remote/main,m/*
rename.default=remote
selectivepulldefault=main,master

[smartlog]
names=main,master
repos=remote/

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
publicheads=origin/master,origin/main,m/*
rename.default=origin

[smartlog]
repos=origin/

[extensions]
sparse=!

[git]
import-remote-refs=m/*
"#);
