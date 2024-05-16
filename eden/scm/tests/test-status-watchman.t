
#require fsmonitor

  $ enable sparse

  $ newclientrepo client test:server

  $ cd ~/server
  $ drawdag <<EOS
  > B  # B/included/file = foofoo
  > |  # B/excluded/file = barbar
  > |
  > A  # A/included/file = foo
  >    # A/excluded/file = bar
  >    # A/sparse = sparse\nincluded\n.gitignore
  >    # A/.gitignore = excluded/file
  >    # drawdag.defaultfiles=false
  > EOS

  $ cd ~/client
  $ hg go -q $A
  $ hg sparse enable sparse
  $ hg st
  $ hg debugtreestate list
  .gitignore: * (glob)
  included/file: * (glob)
  sparse: * (glob)

  $ hg go -q $B
  $ hg debugmanifestdirs -r .
  0ad8313e7d7d13f03dc9a1412c95600d64876670 /
  25061998a3223db5fcbb0220b0c4410c74944fdd included
  f2b0dbe6584d49d20752468d0c644fb6f117e76a excluded
  $ mkdir excluded
  $ touch excluded/file

Make sure we don't have tree fetches for the "excluded/" directory:
  $ LOG=manifest_tree=trace hg st --config devel.watchman-reset-clock=true 2>&1 | grep f2b0dbe6584d49d20752468d0c644fb6f117e76a
  [1]
