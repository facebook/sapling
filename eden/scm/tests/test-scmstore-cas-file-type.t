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
FIXME: wrong file types
  $ ls -l
  -rw-r--r-- A
  -rw-r--r-- exec
  -rw-r--r-- regular
  -rw-r--r-- symlink
