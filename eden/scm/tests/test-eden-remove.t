
#require eden

setup backing repo

  $ newclientrepo backingrepo
  $ eden clone --allow-empty-repo $TESTTMP/backingrepo $TESTTMP/wcrepo
  Cloning new repository at $TESTTMP/wcrepo...
  Success.  Checked out commit 00000000

remove wcrepo with eden rust cli

  $ EDENFSCTL_ONLY_RUST=true eden remove -y $TESTTMP/wcrepo
  Error: Rust remove(Determination) is not implemented!
  [1]
