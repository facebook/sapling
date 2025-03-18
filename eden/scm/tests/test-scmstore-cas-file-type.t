#require symlink execbit no-eden
  $ setconfig scmstore.cas-mode=on

  $ newserver repo
  $ drawdag <<EOS
  > A   # A/regular = regular
  >     # A/symlink = symlink (symlink)
  >     # A/exec    = exec (executable)
  >     # bookmark master = A
  > EOS

  $ newclientrepo client repo
  $ ls -l
  -rw-r--r-- A
  -rwxr-xr-x exec
  -rw-r--r-- regular
  * symlink -> symlink (glob)
