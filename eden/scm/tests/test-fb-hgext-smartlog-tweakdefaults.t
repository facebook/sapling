#chg-compatible

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > rebase=
  > smartlog=
  > tweakdefaults=
  > [experimental]
  > evolution=createmarkers
  > evolution.allowdivergence=on
  > EOF

Prepare a repo for amend checks
  $ hg init repo
  $ cd repo
  $ echo root > root && hg ci -Am root # rev 0
  adding root
  $ echo base > base && hg ci -Am base # rev 1
  adding base
  $ echo a > a && hg ci -Am a          # rev 2
  adding a
  $ echo aa > a && hg amend            # rev 3
  $ hg up --hidden -r 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa > a && hg amend           # rev 4

Check the amend template keywords
  $ hg log --hidden -r 2 -T "{node} amended as {amendsuccessors % '{short(amendsuccessor)} '}\n"
  6e2c701de62843743b3ad0c4397a88605f0aa7c9 amended as [a-f0-9]* [a-f0-9]*  (re)

Prepare a repo for rebase checks
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b > b && hg ci -Am b          # rev 5
  adding b
  $ hg rebase --hidden -r 5 -d 1       # rev 6
  rebasing 1e9a3c00cbe9 "b"
  $ hg rebase --hidden -r 5 -d 2       # rev 7
  rebasing 1e9a3c00cbe9 "b"

Check the rebase template keywords
  $ hg log --hidden -r 5 -T "{node} rebased as {rebasesuccessors % '{short(rebasesuccessor)} '}\n"
  1e9a3c00cbe90d236ac05ef61efcc5e40b7412bc rebased as [a-f0-9]* [a-f0-9]*  (re)

