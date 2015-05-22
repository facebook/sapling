  $ hg init a
  $ cd a
  $ echo 'root' >root
  $ hg add root
  $ hg commit -d '0 0' -m "Adding root node"

  $ echo 'a' >a
  $ hg add a
  $ hg branch a
  marked working directory as branch a
  (branches are permanent and global, did you want a bookmark?)
  $ hg commit -d '1 0' -m "Adding a branch"

  $ hg branch q
  marked working directory as branch q
  $ echo 'aa' >a
  $ hg branch -C
  reset working directory to branch a
  $ hg commit -d '2 0' -m "Adding to a branch"

  $ hg update -C 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'b' >b
  $ hg add b
  $ hg branch b
  marked working directory as branch b
  $ hg commit -d '2 0' -m "Adding b branch"

  $ echo 'bh1' >bh1
  $ hg add bh1
  $ hg commit -d '3 0' -m "Adding b branch head 1"

  $ hg update -C 2
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo 'bh2' >bh2
  $ hg add bh2
  $ hg commit -d '4 0' -m "Adding b branch head 2"

  $ echo 'c' >c
  $ hg add c
  $ hg branch c
  marked working directory as branch c
  $ hg commit -d '5 0' -m "Adding c branch"

reserved names

  $ hg branch tip
  abort: the name 'tip' is reserved
  [255]
  $ hg branch null
  abort: the name 'null' is reserved
  [255]
  $ hg branch .
  abort: the name '.' is reserved
  [255]

invalid characters

  $ hg branch 'foo:bar'
  abort: ':' cannot be used in a name
  [255]

  $ hg branch 'foo
  > bar'
  abort: '\n' cannot be used in a name
  [255]

trailing or leading spaces should be stripped before testing duplicates

  $ hg branch 'b '
  abort: a branch of the same name already exists
  (use 'hg update' to switch to it)
  [255]

  $ hg branch ' b'
  abort: a branch of the same name already exists
  (use 'hg update' to switch to it)
  [255]

verify update will accept invalid legacy branch names

  $ hg init test-invalid-branch-name
  $ cd test-invalid-branch-name
  $ hg pull -u "$TESTDIR"/bundles/test-invalid-branch-name.hg
  pulling from *test-invalid-branch-name.hg (glob)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg update '"colon:test"'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

  $ echo 'd' >d
  $ hg add d
  $ hg branch 'a branch name much longer than the default justification used by branches'
  marked working directory as branch a branch name much longer than the default justification used by branches
  $ hg commit -d '6 0' -m "Adding d branch"

  $ hg branches
  a branch name much longer than the default justification used by branches 7:10ff5895aa57
  b                              4:aee39cd168d0
  c                              6:589736a22561 (inactive)
  a                              5:d8cbc61dbaa6 (inactive)
  default                        0:19709c5a4e75 (inactive)

-------

  $ hg branches -a
  a branch name much longer than the default justification used by branches 7:10ff5895aa57
  b                              4:aee39cd168d0

--- Branch a

  $ hg log -b a
  changeset:   5:d8cbc61dbaa6
  branch:      a
  parent:      2:881fe2b92ad0
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     Adding b branch head 2
  
  changeset:   2:881fe2b92ad0
  branch:      a
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     Adding to a branch
  
  changeset:   1:dd6b440dd85a
  branch:      a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     Adding a branch
  

---- Branch b

  $ hg log -b b
  changeset:   4:aee39cd168d0
  branch:      b
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     Adding b branch head 1
  
  changeset:   3:ac22033332d1
  branch:      b
  parent:      0:19709c5a4e75
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     Adding b branch
  

