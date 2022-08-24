#chg-compatible

  $ cat <<EOF > mmap-unlink.py
  > import mmap
  > import os
  > import shutil
  > 
  > from edenscm import util
  > 
  > with util.posixfile("file", "w") as f:
  >     f.write("CONTENT")
  > 
  > with util.posixfile("file", "r+b") as f:
  >     m = mmap.mmap(f.fileno(), 0)
  > util.unlink("file")
  > EOF

  $ hg debugpython -- ./mmap-unlink.py
  $ ls
  mmap-unlink.py
