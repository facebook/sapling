import os, sys, time
from mercurial import hg, ui, commands, util

TESTDIR = os.environ["TESTDIR"]
BUNDLEPATH = os.path.join(TESTDIR, 'bundles', 'test-no-symlinks.hg')

# only makes sense to test on os which supports symlinks
if not getattr(os, "symlink", False):
    sys.exit(80) # SKIPPED_STATUS defined in run-tests.py

# clone with symlink support
u = ui.ui()
hg.clone(u, {}, BUNDLEPATH, 'test0')

repo = hg.repository(u, 'test0')

# wait a bit, or the status call wont update the dirstate
time.sleep(1)
commands.status(u, repo)

# now disable symlink support -- this is what os.symlink would do on a
# non-symlink file system
def symlink_failure(src, dst):
    raise OSError, (1, "Operation not permitted")
os.symlink = symlink_failure

# dereference links as if a Samba server has exported this to a
# Windows client
for f in 'test0/a.lnk', 'test0/d/b.lnk':
    os.unlink(f)
    fp = open(f, 'wb')
    fp.write(util.readfile(f[:-4]))
    fp.close()

# reload repository
u = ui.ui()
repo = hg.repository(u, 'test0')
commands.status(u, repo)

# try cloning a repo which contains symlinks
u = ui.ui()
hg.clone(u, {}, BUNDLEPATH, 'test1')
