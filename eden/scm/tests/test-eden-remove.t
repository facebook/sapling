
#require eden

setup backing repo

  $ newclientrepo backingrepo
  $ eden clone --allow-empty-repo $TESTTMP/backingrepo $TESTTMP/wcrepo
  Cloning new repository at $TESTTMP/wcrepo...
  Success.  Checked out commit 00000000

Do not provide both '-y' and '-n'
  $ EDENFSCTL_ONLY_RUST=true eden remove -q -y -n $TESTTMP/wcrepo/test_dir
  Error: Both '-y' and '-n' are provided. This is not supported.
  Existing.
  [1]

touch a test file
  $ touch $TESTTMP/wcrepo/file.txt

eden remove this file, answer no when there is prompt
  $ EDENFSCTL_ONLY_RUST=true eden remove -q -n $TESTTMP/wcrepo/file.txt
  Error: User did not confirm the removal. Stopping. Nothing removed!
  [1]

the file is still there
  $ ls $TESTTMP/wcrepo/file.txt | wc -l
  1

eden remove this file, skip prompt with "yes"
  $ EDENFSCTL_ONLY_RUST=true eden remove -q -y $TESTTMP/wcrepo/file.txt

file is now gone
  $ ls $TESTTMP/wcrepo/file.txt
  ls: $TESTTMP/wcrepo/file.txt: $ENOENT$
  [1]

create a test directory

  $ mkdir $TESTTMP/wcrepo/test_dir

eden remove this directory should also see error about Determination state
  $ EDENFSCTL_ONLY_RUST=true eden remove -q -y $TESTTMP/wcrepo/test_dir
  Error: Rust remove(Determination) is not implemented!
  [1]

eden list should give two repos
  $ eden list | wc -l
  2

remove wcrepo with eden rust cli should succeed

  $ EDENFSCTL_ONLY_RUST=true eden remove -q -y $TESTTMP/wcrepo

eden list should now give only one repo
  $ eden list | wc -l
  1

check to make sure the mount point is cleanly removed
  $ ls $TESTTMP/wcrepo
  ls: $TESTTMP/wcrepo: $ENOENT$
  [1]
