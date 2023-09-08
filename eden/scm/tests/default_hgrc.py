"""
Default config file for testing
"""


def get_content(use_watchman: bool = False, use_ipv6: bool = False) -> str:
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

[config]
use-rust=True

[workingcopy]
use-rust=True
rust-status=True

[status]
use-rust=True

[extensions]
treemanifest=

[treemanifest]
sendtrees=True
treeonly=True
rustmanifest=True
useruststore=True

[remotefilelog]
reponame=reponame-default
localdatarepack=True
cachepath=$TESTTMP/default-hgcache

[mutation]
record=False

[hint]
ack-match-full-traversal=True

[scmstore]
contentstorefallback=True

[experimental]
rustmatcher=True
use-rust-changelog=True
windows-symlinks=True
"""
    if use_watchman:
        content += """
[extensions]
fsmonitor=

[fsmonitor]
detectrace=True
"""

    return content
