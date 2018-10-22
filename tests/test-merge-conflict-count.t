  $ enable amend rebase

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
  rebasing 3:211accd27e10 "d" (d)
  merging a
  merging b
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  warning: 2 conflicts while merging b! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat b
  <<<<<<< dest:   2c5d04f1a41f e tip - test: e
  b
  =======
  0
  >>>>>>> source: 211accd27e10 d - test: d
  2
  3
  4
  5
  <<<<<<< dest:   2c5d04f1a41f e tip - test: e
  q
  =======
  9
  >>>>>>> source: 211accd27e10 d - test: d
  7
  8 (no-eol)
  $ cat c
  c (no-eol)