---- going to test branch closing

  $ hg branches
  a branch name much longer than the default justification used by branches 7:10ff5895aa57
  b                              4:aee39cd168d0
  c                              6:589736a22561 (inactive)
  a                              5:d8cbc61dbaa6 (inactive)
  default                        0:19709c5a4e75 (inactive)
  $ hg up -C b
  2 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ echo 'xxx1' >> b
  $ hg commit -d '7 0' -m 'adding cset to branch b'
  $ hg up -C aee39cd168d0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'xxx2' >> b
  $ hg commit -d '8 0' -m 'adding head to branch b'
  created new head
  $ echo 'xxx3' >> b
  $ hg commit -d '9 0' -m 'adding another cset to branch b'
  $ hg branches
  b                             10:bfbe841b666e
  a branch name much longer than the default justification used by branches 7:10ff5895aa57
  c                              6:589736a22561 (inactive)
  a                              5:d8cbc61dbaa6 (inactive)
  default                        0:19709c5a4e75 (inactive)
  $ hg heads --closed
  changeset:   10:bfbe841b666e
  branch:      b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     adding another cset to branch b
  
  changeset:   8:eebb944467c9
  branch:      b
  parent:      4:aee39cd168d0
  user:        test
  date:        Thu Jan 01 00:00:07 1970 +0000
  summary:     adding cset to branch b
  
  changeset:   7:10ff5895aa57
  branch:      a branch name much longer than the default justification used by branches
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     Adding d branch
  
  changeset:   6:589736a22561
  branch:      c
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     Adding c branch
  
  changeset:   5:d8cbc61dbaa6
  branch:      a
  parent:      2:881fe2b92ad0
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     Adding b branch head 2
  
  changeset:   0:19709c5a4e75
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Adding root node
  
  $ hg heads
  changeset:   10:bfbe841b666e
  branch:      b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     adding another cset to branch b
  
  changeset:   8:eebb944467c9
  branch:      b
  parent:      4:aee39cd168d0
  user:        test
  date:        Thu Jan 01 00:00:07 1970 +0000
  summary:     adding cset to branch b
  
  changeset:   7:10ff5895aa57
  branch:      a branch name much longer than the default justification used by branches
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     Adding d branch
  
  changeset:   6:589736a22561
  branch:      c
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     Adding c branch
  
  changeset:   5:d8cbc61dbaa6
  branch:      a
  parent:      2:881fe2b92ad0
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     Adding b branch head 2
  
  changeset:   0:19709c5a4e75
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Adding root node
  
  $ hg commit -d '9 0' --close-branch -m 'prune bad branch'
  $ hg branches -a
  b                              8:eebb944467c9
  a branch name much longer than the default justification used by branches 7:10ff5895aa57
  $ hg up -C b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg commit -d '9 0' --close-branch -m 'close this part branch too'
  $ hg commit -d '9 0' --close-branch -m 're-closing this branch'
  abort: can only close branch heads
  [255]

  $ hg log -r tip --debug
  changeset:   12:e3d49c0575d8fc2cb1cd6859c747c14f5f6d499f
  branch:      b
  tag:         tip
  phase:       draft
  parent:      8:eebb944467c9fb9651ed232aeaf31b3c0a7fc6c1
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    8:6f9ed32d2b310e391a4f107d5f0f071df785bfee
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  extra:       branch=b
  extra:       close=1
  description:
  close this part branch too
  
  
--- b branch should be inactive

  $ hg branches
  a branch name much longer than the default justification used by branches 7:10ff5895aa57
  c                              6:589736a22561 (inactive)
  a                              5:d8cbc61dbaa6 (inactive)
  default                        0:19709c5a4e75 (inactive)
  $ hg branches -c
  a branch name much longer than the default justification used by branches 7:10ff5895aa57
  b                             12:e3d49c0575d8 (closed)
  c                              6:589736a22561 (inactive)
  a                              5:d8cbc61dbaa6 (inactive)
  default                        0:19709c5a4e75 (inactive)
  $ hg branches -a
  a branch name much longer than the default justification used by branches 7:10ff5895aa57
  $ hg branches -q
  a branch name much longer than the default justification used by branches
  c
  a
  default
  $ hg heads b
  no open branch heads found on branches b
  [1]
  $ hg heads --closed b
  changeset:   12:e3d49c0575d8
  branch:      b
  tag:         tip
  parent:      8:eebb944467c9
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     close this part branch too
  
  changeset:   11:d3f163457ebf
  branch:      b
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     prune bad branch
  
  $ echo 'xxx4' >> b
  $ hg commit -d '9 0' -m 'reopen branch with a change'
  reopening closed branch head 12

--- branch b is back in action

  $ hg branches -a
  b                             13:e23b5505d1ad
  a branch name much longer than the default justification used by branches 7:10ff5895aa57

