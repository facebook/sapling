#debugruntest-compatible

#require git

  $ configure modernclient

  $ newclientrepo
  $ hg log -r . -T '{nodescheme}\n'
  hg

  $ cd
  $ hg init --git git
  $ cd git
  $ hg log -r . -T '{nodescheme}\n'
  git
