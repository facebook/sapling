  $ enable smartlog

  $ newclientrepo
  $ touch foo
  $ hg ci -Aqm foo
  $ setconfig remotenames.selectivepulldefault=banana
  $ hg push -q --to banana --create
  $ echo foo > foo
  $ hg ci -qm bar
  $ hg push -q --to apple --create
  $ hg pull -qB apple

We show selectivepulldefault by default:
  $ hg log -r 'interestingmaster()' -T '{remotebookmarks}\n'
  remote/banana
