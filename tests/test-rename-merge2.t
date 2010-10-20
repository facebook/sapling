
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
  >     mkdir t
  >     cd t
  >     hg init
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
    all copies found (* = to merge, ! = divergent):
     b -> a *
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local e300d1c794ec+ remote 4ce40f5aca24
   rev: versions differ -> m
   a: remote copied to b -> m
  preserving a for resolve of b
  preserving rev for resolve of rev
  updating: a 1/2 files (50.00%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging a and b to b
  my b@e300d1c794ec+ other b@4ce40f5aca24 ancestor a@924404dff337
   premerge successful
  updating: rev 2/2 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@e300d1c794ec+ other rev@4ce40f5aca24 ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     b -> a *
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 86a2aa42fc76+ remote f4db7e329e71
   a: remote is newer -> g
   b: local copied/moved to a -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: a 1/3 files (33.33%)
  getting a
  updating: b 2/3 files (66.67%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b and a to b
  my b@86a2aa42fc76+ other a@f4db7e329e71 ancestor a@924404dff337
   premerge successful
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@86a2aa42fc76+ other rev@f4db7e329e71 ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     b -> a *
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local e300d1c794ec+ remote bdb19105162a
   rev: versions differ -> m
   a: remote moved to b -> m
  preserving a for resolve of b
  preserving rev for resolve of rev
  removing a
  updating: a 1/2 files (50.00%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging a and b to b
  my b@e300d1c794ec+ other b@bdb19105162a ancestor a@924404dff337
   premerge successful
  updating: rev 2/2 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@e300d1c794ec+ other rev@bdb19105162a ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     b -> a *
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 02963e448370+ remote f4db7e329e71
   b: local copied/moved to a -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: b 1/2 files (50.00%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b and a to b
  my b@02963e448370+ other a@f4db7e329e71 ancestor a@924404dff337
   premerge successful
  updating: rev 2/2 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@02963e448370+ other rev@f4db7e329e71 ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     b -> a 
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 94b33a1b7f2d+ remote 4ce40f5aca24
   rev: versions differ -> m
   b: remote created -> g
  preserving rev for resolve of rev
  updating: b 1/2 files (50.00%)
  getting b
  updating: rev 2/2 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@94b33a1b7f2d+ other rev@4ce40f5aca24 ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     b -> a 
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 86a2aa42fc76+ remote 97c705ade336
   rev: versions differ -> m
  preserving rev for resolve of rev
  updating: rev 1/1 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@86a2aa42fc76+ other rev@97c705ade336 ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     b -> a 
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 94b33a1b7f2d+ remote bdb19105162a
   a: other deleted -> r
   rev: versions differ -> m
   b: remote created -> g
  preserving rev for resolve of rev
  updating: a 1/3 files (33.33%)
  removing a
  updating: b 2/3 files (66.67%)
  getting b
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@94b33a1b7f2d+ other rev@bdb19105162a ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     b -> a 
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 02963e448370+ remote 97c705ade336
   rev: versions differ -> m
  preserving rev for resolve of rev
  updating: rev 1/1 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@02963e448370+ other rev@97c705ade336 ancestor rev@924404dff337
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
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 62e7bf090eba+ remote 49b6d8032493
   b: versions differ -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: b 1/2 files (50.00%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b
  my b@62e7bf090eba+ other b@49b6d8032493 ancestor a@924404dff337
  updating: rev 2/2 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@62e7bf090eba+ other rev@49b6d8032493 ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     c -> a !
     b -> a !
    checking for directory renames
   a: divergent renames -> dr
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 02963e448370+ remote fe905ef2c33e
   rev: versions differ -> m
   c: remote created -> g
  preserving rev for resolve of rev
  updating: a 1/3 files (33.33%)
  note: possible conflict - a was renamed multiple times to:
   b
   c
  updating: c 2/3 files (66.67%)
  getting c
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@02963e448370+ other rev@fe905ef2c33e ancestor rev@924404dff337
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
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 86a2aa42fc76+ remote af30c7647fc7
   b: versions differ -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: b 1/2 files (50.00%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b
  my b@86a2aa42fc76+ other b@af30c7647fc7 ancestor b@000000000000
  updating: rev 2/2 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@86a2aa42fc76+ other rev@af30c7647fc7 ancestor rev@924404dff337
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
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 59318016310c+ remote bdb19105162a
   a: other deleted -> r
   b: versions differ -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: a 1/3 files (33.33%)
  removing a
  updating: b 2/3 files (66.67%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b
  my b@59318016310c+ other b@bdb19105162a ancestor b@000000000000
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@59318016310c+ other rev@bdb19105162a ancestor rev@924404dff337
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
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 86a2aa42fc76+ remote 8dbce441892a
   a: remote is newer -> g
   b: versions differ -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: a 1/3 files (33.33%)
  getting a
  updating: b 2/3 files (66.67%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b
  my b@86a2aa42fc76+ other b@8dbce441892a ancestor b@000000000000
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@86a2aa42fc76+ other rev@8dbce441892a ancestor rev@924404dff337
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
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 59318016310c+ remote bdb19105162a
   a: other deleted -> r
   b: versions differ -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: a 1/3 files (33.33%)
  removing a
  updating: b 2/3 files (66.67%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b
  my b@59318016310c+ other b@bdb19105162a ancestor b@000000000000
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@59318016310c+ other rev@bdb19105162a ancestor rev@924404dff337
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
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 86a2aa42fc76+ remote 8dbce441892a
   a: remote is newer -> g
   b: versions differ -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: a 1/3 files (33.33%)
  getting a
  updating: b 2/3 files (66.67%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b
  my b@86a2aa42fc76+ other b@8dbce441892a ancestor b@000000000000
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@86a2aa42fc76+ other rev@8dbce441892a ancestor rev@924404dff337
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
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 0b76e65c8289+ remote 4ce40f5aca24
   b: versions differ -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: b 1/2 files (50.00%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b
  my b@0b76e65c8289+ other b@4ce40f5aca24 ancestor b@000000000000
  updating: rev 2/2 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@0b76e65c8289+ other rev@4ce40f5aca24 ancestor rev@924404dff337
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
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 02963e448370+ remote 8dbce441892a
   b: versions differ -> m
   rev: versions differ -> m
  remote changed a which local deleted
  use (c)hanged version or leave (d)eleted? c
   a: prompt recreating -> g
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: a 1/3 files (33.33%)
  getting a
  updating: b 2/3 files (66.67%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b
  my b@02963e448370+ other b@8dbce441892a ancestor b@000000000000
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@02963e448370+ other rev@8dbce441892a ancestor rev@924404dff337
  1 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M a
  M b
  --------------
  
  $ tm "up a b" "nm a b" "      " "19 merge b no ancestor, prompt remove a"
  created new head
  --------------
  test L:up a b R:nm a b W:       - 19 merge b no ancestor, prompt remove a
  --------------
    searching for copies back to rev 1
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 0b76e65c8289+ remote bdb19105162a
   local changed a which remote deleted
  use (c)hanged version or (d)elete? c
   a: prompt keep -> a
   b: versions differ -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: a 1/3 files (33.33%)
  updating: b 2/3 files (66.67%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b
  my b@0b76e65c8289+ other b@bdb19105162a ancestor b@000000000000
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@0b76e65c8289+ other rev@bdb19105162a ancestor rev@924404dff337
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
  C a
  --------------
  
  $ tm "up a  " "um a b" "      " "20 merge a and b to b, remove a"
  created new head
  --------------
  test L:up a   R:um a b W:       - 20 merge a and b to b, remove a
  --------------
    searching for copies back to rev 1
    unmatched files in other:
     b
    all copies found (* = to merge, ! = divergent):
     b -> a *
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local e300d1c794ec+ remote 49b6d8032493
   rev: versions differ -> m
   a: remote moved to b -> m
  preserving a for resolve of b
  preserving rev for resolve of rev
  removing a
  updating: a 1/2 files (50.00%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging a and b to b
  my b@e300d1c794ec+ other b@49b6d8032493 ancestor a@924404dff337
  updating: rev 2/2 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@e300d1c794ec+ other rev@49b6d8032493 ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     b -> a *
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 62e7bf090eba+ remote f4db7e329e71
   b: local copied/moved to a -> m
   rev: versions differ -> m
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: b 1/2 files (50.00%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b and a to b
  my b@62e7bf090eba+ other a@f4db7e329e71 ancestor a@924404dff337
  updating: rev 2/2 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@62e7bf090eba+ other rev@f4db7e329e71 ancestor rev@924404dff337
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
    all copies found (* = to merge, ! = divergent):
     b -> a *
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 924404dff337 local 02963e448370+ remote 2b958612230f
   b: local copied/moved to a -> m
   rev: versions differ -> m
   c: remote created -> g
  preserving b for resolve of b
  preserving rev for resolve of rev
  updating: b 1/3 files (33.33%)
  picked tool 'python ../merge' for b (binary False symlink False)
  merging b and a to b
  my b@02963e448370+ other a@2b958612230f ancestor a@924404dff337
   premerge successful
  updating: c 2/3 files (66.67%)
  getting c
  updating: rev 3/3 files (100.00%)
  picked tool 'python ../merge' for rev (binary False symlink False)
  merging rev
  my rev@02963e448370+ other rev@2b958612230f ancestor rev@924404dff337
  1 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  --------------
  M b
    a
  M c
  --------------
  
