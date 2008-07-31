import os, sys
from mercurial import hg, ui

TESTDIR = os.environ["TESTDIR"]

# only makes sense to test on os which supports symlinks
if not hasattr(os, "symlink"):
    sys.exit(80) # SKIPPED_STATUS defined in run-tests.py

# this is what symlink would do on a non-symlink file system
def symlink_failure(src, dst):
    raise OSError, (1, "Operation not permitted")
os.symlink = symlink_failure

# now try cloning a repo which contains symlinks
u = ui.ui()
hg.clone(u, os.path.join(TESTDIR, 'test-no-symlinks.hg'), 'test1')
