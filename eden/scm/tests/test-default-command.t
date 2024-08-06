
#require no-eden

#chg-compatible
  $ configure modern
  $ enable smartlog
  $ disable commitcloud

Test running hg without any arguments and various configs
  $ hg | grep "These are some common"
  These are some common Sapling commands.  Use 'hg help commands' to list all
  $ setconfig commands.naked-default.no-repo=sl
  $ setconfig commands.naked-default.in-repo=sl
  $ hg
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]
  $ newclientrepo
  $ drawdag << 'EOS'
  > B  # bookmark stable = B
  > |
  > A
  > EOS
  $ setconfig commands.naked-default.in-repo=sl
  $ hg
  o  commit:      112478962961
  │  bookmark:    stable
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     B
  │
  o  commit:      426bada5c675
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  

  $ touch something
  $ setconfig commands.naked-default.in-repo=status
  $ hg
  ? something
  $ hg --verbose
  ? something
  $ hg --config commands.naked-default.in-repo=log
  commit:      112478962961
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B
  
  commit:      426bada5c675
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A
  


Make sure passing either --help or --version, or using HGPLAIN does not trigger the default command

  $ hg --version -q
  Sapling * (glob)
  $ hg --help | grep "These are some common"
  These are some common Sapling commands.  Use 'hg help commands' to list all
  $ HGPLAIN=true hg | grep "These are some common"
  These are some common Sapling commands.  Use 'hg help commands' to list all

Make sure that running a command without the naked default config errors out outside of a repo but but not inside a repo.

  $ cat >> $HGRCPATH << EOF
  > [commands]
  > naked-default.in-repo=sl
  > %unset naked-default.no-repo
  > EOF
  $ cd
  $ hg
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]
  $ newclientrepo
  $ hg
  $ cat >> $HGRCPATH << EOF
  > [commands]
  > %unset naked-default.in-repo
  > EOF
  $ hg | grep "These are some common"
  These are some common Sapling commands.  Use 'hg help commands' to list all
