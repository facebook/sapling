#debugruntest-compatible
#require git no-windows no-eden

  $ eagerepo
Can init with --git in an existing directory
  $ mkdir init-git-nonempty
  $ cd init-git-nonempty
  $ echo hello > hello

  $ sl init --git .
  $ sl status
  ? hello

Re-init is an error
  $ sl init --git
  abort: repository `$TESTTMP/init-git-nonempty` already exists
  [255]
