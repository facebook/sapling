#debugruntest-compatible

#require fsmonitor

  $ configure modernclient
  $ enable sparse

  $ newclientrepo client test:server

  $ cd ~/server
  $ drawdag <<EOS
  > B  # B/included/file = foobar
  > |  # B/excluded/file = foobar
  > |
  > A  # A/included/file = foo
  >    # A/excluded/file = foo
  >    # A/sparse = sparse\nincluded\n.gitignore
  >    # A/.gitignore = excluded
  >    # drawdag.defaultfiles=false
  > EOS

  $ cd ~/client
  $ hg go -q $A
  $ hg sparse enable sparse
  $ hg st
FIXME: should not be tracking excluded/file:
  $ hg debugtreestate list
  .gitignore: * (glob)
  excluded/file: * (glob)
  included/file: * (glob)
  sparse: * (glob)
