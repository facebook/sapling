#debugruntest-compatible

#require git no-eden

  $ configure modernclient

  $ newclientrepo
  $ hg log -r . -T '{nodescheme}\n'
  hg

  $ cd
  $ hg init --git git
  $ cd git
  $ hg log -r . -T '{nodescheme}\n'
  git
