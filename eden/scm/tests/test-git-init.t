#debugruntest-compatible
#require git no-windows

Can init with --git in an existing directory
  $ cd
  $ mkdir init-git-nonempty
  $ cd init-git-nonempty
  $ printf hello > hello
  $ hg init --git .
  $ hg status
  ? hello
