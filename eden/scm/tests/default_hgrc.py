# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

"""
Default config file for testing
"""

from typing import Optional


# non-debugruntest only uses use_wawtchman and use_ipv6
def get_content(
    use_watchman: bool = False,
    use_ipv6: bool = False,
    modernconfig: bool = False,
    testdir: Optional[str] = None,
    testtmp: Optional[str] = None,
) -> str:
    content = f"""
[ui]
slash=True
interactive=False
mergemarkers=detailed
promptecho=True
ignore.test=$RUNTESTDIR/gitignore

[devel]
all-warnings=True
default-date=0 0

[web]
address=localhost
ipv6={use_ipv6}

[commands]
status.relative=True
update.check=noconflict

[config]
use-rust=True

[status]
use-rust=True

[workingcopy]
rust-checkout=True

[remotefilelog]
cachepath=$TESTTMP/default-hgcache

[mutation]
record=False

[hint]
ack-match-full-traversal=True
ack = smartlog-default-command commitcloud-update-on-move

[experimental]
use-rust-changelog=True
windows-symlinks=True
narrow-heads=true

[tweakdefaults]
graftkeepdate=True
logdefaultfollow=False

[checkout]
use-rust=true

[copytrace]
dagcopytrace=True

[committemplate]
commit-message-fields=Summary,"Test Plan",Reviewers,Subscribers,Tasks,Tags,"Differential Revision","Reviewed By"
summary-field=Summary

[templatealias]
sl_hash_minlen=9

[cas]
disable=true

[remotenames]
rename.default=remote
hoist=remote

[subtree]
min-path-depth=2

[pull]
buffer-commit-count = 5
"""
    if use_watchman:
        content += """
[extensions]
fsmonitor=

[fsmonitor]
fallback-on-watchman-exception=false
"""

    if modernconfig:
        content += f"""
[commitcloud]
hostname=testhost
servicetype=local
servicelocation={testtmp}
remotebookmarkssync=True
supported-url-regex=.*

[experimental]
changegroup3=True
evolution=obsolete

[extensions]
amend=
commitcloud=

[mutation]
enabled=true
record=false
date=0 0

[remotefilelog]
http=True

[treemanifest]
http=True

[ui]
ssh=python {testdir}/dummyssh

[visibility]
enabled=true

[clone]
use-rust=true
"""

    return content
