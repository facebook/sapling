  $ enable directaccess fbamend obsstore
  $ newrepo
  $ drawdag <<'EOS'
  > C E
  > | |
  > B D
  > |/
  > A
  > EOS

  $ hg hide -q $B+$D

  $ hg metaedit --fold "'$B'+$C" -m foo
  Warning: accessing hidden changesets 112478962961,26805aba1e60 for write operation
  2 changesets folded
