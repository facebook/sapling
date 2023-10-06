/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use staticconfig::static_config;
use staticconfig::StaticConfig;

/// Default config. Partially migrated from configitems.py.
///
/// Lowest priority. Should always be loaded.
pub static CONFIG: StaticConfig = static_config!("builtin:core" => r#"
[treestate]
mingcage=900
minrepackthreshold=10M
repackfactor=3

[ui]
timeout=600
color=auto
paginate=true

[checkout]
resumable=true

[tracing]
stderr=false
threshold=10

[format]
generaldelta=false
usegeneraldelta=true

[color]
status.added=green bold
status.clean=none
status.copied=none
status.deleted=cyan bold underline
status.ignored=black bold
status.modified=blue bold
status.removed=red bold
status.unknown=magenta bold underline

[unsafe]
filtersuspectsymlink=true

[experimental]
exportstack-max-bytes=1M

log-implicit-follow-threshold=10000

titles-namespace=true
local-committemplate=true

[zsh]
completion-age=7
completion-description=false

[merge]
enable-merge-tool-script=true

[remotenames]
autocleanupthreshold=50
selectivepulldefault=master
selectivepulldiscovery=true
autopullhoistpattern=
autopullpattern=re:^(?:default|remote)/[A-Za-z0-9._/-]+$
hoist=default
"#);
