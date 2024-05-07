# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Default config file for testing
"""

import os

from typing import Optional


# non-debugruntest only uses use_wawtchman and use_ipv6
def get_content(
    use_watchman: bool = False,
    use_ipv6: bool = False,
    edenpath: Optional[str] = None,
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
collapse-traceback =True
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

[extensions]
treemanifest=
copytrace=

[treemanifest]
sendtrees=True
treeonly=True
rustmanifest=True
useruststore=True

[remotefilelog]
reponame=reponame-default
cachepath=$TESTTMP/default-hgcache

[mutation]
record=False

[pull]
httpcommitgraph2=true

[hint]
ack-match-full-traversal=True

[scmstore]
contentstorefallback=True

[experimental]
use-rust-changelog=True
windows-symlinks=True
copytrace=off

[tweakdefaults]
graftkeepdate=True
logdefaultfollow=False

[checkout]
use-rust=true

[copytrace]
dagcopytrace=True
"""
    if use_watchman:
        content += """
[extensions]
fsmonitor=

[fsmonitor]
fallback-on-watchman-exception=false
"""

    if edenpath:
        content += f"""
[edenfs]
command={edenpath + ('.bat' if os.name == "nt" else '')}
backing-repos-dir=$TESTTMP/.eden-backing-repos

[clone]
use-eden=True
"""

    if modernconfig:
        content += f"""
[commitcloud]
hostname=testhost
servicetype=local
servicelocation={testtmp}
remotebookmarkssync=True

[experimental]
changegroup3=True
evolution=obsolete
narrow-heads=true

[extensions]
amend=
commitcloud=
infinitepush=
remotenames=

[mutation]
enabled=true
record=false
date=0 0

[remotefilelog]
http=True

[remotenames]
rename.default=remote
hoist=remote
selectivepull=True
selectivepulldefault=master

[treemanifest]
http=True

[ui]
ssh=python {testdir}/dummyssh

[visibility]
enabled=true
"""

    return content
