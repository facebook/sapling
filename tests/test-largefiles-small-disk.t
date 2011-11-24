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
