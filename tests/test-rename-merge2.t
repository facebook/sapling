
  $ mkdir -p t
  $ cd t
  $ cat <<EOF > merge
  > import sys, os
  > f = open(sys.argv[1], "wb")
  > f.write("merge %s %s %s" % (sys.argv[1], sys.argv[2], sys.argv[3]))
  > f.close()
  > EOF

perform a test merge with possible renaming
args:
$1 = action in local branch
$2 = action in remote branch
$3 = action in working dir
$4 = expected result

  $ tm()
  > {
  >     hg init t
  >     cd t
  >     echo "[merge]" >> .hg/hgrc
  >     echo "followcopies = 1" >> .hg/hgrc
  > 
  >     # base
  >     echo base > a
  >     echo base > rev # used to force commits
  >     hg add a rev
  >     hg ci -m "base"
  > 
  >     # remote
  >     echo remote > rev
  >     if [ "$2" != "" ] ; then $2 ; fi
  >     hg ci -m "remote"
  > 
  >     # local
  >     hg co -q 0
  >     echo local > rev
  >     if [ "$1" != "" ] ; then $1 ; fi
  >     hg ci -m "local"
  > 
  >     # working dir
  >     echo local > rev
  >     if [ "$3" != "" ] ; then $3 ; fi
  > 
  >     # merge
  >     echo "--------------"
  >     echo "test L:$1 R:$2 W:$3 - $4"
  >     echo "--------------"
  >     hg merge -y --debug --traceback --tool="python ../merge"
  > 
  >     echo "--------------"
  >     hg status -camC -X rev
  > 
  >     hg ci -m "merge"
  > 
  >     echo "--------------"
  >     echo
  > 
  >     cd ..
  >     rm -r t
  > }
  $ up() {
  >     cp rev $1
  >     hg add $1 2> /dev/null
  >     if [ "$2" != "" ] ; then
  >         cp rev $2
  >         hg add $2 2> /dev/null
  >     fi
  > }
  $ uc() { up $1; hg cp $1 $2; } # update + copy
  $ um() { up $1; hg mv $1 $2; }
  $ nc() { hg cp $1 $2; } # just copy
  $ nm() { hg mv $1 $2; } # just move
  $ tm "up a  " "nc a b" "      " "1  get local a to b"
  created new head
  --------------
  test L:up a   R:nc a b W:       - 1  get local a to b
  --------------
    searching for copies back to rev 1
    unmatched files in other:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: e300d1c794ec+, remote: 4ce40f5aca24
   preserving a for resolve of b
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   a: remote unchanged -> k
   b: remote copied from a -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging a and b to b
  my b@e300d1c794ec+ other b@4ce40f5aca24 ancestor a@924404dff337
   premerge successful
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@e300d1c794ec+ other rev@4ce40f5aca24 ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@e300d1c794ec+ other rev@4ce40f5aca24 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
    a
  C a
  --------------
  
  $ tm "nc a b" "up a  " "      " "2  get rem change to a and b"
  created new head
  --------------
  test L:nc a b R:up a   W:       - 2  get rem change to a and b
  --------------
    searching for copies back to rev 1
    unmatched files in local:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 86a2aa42fc76+, remote: f4db7e329e71
   preserving b for resolve of b
   preserving rev for resolve of rev
   a: remote is newer -> g
  getting a
   b: local copied/moved from a -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b and a to b
  my b@86a2aa42fc76+ other a@f4db7e329e71 ancestor a@924404dff337
   premerge successful
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@86a2aa42fc76+ other rev@f4db7e329e71 ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@86a2aa42fc76+ other rev@f4db7e329e71 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  1 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M a
  M b
    a
  --------------
  
  $ tm "up a  " "nm a b" "      " "3  get local a change to b, remove a"
  created new head
  --------------
  test L:up a   R:nm a b W:       - 3  get local a change to b, remove a
  --------------
    searching for copies back to rev 1
    unmatched files in other:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: e300d1c794ec+, remote: bdb19105162a
   preserving a for resolve of b
   preserving rev for resolve of rev
  removing a
  starting 4 threads for background file closing (?)
   b: remote moved from a -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging a and b to b
  my b@e300d1c794ec+ other b@bdb19105162a ancestor a@924404dff337
   premerge successful
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@e300d1c794ec+ other rev@bdb19105162a ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@e300d1c794ec+ other rev@bdb19105162a ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
    a
  --------------
  
  $ tm "nm a b" "up a  " "      " "4  get remote change to b"
  created new head
  --------------
  test L:nm a b R:up a   W:       - 4  get remote change to b
  --------------
    searching for copies back to rev 1
    unmatched files in local:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 02963e448370+, remote: f4db7e329e71
   preserving b for resolve of b
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   b: local copied/moved from a -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b and a to b
  my b@02963e448370+ other a@f4db7e329e71 ancestor a@924404dff337
   premerge successful
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@02963e448370+ other rev@f4db7e329e71 ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@02963e448370+ other rev@f4db7e329e71 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
    a
  --------------
  
  $ tm "      " "nc a b" "      " "5  get b"
  created new head
  --------------
  test L:       R:nc a b W:       - 5  get b
  --------------
    searching for copies back to rev 1
    unmatched files in other:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' 
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 94b33a1b7f2d+, remote: 4ce40f5aca24
   preserving rev for resolve of rev
   b: remote created -> g
  getting b
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@94b33a1b7f2d+ other rev@4ce40f5aca24 ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@94b33a1b7f2d+ other rev@4ce40f5aca24 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
  C a
  --------------
  
  $ tm "nc a b" "      " "      " "6  nothing"
  created new head
  --------------
  test L:nc a b R:       W:       - 6  nothing
  --------------
    searching for copies back to rev 1
    unmatched files in local:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' 
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 86a2aa42fc76+, remote: 97c705ade336
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@86a2aa42fc76+ other rev@97c705ade336 ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@86a2aa42fc76+ other rev@97c705ade336 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  C a
  C b
  --------------
  
  $ tm "      " "nm a b" "      " "7  get b"
  created new head
  --------------
  test L:       R:nm a b W:       - 7  get b
  --------------
    searching for copies back to rev 1
    unmatched files in other:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' 
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 94b33a1b7f2d+, remote: bdb19105162a
   preserving rev for resolve of rev
   a: other deleted -> r
  removing a
   b: remote created -> g
  getting b
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@94b33a1b7f2d+ other rev@bdb19105162a ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@94b33a1b7f2d+ other rev@bdb19105162a ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  1 files updated, 1 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
  --------------
  
  $ tm "nm a b" "      " "      " "8  nothing"
  created new head
  --------------
  test L:nm a b R:       W:       - 8  nothing
  --------------
    searching for copies back to rev 1
    unmatched files in local:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' 
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 02963e448370+, remote: 97c705ade336
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@02963e448370+ other rev@97c705ade336 ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@02963e448370+ other rev@97c705ade336 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  C b
  --------------
  
  $ tm "um a b" "um a b" "      " "9  do merge with ancestor in a"
  created new head
  --------------
  test L:um a b R:um a b W:       - 9  do merge with ancestor in a
  --------------
    searching for copies back to rev 1
    unmatched files new in both:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 62e7bf090eba+, remote: 49b6d8032493
   preserving b for resolve of b
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   b: both renamed from a -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b
  my b@62e7bf090eba+ other b@49b6d8032493 ancestor a@924404dff337
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@62e7bf090eba+ other rev@49b6d8032493 ancestor rev@924404dff337
   b: both renamed from a -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@62e7bf090eba+ other b@49b6d8032493 ancestor a@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@62e7bf090eba+ other rev@49b6d8032493 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
  --------------
  

