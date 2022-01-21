#chg-compatible

  $ newrepo

The "enabled" template returns true or false:

  $ hg log -r null -T '{enabled("rebase")}\n'
  False
  $ hg log -r null -T '{enabled("rebase")}\n' --config extensions.rebase=
  True

Can be used in "if" template function:

  $ hg log -r null -T '{if(enabled("rebase"),1,2)}\n'
  2
  $ hg log -r null -T '{if(enabled("rebase"),1,2)}\n' --config extensions.rebase=
  1

Missing arguments:

  $ hg log -r null -T '{enabled()}\n'
  hg: parse error: enabled expects one argument
  [255]