---- test heads listings

  $ hg heads
  changeset:   13:e23b5505d1ad
  branch:      b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     reopen branch with a change
  
  changeset:   7:10ff5895aa57
  branch:      a branch name much longer than the default justification used by branches
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     Adding d branch
  
  changeset:   6:589736a22561
  branch:      c
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     Adding c branch
  
  changeset:   5:d8cbc61dbaa6
  branch:      a
  parent:      2:881fe2b92ad0
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     Adding b branch head 2
  
  changeset:   0:19709c5a4e75
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Adding root node
  

branch default

  $ hg heads default
  changeset:   0:19709c5a4e75
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Adding root node
  

branch a

  $ hg heads a
  changeset:   5:d8cbc61dbaa6
  branch:      a
  parent:      2:881fe2b92ad0
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     Adding b branch head 2
  
  $ hg heads --active a
  no open branch heads found on branches a
  [1]

branch b

  $ hg heads b
  changeset:   13:e23b5505d1ad
  branch:      b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     reopen branch with a change
  
  $ hg heads --closed b
  changeset:   13:e23b5505d1ad
  branch:      b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     reopen branch with a change
  
  changeset:   11:d3f163457ebf
  branch:      b
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     prune bad branch
  
default branch colors:

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > color =
  > [color]
  > mode = ansi
  > EOF

  $ hg up -C c
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg commit -d '9 0' --close-branch -m 'reclosing this branch'
  $ hg up -C b
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg branches --color=always
  \x1b[0;32mb\x1b[0m\x1b[0;33m                             13:e23b5505d1ad\x1b[0m (esc)
  \x1b[0;0ma branch name much longer than the default justification used by branches\x1b[0m\x1b[0;33m 7:10ff5895aa57\x1b[0m (esc)
  \x1b[0;0ma\x1b[0m\x1b[0;33m                              5:d8cbc61dbaa6\x1b[0m (inactive) (esc)
  \x1b[0;0mdefault\x1b[0m\x1b[0;33m                        0:19709c5a4e75\x1b[0m (inactive) (esc)

default closed branch color:

  $ hg branches --color=always --closed
  \x1b[0;32mb\x1b[0m\x1b[0;33m                             13:e23b5505d1ad\x1b[0m (esc)
  \x1b[0;0ma branch name much longer than the default justification used by branches\x1b[0m\x1b[0;33m 7:10ff5895aa57\x1b[0m (esc)
  \x1b[0;30;1mc\x1b[0m\x1b[0;33m                             14:f894c25619d3\x1b[0m (closed) (esc)
  \x1b[0;0ma\x1b[0m\x1b[0;33m                              5:d8cbc61dbaa6\x1b[0m (inactive) (esc)
  \x1b[0;0mdefault\x1b[0m\x1b[0;33m                        0:19709c5a4e75\x1b[0m (inactive) (esc)

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > color =
  > [color]
  > branches.active = green
  > branches.closed = blue
  > branches.current = red
  > branches.inactive = magenta
  > log.changeset = cyan
  > EOF

custom branch colors:

  $ hg branches --color=always
  \x1b[0;31mb\x1b[0m\x1b[0;36m                             13:e23b5505d1ad\x1b[0m (esc)
  \x1b[0;32ma branch name much longer than the default justification used by branches\x1b[0m\x1b[0;36m 7:10ff5895aa57\x1b[0m (esc)
  \x1b[0;35ma\x1b[0m\x1b[0;36m                              5:d8cbc61dbaa6\x1b[0m (inactive) (esc)
  \x1b[0;35mdefault\x1b[0m\x1b[0;36m                        0:19709c5a4e75\x1b[0m (inactive) (esc)

custom closed branch color:

  $ hg branches --color=always --closed
  \x1b[0;31mb\x1b[0m\x1b[0;36m                             13:e23b5505d1ad\x1b[0m (esc)
  \x1b[0;32ma branch name much longer than the default justification used by branches\x1b[0m\x1b[0;36m 7:10ff5895aa57\x1b[0m (esc)
  \x1b[0;34mc\x1b[0m\x1b[0;36m                             14:f894c25619d3\x1b[0m (closed) (esc)
  \x1b[0;35ma\x1b[0m\x1b[0;36m                              5:d8cbc61dbaa6\x1b[0m (inactive) (esc)
  \x1b[0;35mdefault\x1b[0m\x1b[0;36m                        0:19709c5a4e75\x1b[0m (inactive) (esc)

template output:

  $ hg branches -Tjson --closed
  [
   {
    "active": true,
    "branch": "b",
    "closed": false,
    "current": true,
    "node": "e23b5505d1ad24aab6f84fd8c7cb8cd8e5e93be0",
    "rev": 13
   },
   {
    "active": true,
    "branch": "a branch name much longer than the default justification used by branches",
    "closed": false,
    "current": false,
    "node": "10ff5895aa5793bd378da574af8cec8ea408d831",
    "rev": 7
   },
   {
    "active": false,
    "branch": "c",
    "closed": true,
    "current": false,
    "node": "f894c25619d3f1484639d81be950e0a07bc6f1f6",
    "rev": 14
   },
   {
    "active": false,
    "branch": "a",
    "closed": false,
    "current": false,
    "node": "d8cbc61dbaa6dc817175d1e301eecb863f280832",
    "rev": 5
   },
   {
    "active": false,
    "branch": "default",
    "closed": false,
    "current": false,
    "node": "19709c5a4e75bf938f8e349aff97438539bb729e",
    "rev": 0
   }
  ]


Tests of revision branch name caching

We rev branch cache is updated automatically. In these tests we use a trick to
trigger rebuilds. We remove the branch head cache and run 'hg head' to cause a
rebuild that also will populate the rev branch cache.

revision branch cache is created when building the branch head cache
  $ rm -rf .hg/cache; hg head a -T '{rev}\n'
  5
  $ f --hexdump --size .hg/cache/rbc-*
  .hg/cache/rbc-names-v1: size=87
  0000: 64 65 66 61 75 6c 74 00 61 00 62 00 63 00 61 20 |default.a.b.c.a |
  0010: 62 72 61 6e 63 68 20 6e 61 6d 65 20 6d 75 63 68 |branch name much|
  0020: 20 6c 6f 6e 67 65 72 20 74 68 61 6e 20 74 68 65 | longer than the|
  0030: 20 64 65 66 61 75 6c 74 20 6a 75 73 74 69 66 69 | default justifi|
  0040: 63 61 74 69 6f 6e 20 75 73 65 64 20 62 79 20 62 |cation used by b|
  0050: 72 61 6e 63 68 65 73                            |ranches|
  .hg/cache/rbc-revs-v1: size=120
  0000: 19 70 9c 5a 00 00 00 00 dd 6b 44 0d 00 00 00 01 |.p.Z.....kD.....|
  0010: 88 1f e2 b9 00 00 00 01 ac 22 03 33 00 00 00 02 |.........".3....|
  0020: ae e3 9c d1 00 00 00 02 d8 cb c6 1d 00 00 00 01 |................|
  0030: 58 97 36 a2 00 00 00 03 10 ff 58 95 00 00 00 04 |X.6.......X.....|
  0040: ee bb 94 44 00 00 00 02 5f 40 61 bb 00 00 00 02 |...D...._@a.....|
  0050: bf be 84 1b 00 00 00 02 d3 f1 63 45 80 00 00 02 |..........cE....|
  0060: e3 d4 9c 05 80 00 00 02 e2 3b 55 05 00 00 00 02 |.........;U.....|
  0070: f8 94 c2 56 80 00 00 03                         |...V....|

#if unix-permissions no-root
no errors when revbranchcache is not writable

  $ echo >> .hg/cache/rbc-revs-v1
  $ chmod a-w .hg/cache/rbc-revs-v1
  $ rm -f .hg/cache/branch* && hg head a -T '{rev}\n'
  5
  $ chmod a+w .hg/cache/rbc-revs-v1
#endif

recovery from invalid cache revs file with trailing data
  $ echo >> .hg/cache/rbc-revs-v1
  $ rm -f .hg/cache/branch* && hg head a -T '{rev}\n' --debug
  5
  truncating cache/rbc-revs-v1 to 120
  $ f --size .hg/cache/rbc-revs*
  .hg/cache/rbc-revs-v1: size=120
recovery from invalid cache file with partial last record
  $ mv .hg/cache/rbc-revs-v1 .
  $ f -qDB 119 rbc-revs-v1 > .hg/cache/rbc-revs-v1
  $ f --size .hg/cache/rbc-revs*
  .hg/cache/rbc-revs-v1: size=119
  $ rm -f .hg/cache/branch* && hg head a -T '{rev}\n' --debug
  5
  truncating cache/rbc-revs-v1 to 112
  $ f --size .hg/cache/rbc-revs*
  .hg/cache/rbc-revs-v1: size=120
recovery from invalid cache file with missing record - no truncation
  $ mv .hg/cache/rbc-revs-v1 .
  $ f -qDB 112 rbc-revs-v1 > .hg/cache/rbc-revs-v1
  $ rm -f .hg/cache/branch* && hg head a -T '{rev}\n' --debug
  5
  $ f --size .hg/cache/rbc-revs*
  .hg/cache/rbc-revs-v1: size=120
recovery from invalid cache file with some bad records
  $ mv .hg/cache/rbc-revs-v1 .
  $ f -qDB 8 rbc-revs-v1 > .hg/cache/rbc-revs-v1
  $ f --size .hg/cache/rbc-revs*
  .hg/cache/rbc-revs-v1: size=8
  $ f -qDB 112 rbc-revs-v1 >> .hg/cache/rbc-revs-v1
  $ f --size .hg/cache/rbc-revs*
  .hg/cache/rbc-revs-v1: size=120
  $ hg log -r 'branch(.)' -T '{rev} ' --debug
  3 4 8 9 10 11 12 13 truncating cache/rbc-revs-v1 to 8
  $ rm -f .hg/cache/branch* && hg head a -T '{rev}\n' --debug
  5
  truncating cache/rbc-revs-v1 to 104
  $ f --size --hexdump --bytes=16 .hg/cache/rbc-revs*
  .hg/cache/rbc-revs-v1: size=120
  0000: 19 70 9c 5a 00 00 00 00 dd 6b 44 0d 00 00 00 01 |.p.Z.....kD.....|
cache is updated when committing
  $ hg branch i-will-regret-this
  marked working directory as branch i-will-regret-this
  $ hg ci -m regrets
  $ f --size .hg/cache/rbc-*
  .hg/cache/rbc-names-v1: size=106
  .hg/cache/rbc-revs-v1: size=128
update after rollback - the cache will be correct but rbc-names will will still
contain the branch name even though it no longer is used
  $ hg up -qr '.^'
  $ hg rollback -qf
  $ f --size --hexdump .hg/cache/rbc-*
  .hg/cache/rbc-names-v1: size=106
  0000: 64 65 66 61 75 6c 74 00 61 00 62 00 63 00 61 20 |default.a.b.c.a |
  0010: 62 72 61 6e 63 68 20 6e 61 6d 65 20 6d 75 63 68 |branch name much|
  0020: 20 6c 6f 6e 67 65 72 20 74 68 61 6e 20 74 68 65 | longer than the|
  0030: 20 64 65 66 61 75 6c 74 20 6a 75 73 74 69 66 69 | default justifi|
  0040: 63 61 74 69 6f 6e 20 75 73 65 64 20 62 79 20 62 |cation used by b|
  0050: 72 61 6e 63 68 65 73 00 69 2d 77 69 6c 6c 2d 72 |ranches.i-will-r|
  0060: 65 67 72 65 74 2d 74 68 69 73                   |egret-this|
  .hg/cache/rbc-revs-v1: size=120
  0000: 19 70 9c 5a 00 00 00 00 dd 6b 44 0d 00 00 00 01 |.p.Z.....kD.....|
  0010: 88 1f e2 b9 00 00 00 01 ac 22 03 33 00 00 00 02 |.........".3....|
  0020: ae e3 9c d1 00 00 00 02 d8 cb c6 1d 00 00 00 01 |................|
  0030: 58 97 36 a2 00 00 00 03 10 ff 58 95 00 00 00 04 |X.6.......X.....|
  0040: ee bb 94 44 00 00 00 02 5f 40 61 bb 00 00 00 02 |...D...._@a.....|
  0050: bf be 84 1b 00 00 00 02 d3 f1 63 45 80 00 00 02 |..........cE....|
  0060: e3 d4 9c 05 80 00 00 02 e2 3b 55 05 00 00 00 02 |.........;U.....|
  0070: f8 94 c2 56 80 00 00 03                         |...V....|
cache is updated/truncated when stripping - it is thus very hard to get in a
situation where the cache is out of sync and the hash check detects it
  $ hg --config extensions.strip= strip -r tip --nob
  $ f --size .hg/cache/rbc-revs*
  .hg/cache/rbc-revs-v1: size=112

  $ cd ..
