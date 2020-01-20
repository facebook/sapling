#chg-compatible

  $ configure evolution
  $ enable amend directaccess
  $ newrepo
  $ drawdag <<'EOS'
  > C E
  > | |
  > B D
  > |/
  > A
  > EOS

  $ hg hide -q $B+$D

Both string and symbol are processed

  $ hg metaedit --fold "'$B'+$C" -m foo
  Warning: accessing hidden changesets 112478962961,26805aba1e60 for write operation
  2 changesets folded

"Or" function is handled

  $ hg log -r "$D+'$E'+merge()" -T '{desc}\n'
  D
  E
