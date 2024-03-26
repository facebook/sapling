/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use staticconfig::static_config;
use staticconfig::StaticConfig;

/// Config loaded only in OSS build.
pub static CONFIG: StaticConfig = static_config!("builtin:open_source" => r###"
[annotate]
default-flags=user short-date

[fsmonitor]
# TODO: T130638905 Update this
sockpath=/opt/facebook/watchman/var/run/watchman/%i-state/sock

[remotenames]
# TODO what's the right oss value for this?
autopullhoistpattern=
disallowedbookmarks=master
 remote/master
 main
 remote/main

[tweakdefaults]
singlecolonmsg=':' is deprecated; use '::' instead.

[ui]
style=sl_default
allowmerge=True
disallowedbrancheshint=use bookmarks instead

[committemplate]
defaultadvice=
emptymsg={if(title, title, defaulttitle)}\n
 Summary: {summary}\n
 Test Plan: {testplan}\n

[smartlog]
names=master main

[amend]
autorestackmsg=automatically restacking children!

[init]
prefer-git=True

[color]
use-rust=false
"###);
