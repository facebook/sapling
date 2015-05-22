  $ heads()
  > {
  >    hg heads --template '{rev}: {desc|firstline|strip} ({branches})\n' "$@"
  > }

  $ hg init a
  $ cd a
  $ echo 'root' >root
  $ hg add root
  $ hg commit -m "Adding root node"
  $ heads
  0: Adding root node ()
-------
  $ heads .
  0: Adding root node ()

=======

  $ echo 'a' >a
  $ hg add a
  $ hg branch a
  marked working directory as branch a
  (branches are permanent and global, did you want a bookmark?)
  $ hg commit -m "Adding a branch"
  $ heads
  1: Adding a branch (a)
  0: Adding root node ()
-------
  $ heads .
  1: Adding a branch (a)

=======

  $ hg update -C 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'b' >b
  $ hg add b
  $ hg branch b
  marked working directory as branch b
  $ hg commit -m "Adding b branch"
  $ heads
  2: Adding b branch (b)
  1: Adding a branch (a)
  0: Adding root node ()
-------
  $ heads .
  2: Adding b branch (b)

=======

  $ echo 'bh1' >bh1
  $ hg add bh1
  $ hg commit -m "Adding b branch head 1"
  $ heads
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
-------
  $ heads .
  3: Adding b branch head 1 (b)

=======

  $ hg update -C 2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'bh2' >bh2
  $ hg add bh2
  $ hg commit -m "Adding b branch head 2"
  created new head
  $ heads
  4: Adding b branch head 2 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
  $ heads .
  4: Adding b branch head 2 (b)
  3: Adding b branch head 1 (b)

=======

  $ hg update -C 2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'bh3' >bh3
  $ hg add bh3
  $ hg commit -m "Adding b branch head 3"
  created new head
  $ heads
  5: Adding b branch head 3 (b)
  4: Adding b branch head 2 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
-------
  $ heads .
  5: Adding b branch head 3 (b)
  4: Adding b branch head 2 (b)
  3: Adding b branch head 1 (b)

=======

  $ hg merge 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "Merging b branch head 2 and b branch head 3"
  $ heads
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
-------
  $ heads .
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)

=======

  $ echo 'c' >c
  $ hg add c
  $ hg branch c
  marked working directory as branch c
  $ hg commit -m "Adding c branch"
  $ heads
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
-------
  $ heads .
  7: Adding c branch (c)

=======

  $ heads -r 3 .
  no open branch heads found on branches c (started at 3)
  [1]
  $ heads -r 2 .
  7: Adding c branch (c)
-------
  $ hg update -C 4
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
-------
  $ heads -r 3 .
  3: Adding b branch head 1 (b)
-------
  $ heads -r 2 .
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
-------
  $ heads -r 7 .
  no open branch heads found on branches b (started at 7)
  [1]

=======

  $ for i in 0 1 2 3 4 5 6 7; do
  >     hg update -C "$i"
  >     heads
  >     echo '-------'
  >     heads .
  >     echo '-------'
  > done
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
  -------
  0: Adding root node ()
  -------
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
  -------
  1: Adding a branch (a)
  -------
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
  -------
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  -------
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
  -------
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  -------
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
  -------
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  -------
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
  -------
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  -------
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
  -------
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  -------
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()
  -------
  7: Adding c branch (c)
  -------

=======

  $ for i in a b c z; do
  >     heads "$i"
  >     echo '-------'
  > done
  1: Adding a branch (a)
  -------
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  -------
  7: Adding c branch (c)
  -------
  abort: unknown revision 'z'!
  -------

=======

  $ heads 0 1 2 3 4 5 6 7
  7: Adding c branch (c)
  6: Merging b branch head 2 and b branch head 3 (b)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)
  0: Adding root node ()

Topological heads:

  $ heads -t
  7: Adding c branch (c)
  3: Adding b branch head 1 (b)
  1: Adding a branch (a)

  $ cd ..
______________

"created new head" message tests

  $ hg init newheadmsg
  $ cd newheadmsg

Init: no msg

  $ echo 1 > a
  $ hg ci -Am "a0: Initial root"
  adding a
  $ echo 2 >> a
  $ hg ci -m "a1 (HN)"

  $ hg branch b
  marked working directory as branch b
  (branches are permanent and global, did you want a bookmark?)
  $ echo 1 > b
  $ hg ci -Am "b2: Initial root for branch b"
  adding b
  $ echo 2 >> b
  $ hg ci -m "b3 (HN)"

Case NN: msg

  $ hg up -q null
  $ hg branch -f b
  marked working directory as branch b
  $ echo 1 > bb
  $ hg ci -Am "b4 (NN): new topo root for branch b"
  adding bb
  created new head

Case HN: no msg

  $ echo 2 >> bb
  $ hg ci -m "b5 (HN)"

Case BN: msg

  $ hg branch -f default
  marked working directory as branch default
  $ echo 1 > aa
  $ hg ci -Am "a6 (BN): new branch root"
  adding aa
  created new head

Case CN: msg

  $ hg up -q 4
  $ echo 3 >> bbb
  $ hg ci -Am "b7 (CN): regular new head"
  adding bbb
  created new head

Case BB: msg

  $ hg up -q 4
  $ hg merge -q 3
  $ hg branch -f default
  marked working directory as branch default
  $ hg ci -m "a8 (BB): weird new branch root"
  created new head

Case CB: msg

  $ hg up -q 4
  $ hg merge -q 1
  $ hg ci -m "b9 (CB): new head from branch merge"
  created new head

Case HB: no msg

  $ hg up -q 7
  $ hg merge -q 6
  $ hg ci -m "b10 (HB): continuing head from branch merge"

Case CC: msg

  $ hg up -q 4
  $ hg merge -q 2
  $ hg ci -m "b11 (CC): new head from merge"
  created new head

Case CH: no msg

  $ hg up -q 2
  $ hg merge -q 10
  $ hg ci -m "b12 (CH): continuing head from merge"

Case HH: no msg

  $ hg merge -q 3
  $ hg ci -m "b12 (HH): merging two heads"

  $ cd ..
