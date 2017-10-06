  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > [phases]
  > publish=False
  > [merge]
  > EOF

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ echo b > b
  $ hg commit -qAm ab
  $ echo c >> a
  $ echo c >> b
  $ hg commit -qAm c
  $ hg up -q ".^"
  $ echo d >> a
  $ echo d >> b
  $ hg commit -qAm d

Testing on-failure=continue
  $ echo on-failure=continue >> $HGRCPATH
  $ hg rebase -s 1 -d 2 --tool false
  rebasing 1:1f28a51c3c9b "c"
  merging a
  merging b
  merging a failed!
  merging b failed!
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg resolve --list
  U a
  U b

  $ hg rebase --abort
  rebase aborted

Testing on-failure=halt
  $ echo on-failure=halt >> $HGRCPATH
  $ hg rebase -s 1 -d 2 --tool false
  rebasing 1:1f28a51c3c9b "c"
  merging a
  merging b
  merging a failed!
  merging b failed!
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg resolve --list
  U a
  U b

  $ hg rebase --abort
  rebase aborted

Testing on-failure=prompt
  $ echo on-failure=prompt >> $HGRCPATH
  $ cat <<EOS | hg rebase -s 1 -d 2 --tool false --config ui.interactive=1
  > y
  > n
  > EOS
  rebasing 1:1f28a51c3c9b "c"
  merging a
  merging b
  merging a failed!
  merging b failed!
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg resolve --list
  U a
  U b

  $ hg rebase --abort
  rebase aborted

