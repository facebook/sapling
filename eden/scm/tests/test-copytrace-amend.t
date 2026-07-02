
  $ configure mutation-norecord
  $ enable amend rebase shelve

Test amend copytrace
  $ sl init repo
  $ cd repo
  $ echo x > x
  $ sl add x
  $ sl ci -m initial
  $ echo a > a
  $ sl add a
  $ sl ci -m "create a"
  $ echo b > a
  $ sl ci -qm "mod a"
  $ sl up -q ".^"
  $ sl mv a b
  $ sl amend
  hint[amend-restack]: descendants of 9f815da0cfb3 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl rebase --restack
  rebasing ad25e018afa9 "mod a"
  merging b and a to b
  $ ls
  b
  x
  $ cat b
  a
  $ sl goto 'max(desc(mod))'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat b
  b
  $ cd ..
  $ rm -rf repo

Test amend copytrace with multiple stacked commits
  $ sl init repo
  $ cd repo
  $ echo x > x
  $ sl add x
  $ sl ci -m initial
  $ echo a > a
  $ echo b > b
  $ echo c > c
  $ sl add a b c
  $ sl ci -m "create a b c"
  $ echo a1 > a
  $ sl ci -qm "mod a"
  $ echo b2 > b
  $ sl ci -qm "mod b"
  $ echo c3 > c
  $ sl ci -qm "mod c"
  $ sl bookmark test-top
  $ sl up -q '.~3'
  $ sl mv a a1
  $ sl mv b b2
  $ sl amend
  hint[amend-restack]: descendants of ec8c441da632 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl mv c c3
  $ sl amend
  $ sl rebase --restack
  rebasing 797127d4e250 "mod a"
  merging a1 and a to a1
  rebasing e2aabbfe749a "mod b"
  merging b2 and b to b2
  rebasing 4f8d18558559 "mod c" (test-top)
  merging c3 and c to c3
  $ sl up test-top
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark test-top)
  $ cat a1 b2 c3
  a1
  b2
  c3
  $ cd ..
  $ rm -rf repo

Test amend copytrace with multiple renames of the same file
  $ sl init repo
  $ cd repo
  $ echo x > x
  $ sl add x
  $ sl ci -m initial
  $ echo a > a
  $ sl add a
  $ sl ci -m "create a"
  $ echo b > a
  $ sl ci -qm "mod a"
  $ sl up -q ".^"
  $ sl mv a b
  $ sl amend
  hint[amend-restack]: descendants of 9f815da0cfb3 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl mv b c
  $ sl amend
  $ sl rebase --restack
  rebasing ad25e018afa9 "mod a"
  merging c and a to c
  $ sl goto 'max(desc(mod))'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat c
  b
  $ cd ..
  $ rm -rf repo

Test amend copytrace with copies
  $ sl init repo
  $ cd repo
  $ echo x > x
  $ sl add x
  $ sl ci -m initial
  $ echo a > a
  $ echo i > i
  $ sl add a i
  $ sl ci -m "create a i"
  $ echo b > a
  $ sl ci -qm "mod a"
  $ echo j > i
  $ sl ci -qm "mod i"
  $ sl bookmark test-top
  $ sl up -q ".~2"
  $ sl cp a b
  $ sl amend
  hint[amend-restack]: descendants of 0157114ee1b3 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl cp i j
  $ sl amend
  $ sl cp b c
  $ sl amend
  $ sl rebase --restack
  rebasing 6938f0d82b23 "mod a"
  merging b and a to b
  merging c and a to c
  rebasing df8dfcb1d237 "mod i" (test-top)
  merging j and i to j
  $ sl up test-top
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark test-top)
  $ cat a b c i j
  b
  b
  b
  j
  j
  $ cd ..
  $ rm -rf repo

Test rebase after amend deletion of copy
  $ sl init repo
  $ cd repo
  $ echo x > x
  $ sl add x
  $ sl ci -m initial
  $ echo a > a
  $ sl add a
  $ sl ci -m "create a"
  $ echo b > a
  $ sl ci -qm "mod a"
  $ sl up -q ".^"
  $ sl cp a b
  $ sl amend
  hint[amend-restack]: descendants of 9f815da0cfb3 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl rm b
  $ sl amend
  $ sl rebase --restack
  rebasing ad25e018afa9 "mod a"
  $ cd ..
  $ rm -rf repo

Test failure to rebase deletion after rename
  $ sl init repo
  $ cd repo
  $ echo x > x
  $ sl add x
  $ sl ci -m initial
  $ echo a > a
  $ sl add a
  $ sl ci -m "create a"
  $ echo b > a
  $ sl ci -qm "mod a"
  $ sl rm a
  $ sl ci -m "delete a"
  $ sl up -q ".~2"
  $ sl mv a b
  $ sl amend
  hint[amend-restack]: descendants of 9f815da0cfb3 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl rebase --restack
  rebasing ad25e018afa9 "mod a"
  merging b and a to b
  rebasing ba0395f0e180 "delete a"
  local [dest] changed b which other [source] deleted (as a)
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
  $ sl rebase --abort
  rebase aborted
  $ cd ..
  $ rm -rf repo

Test amend copytrace can be disabled
  $ cat >> $HGRCPATH << EOF
  > [copytrace]
  > enableamendcopytrace=false
  > EOF
  $ sl init repo
  $ cd repo
  $ echo x > x
  $ sl add x
  $ sl ci -m initial
  $ echo a > a
  $ sl add a
  $ sl ci -m "create a"
  $ echo b > a
  $ sl ci -qm "mod a"
  $ sl up -q ".^"
  $ sl mv a b
  $ sl amend
  hint[amend-restack]: descendants of 9f815da0cfb3 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl rebase --restack
  rebasing ad25e018afa9 "mod a"
  other [source] changed a which local [dest] is missing
  hint: the missing file was probably added by commit 9f815da0cfb3 in the branch being rebased
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
  $ cd ..
  $ rm -rf repo
