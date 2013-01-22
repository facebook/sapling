Test how largefiles abort in case the disk runs full

  $ cat > criple.py <<EOF
  > import os, errno, shutil
  > from mercurial import util
  > #
  > # this makes the original largefiles code abort:
  > def copyfileobj(fsrc, fdst, length=16*1024):
  >     fdst.write(fsrc.read(4))
  >     raise IOError(errno.ENOSPC, os.strerror(errno.ENOSPC))
  > shutil.copyfileobj = copyfileobj
  > #
  > # this makes the rewritten code abort:
  > def filechunkiter(f, size=65536, limit=None):
  >     yield f.read(4)
  >     raise IOError(errno.ENOSPC, os.strerror(errno.ENOSPC))
  > util.filechunkiter = filechunkiter
  > #
  > def oslink(src, dest):
  >     raise OSError("no hardlinks, try copying instead")
  > util.oslink = oslink
  > EOF

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "largefiles =" >> $HGRCPATH

  $ hg init alice
  $ cd alice
  $ echo "this is a very big file" > big
  $ hg add --large big
  $ hg commit --config extensions.criple=$TESTTMP/criple.py -m big
  abort: No space left on device
  [255]

The largefile is not created in .hg/largefiles:

  $ ls .hg/largefiles
  dirstate

The user cache is not even created:

  >>> import os; os.path.exists("$HOME/.cache/largefiles/")
  False

Make the commit with space on the device:

  $ hg commit -m big

Now make a clone with a full disk, and make sure lfutil.link function
makes copies instead of hardlinks:

  $ cd ..
  $ hg --config extensions.criple=$TESTTMP/criple.py clone --pull alice bob
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  getting changed largefiles
  abort: No space left on device
  [255]

The largefile is not created in .hg/largefiles:

  $ ls bob/.hg/largefiles