m "um a c" "um x c" "      " "10 do merge with no ancestor"

  $ tm "nm a b" "nm a c" "      " "11 get c, keep b"
  created new head
  --------------
  test L:nm a b R:nm a c W:       - 11 get c, keep b
  --------------
    searching for copies back to rev 1
    unmatched files in local:
     b
    unmatched files in other:
     c
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' !
     src: 'a' -> dst: 'c' !
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 02963e448370+, remote: fe905ef2c33e
  note: possible conflict - a was renamed multiple times to:
   b
   c
   preserving rev for resolve of rev
   c: remote created -> g
  getting c
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@02963e448370+ other rev@fe905ef2c33e ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@02963e448370+ other rev@fe905ef2c33e ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M c
  C b
  --------------
  
  $ tm "nc a b" "up b  " "      " "12 merge b no ancestor"
  created new head
  --------------
  test L:nc a b R:up b   W:       - 12 merge b no ancestor
  --------------
    searching for copies back to rev 1
    unmatched files new in both:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 86a2aa42fc76+, remote: af30c7647fc7
   preserving b for resolve of b
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   b: both created -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b
  my b@86a2aa42fc76+ other b@af30c7647fc7 ancestor b@000000000000
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@86a2aa42fc76+ other rev@af30c7647fc7 ancestor rev@924404dff337
   b: both created -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@86a2aa42fc76+ other b@af30c7647fc7 ancestor b@000000000000
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@86a2aa42fc76+ other rev@af30c7647fc7 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
  C a
  --------------
  
  $ tm "up b  " "nm a b" "      " "13 merge b no ancestor"
  created new head
  --------------
  test L:up b   R:nm a b W:       - 13 merge b no ancestor
  --------------
    searching for copies back to rev 1
    unmatched files new in both:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 59318016310c+, remote: bdb19105162a
   preserving b for resolve of b
   preserving rev for resolve of rev
   a: other deleted -> r
  removing a
  starting 4 threads for background file closing (?)
   b: both created -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b
  my b@59318016310c+ other b@bdb19105162a ancestor b@000000000000
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@59318016310c+ other rev@bdb19105162a ancestor rev@924404dff337
   b: both created -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@59318016310c+ other b@bdb19105162a ancestor b@000000000000
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@59318016310c+ other rev@bdb19105162a ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
  --------------
  
  $ tm "nc a b" "up a b" "      " "14 merge b no ancestor"
  created new head
  --------------
  test L:nc a b R:up a b W:       - 14 merge b no ancestor
  --------------
    searching for copies back to rev 1
    unmatched files new in both:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 86a2aa42fc76+, remote: 8dbce441892a
   preserving b for resolve of b
   preserving rev for resolve of rev
   a: remote is newer -> g
  getting a
   b: both created -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b
  my b@86a2aa42fc76+ other b@8dbce441892a ancestor b@000000000000
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@86a2aa42fc76+ other rev@8dbce441892a ancestor rev@924404dff337
   b: both created -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@86a2aa42fc76+ other b@8dbce441892a ancestor b@000000000000
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@86a2aa42fc76+ other rev@8dbce441892a ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  1 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M a
  M b
  --------------
  
  $ tm "up b  " "nm a b" "      " "15 merge b no ancestor, remove a"
  created new head
  --------------
  test L:up b   R:nm a b W:       - 15 merge b no ancestor, remove a
  --------------
    searching for copies back to rev 1
    unmatched files new in both:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 59318016310c+, remote: bdb19105162a
   preserving b for resolve of b
   preserving rev for resolve of rev
   a: other deleted -> r
  removing a
  starting 4 threads for background file closing (?)
   b: both created -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b
  my b@59318016310c+ other b@bdb19105162a ancestor b@000000000000
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@59318016310c+ other rev@bdb19105162a ancestor rev@924404dff337
   b: both created -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@59318016310c+ other b@bdb19105162a ancestor b@000000000000
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@59318016310c+ other rev@bdb19105162a ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
  --------------
  
  $ tm "nc a b" "up a b" "      " "16 get a, merge b no ancestor"
  created new head
  --------------
  test L:nc a b R:up a b W:       - 16 get a, merge b no ancestor
  --------------
    searching for copies back to rev 1
    unmatched files new in both:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 86a2aa42fc76+, remote: 8dbce441892a
   preserving b for resolve of b
   preserving rev for resolve of rev
   a: remote is newer -> g
  getting a
   b: both created -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b
  my b@86a2aa42fc76+ other b@8dbce441892a ancestor b@000000000000
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@86a2aa42fc76+ other rev@8dbce441892a ancestor rev@924404dff337
   b: both created -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@86a2aa42fc76+ other b@8dbce441892a ancestor b@000000000000
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@86a2aa42fc76+ other rev@8dbce441892a ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  1 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M a
  M b
  --------------
  
  $ tm "up a b" "nc a b" "      " "17 keep a, merge b no ancestor"
  created new head
  --------------
  test L:up a b R:nc a b W:       - 17 keep a, merge b no ancestor
  --------------
    searching for copies back to rev 1
    unmatched files new in both:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 0b76e65c8289+, remote: 4ce40f5aca24
   preserving b for resolve of b
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   a: remote unchanged -> k
   b: both created -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b
  my b@0b76e65c8289+ other b@4ce40f5aca24 ancestor b@000000000000
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@0b76e65c8289+ other rev@4ce40f5aca24 ancestor rev@924404dff337
   b: both created -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@0b76e65c8289+ other b@4ce40f5aca24 ancestor b@000000000000
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@0b76e65c8289+ other rev@4ce40f5aca24 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
  C a
  --------------
  
  $ tm "nm a b" "up a b" "      " "18 merge b no ancestor"
  created new head
  --------------
  test L:nm a b R:up a b W:       - 18 merge b no ancestor
  --------------
    searching for copies back to rev 1
    unmatched files new in both:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 02963e448370+, remote: 8dbce441892a
   preserving b for resolve of b
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   a: prompt deleted/changed -> m (premerge)
  picked tool ':prompt' for a (binary False symlink False changedelete True)
  remote changed a which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
   b: both created -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b
  my b@02963e448370+ other b@8dbce441892a ancestor b@000000000000
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@02963e448370+ other rev@8dbce441892a ancestor rev@924404dff337
   b: both created -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@02963e448370+ other b@8dbce441892a ancestor b@000000000000
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@02963e448370+ other rev@8dbce441892a ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  --------------
  M a
  M b
  abort: unresolved merge conflicts (see "hg help resolve")
  --------------
  
  $ tm "up a b" "nm a b" "      " "19 merge b no ancestor, prompt remove a"
  created new head
  --------------
  test L:up a b R:nm a b W:       - 19 merge b no ancestor, prompt remove a
  --------------
    searching for copies back to rev 1
    unmatched files new in both:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 0b76e65c8289+, remote: bdb19105162a
   preserving a for resolve of a
   preserving b for resolve of b
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   a: prompt changed/deleted -> m (premerge)
  picked tool ':prompt' for a (binary False symlink False changedelete True)
  local changed a which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
   b: both created -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b
  my b@0b76e65c8289+ other b@bdb19105162a ancestor b@000000000000
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@0b76e65c8289+ other rev@bdb19105162a ancestor rev@924404dff337
   b: both created -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@0b76e65c8289+ other b@bdb19105162a ancestor b@000000000000
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@0b76e65c8289+ other rev@bdb19105162a ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  --------------
  M b
  C a
  abort: unresolved merge conflicts (see "hg help resolve")
  --------------
  
  $ tm "up a  " "um a b" "      " "20 merge a and b to b, remove a"
  created new head
  --------------
  test L:up a   R:um a b W:       - 20 merge a and b to b, remove a
  --------------
    searching for copies back to rev 1
    unmatched files in other:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: e300d1c794ec+, remote: 49b6d8032493
   preserving a for resolve of b
   preserving rev for resolve of rev
  removing a
  starting 4 threads for background file closing (?)
   b: remote moved from a -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging a and b to b
  my b@e300d1c794ec+ other b@49b6d8032493 ancestor a@924404dff337
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@e300d1c794ec+ other rev@49b6d8032493 ancestor rev@924404dff337
   b: remote moved from a -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@e300d1c794ec+ other b@49b6d8032493 ancestor a@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@e300d1c794ec+ other rev@49b6d8032493 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
    a
  --------------
  
  $ tm "um a b" "up a  " "      " "21 merge a and b to b"
  created new head
  --------------
  test L:um a b R:up a   W:       - 21 merge a and b to b
  --------------
    searching for copies back to rev 1
    unmatched files in local:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 62e7bf090eba+, remote: f4db7e329e71
   preserving b for resolve of b
   preserving rev for resolve of rev
  starting 4 threads for background file closing (?)
   b: local copied/moved from a -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b and a to b
  my b@62e7bf090eba+ other a@f4db7e329e71 ancestor a@924404dff337
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@62e7bf090eba+ other rev@f4db7e329e71 ancestor rev@924404dff337
   b: local copied/moved from a -> m (merge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  my b@62e7bf090eba+ other a@f4db7e329e71 ancestor a@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/b* * * (glob)
  merge tool returned: 0
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@62e7bf090eba+ other rev@f4db7e329e71 ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
    a
  --------------
  

m "nm a b" "um x a" "      " "22 get a, keep b"

  $ tm "nm a b" "up a c" "      " "23 get c, keep b"
  created new head
  --------------
  test L:nm a b R:up a c W:       - 23 get c, keep b
  --------------
    searching for copies back to rev 1
    unmatched files in local:
     b
    unmatched files in other:
     c
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 924404dff337, local: 02963e448370+, remote: 2b958612230f
   preserving b for resolve of b
   preserving rev for resolve of rev
   c: remote created -> g
  getting c
   b: local copied/moved from a -> m (premerge)
  picked tool 'python ../merge' for b (binary False symlink False changedelete False)
  merging b and a to b
  my b@02963e448370+ other a@2b958612230f ancestor a@924404dff337
   premerge successful
   rev: versions differ -> m (premerge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  merging rev
  my rev@02963e448370+ other rev@2b958612230f ancestor rev@924404dff337
   rev: versions differ -> m (merge)
  picked tool 'python ../merge' for rev (binary False symlink False changedelete False)
  my rev@02963e448370+ other rev@2b958612230f ancestor rev@924404dff337
  launching merge tool: python ../merge *$TESTTMP/t/t/rev* * * (glob)
  merge tool returned: 0
  1 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
    a
  M c
  --------------
  

  $ cd ..


Systematic and terse testing of merge merges and ancestor calculation:

Expected result:

\  a  m1  m2  dst
0  -   f   f   f   "versions differ"
1  f   g   g   g   "versions differ"
2  f   f   f   f   "versions differ"
3  f   f   g  f+g  "remote copied to " + f
4  f   f   g   g   "remote moved to " + f
5  f   g   f  f+g  "local copied to " + f2
6  f   g   f   g   "local moved to " + f2
7  -  (f)  f   f   "remote differs from untracked local"
8  f  (f)  f   f   "remote differs from untracked local"

  $ hg init ancestortest
  $ cd ancestortest
  $ for x in 1 2 3 4 5 6 8; do mkdir $x; echo a > $x/f; done
  $ hg ci -Aqm "a"
  $ mkdir 0
  $ touch 0/f
  $ hg mv 1/f 1/g
  $ hg cp 5/f 5/g
  $ hg mv 6/f 6/g
  $ hg rm 8/f
  $ for x in */*; do echo m1 > $x; done
  $ hg ci -Aqm "m1"
  $ hg up -qr0
  $ mkdir 0 7
  $ touch 0/f 7/f
  $ hg mv 1/f 1/g
  $ hg cp 3/f 3/g
  $ hg mv 4/f 4/g
  $ for x in */*; do echo m2 > $x; done
  $ hg ci -Aqm "m2"
  $ hg up -qr1
  $ mkdir 7 8
  $ echo m > 7/f
  $ echo m > 8/f
  $ hg merge -f --tool internal:dump -v --debug -r2 | sed '/^resolving manifests/,$d' 2> /dev/null
    searching for copies back to rev 1
    unmatched files in local:
     5/g
     6/g
    unmatched files in other:
     3/g
     4/g
     7/f
    unmatched files new in both:
     0/f
     1/g
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: '3/f' -> dst: '3/g' *
     src: '4/f' -> dst: '4/g' *
     src: '5/f' -> dst: '5/g' *
     src: '6/f' -> dst: '6/g' *
    checking for directory renames
  $ hg mani
  0/f
  1/g
  2/f
  3/f
  4/f
  5/f
  5/g
  6/g
  $ for f in */*; do echo $f:; cat $f; done
  0/f:
  m1
  0/f.base:
  0/f.local:
  m1
  0/f.orig:
  m1
  0/f.other:
  m2
  1/g:
  m1
  1/g.base:
  a
  1/g.local:
  m1
  1/g.orig:
  m1
  1/g.other:
  m2
  2/f:
  m1
  2/f.base:
  a
  2/f.local:
  m1
  2/f.orig:
  m1
  2/f.other:
  m2
  3/f:
  m1
  3/f.base:
  a
  3/f.local:
  m1
  3/f.orig:
  m1
  3/f.other:
  m2
  3/g:
  m1
  3/g.base:
  a
  3/g.local:
  m1
  3/g.orig:
  m1
  3/g.other:
  m2
  4/g:
  m1
  4/g.base:
  a
  4/g.local:
  m1
  4/g.orig:
  m1
  4/g.other:
  m2
  5/f:
  m1
  5/f.base:
  a
  5/f.local:
  m1
  5/f.orig:
  m1
  5/f.other:
  m2
  5/g:
  m1
  5/g.base:
  a
  5/g.local:
  m1
  5/g.orig:
  m1
  5/g.other:
  m2
  6/g:
  m1
  6/g.base:
  a
  6/g.local:
  m1
  6/g.orig:
  m1
  6/g.other:
  m2
  7/f:
  m
  7/f.base:
  7/f.local:
  m
  7/f.orig:
  m
  7/f.other:
  m2
  8/f:
  m2
  $ cd ..
