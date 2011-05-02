  $ hg init a
  $ cd a
  $ echo 'root' >root
  $ hg add root
  $ hg commit -d '0 0' -m "Adding root node"

  $ echo 'a' >a
  $ hg add a
  $ hg branch a
  marked working directory as branch a
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

  $ hg branch tip
  abort: the name 'tip' is reserved
  [255]
  $ hg branch null
  abort: the name 'null' is reserved
  [255]
  $ hg branch .
  abort: the name '.' is reserved
  [255]

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

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "color =" >> $HGRCPATH
  $ echo "[color]" >> $HGRCPATH
  $ echo "mode = ansi" >> $HGRCPATH

  $ hg up -C c
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg commit -d '9 0' --close-branch -m 'reclosing this branch'
  $ hg up -C b
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg branches --color=always
  \x1b[0;32mb\x1b[0m \x1b[0;33m                            13:e23b5505d1ad\x1b[0m (esc)
  \x1b[0;0ma branch name much longer than the default justification used by branches\x1b[0m \x1b[0;33m7:10ff5895aa57\x1b[0m (esc)
  \x1b[0;0ma\x1b[0m \x1b[0;33m                             5:d8cbc61dbaa6\x1b[0m (inactive) (esc)
  \x1b[0;0mdefault\x1b[0m \x1b[0;33m                       0:19709c5a4e75\x1b[0m (inactive) (esc)

default closed branch color:

  $ hg branches --color=always --closed
  \x1b[0;32mb\x1b[0m \x1b[0;33m                            13:e23b5505d1ad\x1b[0m (esc)
  \x1b[0;0ma branch name much longer than the default justification used by branches\x1b[0m \x1b[0;33m7:10ff5895aa57\x1b[0m (esc)
  \x1b[0;30;1mc\x1b[0m \x1b[0;33m                            14:f894c25619d3\x1b[0m (closed) (esc)
  \x1b[0;0ma\x1b[0m \x1b[0;33m                             5:d8cbc61dbaa6\x1b[0m (inactive) (esc)
  \x1b[0;0mdefault\x1b[0m \x1b[0;33m                       0:19709c5a4e75\x1b[0m (inactive) (esc)

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "color =" >> $HGRCPATH
  $ echo "[color]" >> $HGRCPATH
  $ echo "branches.active = green" >> $HGRCPATH
  $ echo "branches.closed = blue" >> $HGRCPATH
  $ echo "branches.current = red" >> $HGRCPATH
  $ echo "branches.inactive = magenta" >> $HGRCPATH
  $ echo "log.changeset = cyan" >> $HGRCPATH

custom branch colors:

  $ hg branches --color=always
  \x1b[0;31mb\x1b[0m \x1b[0;36m                            13:e23b5505d1ad\x1b[0m (esc)
  \x1b[0;32ma branch name much longer than the default justification used by branches\x1b[0m \x1b[0;36m7:10ff5895aa57\x1b[0m (esc)
  \x1b[0;35ma\x1b[0m \x1b[0;36m                             5:d8cbc61dbaa6\x1b[0m (inactive) (esc)
  \x1b[0;35mdefault\x1b[0m \x1b[0;36m                       0:19709c5a4e75\x1b[0m (inactive) (esc)

custom closed branch color:

  $ hg branches --color=always --closed
  \x1b[0;31mb\x1b[0m \x1b[0;36m                            13:e23b5505d1ad\x1b[0m (esc)
  \x1b[0;32ma branch name much longer than the default justification used by branches\x1b[0m \x1b[0;36m7:10ff5895aa57\x1b[0m (esc)
  \x1b[0;34mc\x1b[0m \x1b[0;36m                            14:f894c25619d3\x1b[0m (closed) (esc)
  \x1b[0;35ma\x1b[0m \x1b[0;36m                             5:d8cbc61dbaa6\x1b[0m (inactive) (esc)
  \x1b[0;35mdefault\x1b[0m \x1b[0;36m                       0:19709c5a4e75\x1b[0m (inactive) (esc)
