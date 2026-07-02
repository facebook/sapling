#inprocess-hg-incompatible

  $ eagerepo
  $ newrepo

The "enabled" template returns true or false:

  $ sl log -r null -T '{enabled("rebase")}\n'
  False
  $ sl log -r null -T '{enabled("rebase")}\n' --config extensions.rebase=
  True

Can be used in "if" template function:

  $ sl log -r null -T '{if(enabled("rebase"),1,2)}\n'
  2
  $ sl log -r null -T '{if(enabled("rebase"),1,2)}\n' --config extensions.rebase=
  1

Missing arguments:

  $ sl log -r null -T '{enabled()}\n'
  sl: parse error: enabled expects one argument
  [255]
