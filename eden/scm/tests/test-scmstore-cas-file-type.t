#require symlink execbit no-eden
  $ setconfig scmstore.fetch-from-cas=true scmstore.fetch-tree-aux-data=true scmstore.tree-metadata-mode=always

  $ newserver repo
  $ drawdag <<EOS
  > A   # A/regular = regular
  >     # A/symlink = symlink (symlink)
  >     # A/exec    = exec (executable)
  >     # bookmark master = A
  > EOS

  $ newclientrepo client test:repo
  $ ls -l
  -rw-r--r-- A
  -rwxr-xr-x exec
  -rw-r--r-- regular
  * symlink -> symlink (glob)
