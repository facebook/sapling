"""
Default config file for testing
"""


def get_content(
    use_watchman: bool = False, use_ipv6: bool = False, is_run_tests_py: bool = False
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

[config]
use-rust=True

[workingcopy]
enablerustwalker=True
use-rust=True

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
"""
    if use_watchman:
        content += """
[extensions]
fsmonitor=

[fsmonitor]
detectrace=True
"""

    # Extra configs for run-tests.py.
    # For compatibility. Ideally this does not exist.
    if is_run_tests_py:
        content += """
[scmstore]
enableshim=True
contentstorefallback=True

[workingcopy]
ruststatus=True
"""

    return content
