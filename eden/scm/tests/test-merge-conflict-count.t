#chg-compatible

  $ enable amend rebase
  $ setconfig merge.printcandidatecommmits=True

Encountering a merge conflict prints the number of textual conflicts in each file:
  $ newrepo
  $ hg debugdrawdag <<'EOS'
  > e
  > |            # "b" has two conflicts between c and d:
  > c d          # b/b = 1\n2\n3\n4\n5\n6\n7\n8
  > |/           # c/b = b\n2\n3\n4\n5\nq\n7\n8
  > b            # d/b = 0\n2\n3\n4\n5\n9\n7\n8
  > |
  > |            # "a" has two conflicts between c and d:
  > |            # c/a = one
  > a            # d/a = two
  > EOS
  $ hg rebase -r d -d e
  rebasing 211accd27e10 "d" (d)
  merging a
  merging b
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
   1 commits might have introduced this conflict:
    - [21906429c027] c
  warning: 2 conflicts while merging b! (edit, then use 'hg resolve --mark')
   1 commits might have introduced this conflict:
    - [21906429c027] c
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat b
  <<<<<<< dest:   2c5d04f1a41f e - test: e
  b
  =======
  0
  >>>>>>> source: 211accd27e10 d - test: d
  2
  3
  4
  5
  <<<<<<< dest:   2c5d04f1a41f e - test: e
  q
  =======
  9
  >>>>>>> source: 211accd27e10 d - test: d
  7
  8 (no-eol)
  $ cat c
  c (no-eol)

A merge conflict prints the possible conflicting commits:
  $ newrepo
  $ hg debugdrawdag <<'EOS'
  > j
  > |            # d conflicts with c and h. e changed the file but doesn't
  > i            # conflict. We'll see that it gets included in the list as
  > |            # the algorithm isn't smart enough yet to know that.
  > h            # b/b = 1\n2\n3\n4\n5\n6\n7\n8
  > |            # c/b = b\n2\n3\n4\n5\nq\n7\n8
  > g            # d/b = 0\n2\n3\n4\n5\n9\n7\n8
  > |            # e/b = b\n2\n3\n4\n5\nq\n7\n8\nnot_conflicting
  > f            # h/b = c\n2\n3\n4\n5\nq\n7\n8\nnot_conflicting
  > |
  > e
  > |
  > c d
  > |/
  > b
  > |
  > |            # "a" has two conflicts between g and d:
  > |            # g/a = one
  > a            # d/a = two
  > EOS
  $ hg rebase -r d -d j
  rebasing 211accd27e10 "d" (d)
  merging a
  merging b
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
   1 commits might have introduced this conflict:
    - [395beecc0ab6] g
  warning: 2 conflicts while merging b! (edit, then use 'hg resolve --mark')
   3 commits might have introduced this conflict:
    - [0942ca9aff3d] h
    - [3ebd0a462491] e
    - [3e5843b4b236] c
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
