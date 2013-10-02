
  $ HGMERGE=true; export HGMERGE
  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'graphlog =' >> $HGRCPATH
  $ echo 'convert =' >> $HGRCPATH
  $ glog()
  > {
  >     hg glog --template '{rev} "{desc}" files: {files}\n' "$@"
  > }
  $ hg init source
  $ cd source
  $ echo foo > foo
  $ echo baz > baz
  $ mkdir -p dir/subdir
  $ echo dir/file >> dir/file
  $ echo dir/file2 >> dir/file2
  $ echo dir/file3 >> dir/file3 # to be corrupted in rev 0
  $ echo dir/subdir/file3 >> dir/subdir/file3
  $ echo dir/subdir/file4 >> dir/subdir/file4
  $ hg ci -d '0 0' -qAm '0: add foo baz dir/'
  $ echo bar > bar
  $ echo quux > quux
  $ echo dir/file4 >> dir/file4 # to be corrupted in rev 1
  $ hg copy foo copied
  $ hg ci -d '1 0' -qAm '1: add bar quux; copy foo to copied'
  $ echo >> foo
  $ hg ci -d '2 0' -m '2: change foo'
  $ hg up -qC 1
  $ echo >> bar
  $ echo >> quux
  $ hg ci -d '3 0' -m '3: change bar quux'
  created new head
  $ hg up -qC 2
  $ hg merge -qr 3
  $ echo >> bar
  $ echo >> baz
  $ hg ci -d '4 0' -m '4: first merge; change bar baz'
  $ echo >> bar
  $ echo 1 >> baz
  $ echo >> quux
  $ hg ci -d '5 0' -m '5: change bar baz quux'
  $ hg up -qC 4
  $ echo >> foo
  $ echo 2 >> baz
  $ hg ci -d '6 0' -m '6: change foo baz'
  created new head
  $ hg up -qC 5
  $ hg merge -qr 6
  $ echo >> bar
  $ hg ci -d '7 0' -m '7: second merge; change bar'
  $ echo >> foo
  $ hg ci -m '8: change foo'
  $ glog
  @  8 "8: change foo" files: foo
  |
  o    7 "7: second merge; change bar" files: bar baz
  |\
  | o  6 "6: change foo baz" files: baz foo
  | |
  o |  5 "5: change bar baz quux" files: bar baz quux
  |/
  o    4 "4: first merge; change bar baz" files: bar baz
  |\
  | o  3 "3: change bar quux" files: bar quux
  | |
  o |  2 "2: change foo" files: foo
  |/
  o  1 "1: add bar quux; copy foo to copied" files: bar copied dir/file4 quux
  |
  o  0 "0: add foo baz dir/" files: baz dir/file dir/file2 dir/file3 dir/subdir/file3 dir/subdir/file4 foo
  

final file versions in this repo:

  $ hg manifest --debug
  9463f52fe115e377cf2878d4fc548117211063f2 644   bar
  94c1be4dfde2ee8d78db8bbfcf81210813307c3d 644   baz
  7711d36246cc83e61fb29cd6d4ef394c63f1ceaf 644   copied
  3e20847584beff41d7cd16136b7331ab3d754be0 644   dir/file
  75e6d3f8328f5f6ace6bf10b98df793416a09dca 644   dir/file2
  e96dce0bc6a217656a3a410e5e6bec2c4f42bf7c 644   dir/file3
  6edd55f559cdce67132b12ca09e09cee08b60442 644   dir/file4
  5fe139720576e18e34bcc9f79174db8897c8afe9 644   dir/subdir/file3
  57a1c1511590f3de52874adfa04effe8a77d64af 644   dir/subdir/file4
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  bc3eca3f47023a3e70ca0d8cc95a22a6827db19d 644   quux
  $ hg debugrename copied
  copied renamed from foo:2ed2a3912a0b24502043eae84ee4b279c18b90dd

  $ cd ..


Test interaction with startrev and verify that changing it is handled properly:

  $ > empty
  $ hg convert --filemap empty source movingstart --config convert.hg.startrev=3 -r4
  initializing destination movingstart repository
  scanning source...
  sorting...
  converting...
  1 3: change bar quux
  0 4: first merge; change bar baz
  $ hg convert --filemap empty source movingstart
  scanning source...
  sorting...
  converting...
  3 5: change bar baz quux
  2 6: change foo baz
  1 7: second merge; change bar
  warning: af455ce4166b3c9c88e6309c2b9332171dcea595 parent 61e22ca76c3b3e93df20338c4e02ce286898e825 is missing
  warning: cf908b3eeedc301c9272ebae931da966d5b326c7 parent 59e1ab45c888289513b7354484dac8a88217beab is missing
  0 8: change foo


splitrepo tests

  $ splitrepo()
  > {
  >     msg="$1"
  >     files="$2"
  >     opts=$3
  >     echo "% $files: $msg"
  >     prefix=`echo "$files" | sed -e 's/ /-/g'`
  >     fmap="$prefix.fmap"
  >     repo="$prefix.repo"
  >     for i in $files; do
  >         echo "include $i" >> "$fmap"
  >     done
  >     hg -q convert $opts --filemap "$fmap" --datesort source "$repo"
  >     hg up -q -R "$repo"
  >     glog -R "$repo"
  >     hg -R "$repo" manifest --debug
  > }
  $ splitrepo 'skip unwanted merges; use 1st parent in 1st merge, 2nd in 2nd' foo
  % foo: skip unwanted merges; use 1st parent in 1st merge, 2nd in 2nd
  @  3 "8: change foo" files: foo
  |
  o  2 "6: change foo baz" files: foo
  |
  o  1 "2: change foo" files: foo
  |
  o  0 "0: add foo baz dir/" files: foo
  
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  $ splitrepo 'merges are not merges anymore' bar
  % bar: merges are not merges anymore
  @  4 "7: second merge; change bar" files: bar
  |
  o  3 "5: change bar baz quux" files: bar
  |
  o  2 "4: first merge; change bar baz" files: bar
  |
  o  1 "3: change bar quux" files: bar
  |
  o  0 "1: add bar quux; copy foo to copied" files: bar
  
  9463f52fe115e377cf2878d4fc548117211063f2 644   bar
  $ splitrepo '1st merge is not a merge anymore; 2nd still is' baz
  % baz: 1st merge is not a merge anymore; 2nd still is
  @    4 "7: second merge; change bar" files: baz
  |\
  | o  3 "6: change foo baz" files: baz
  | |
  o |  2 "5: change bar baz quux" files: baz
  |/
  o  1 "4: first merge; change bar baz" files: baz
  |
  o  0 "0: add foo baz dir/" files: baz
  
  94c1be4dfde2ee8d78db8bbfcf81210813307c3d 644   baz
  $ splitrepo 'we add additional merges when they are interesting' 'foo quux'
  % foo quux: we add additional merges when they are interesting
  @  8 "8: change foo" files: foo
  |
  o    7 "7: second merge; change bar" files:
  |\
  | o  6 "6: change foo baz" files: foo
  | |
  o |  5 "5: change bar baz quux" files: quux
  |/
  o    4 "4: first merge; change bar baz" files:
  |\
  | o  3 "3: change bar quux" files: quux
  | |
  o |  2 "2: change foo" files: foo
  |/
  o  1 "1: add bar quux; copy foo to copied" files: quux
  |
  o  0 "0: add foo baz dir/" files: foo
  
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  bc3eca3f47023a3e70ca0d8cc95a22a6827db19d 644   quux
  $ splitrepo 'partial conversion' 'bar quux' '-r 3'
  % bar quux: partial conversion
  @  1 "3: change bar quux" files: bar quux
  |
  o  0 "1: add bar quux; copy foo to copied" files: bar quux
  
  b79105bedc55102f394e90a789c9c380117c1b4a 644   bar
  db0421cc6b685a458c8d86c7d5c004f94429ea23 644   quux
  $ splitrepo 'complete the partial conversion' 'bar quux'
  % bar quux: complete the partial conversion
  @  4 "7: second merge; change bar" files: bar
  |
  o  3 "5: change bar baz quux" files: bar quux
  |
  o  2 "4: first merge; change bar baz" files: bar
  |
  o  1 "3: change bar quux" files: bar quux
  |
  o  0 "1: add bar quux; copy foo to copied" files: bar quux
  
  9463f52fe115e377cf2878d4fc548117211063f2 644   bar
  bc3eca3f47023a3e70ca0d8cc95a22a6827db19d 644   quux
  $ rm -r foo.repo
  $ splitrepo 'partial conversion' 'foo' '-r 3'
  % foo: partial conversion
  @  0 "0: add foo baz dir/" files: foo
  
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   foo
  $ splitrepo 'complete the partial conversion' 'foo'
  % foo: complete the partial conversion
  @  3 "8: change foo" files: foo
  |
  o  2 "6: change foo baz" files: foo
  |
  o  1 "2: change foo" files: foo
  |
  o  0 "0: add foo baz dir/" files: foo
  
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  $ splitrepo 'copied file; source not included in new repo' copied
  % copied: copied file; source not included in new repo
  @  0 "1: add bar quux; copy foo to copied" files: copied
  
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   copied
  $ hg --cwd copied.repo debugrename copied
  copied not renamed
  $ splitrepo 'copied file; source included in new repo' 'foo copied'
  % foo copied: copied file; source included in new repo
  @  4 "8: change foo" files: foo
  |
  o  3 "6: change foo baz" files: foo
  |
  o  2 "2: change foo" files: foo
  |
  o  1 "1: add bar quux; copy foo to copied" files: copied
  |
  o  0 "0: add foo baz dir/" files: foo
  
  7711d36246cc83e61fb29cd6d4ef394c63f1ceaf 644   copied
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo
  $ hg --cwd foo-copied.repo debugrename copied
  copied renamed from foo:2ed2a3912a0b24502043eae84ee4b279c18b90dd

ensure that the filemap contains duplicated slashes (issue3612)

  $ cat > renames.fmap <<EOF
  > include dir
  > exclude dir/file2
  > rename dir dir2//dir3
  > include foo
  > include copied
  > rename foo foo2/
  > rename copied ./copied2
  > exclude dir/subdir
  > include dir/subdir/file3
  > EOF
  $ rm source/.hg/store/data/dir/file3.i
  $ rm source/.hg/store/data/dir/file4.i
  $ hg -q convert --filemap renames.fmap --datesort source dummydest
  abort: data/dir/file3.i@e96dce0bc6a2: no match found!
  [255]
  $ hg -q convert --filemap renames.fmap --datesort --config convert.hg.ignoreerrors=1 source renames.repo
  ignoring: data/dir/file3.i@e96dce0bc6a2: no match found
  ignoring: data/dir/file4.i@6edd55f559cd: no match found
  $ hg up -q -R renames.repo
  $ glog -R renames.repo
  @  4 "8: change foo" files: foo2
  |
  o  3 "6: change foo baz" files: foo2
  |
  o  2 "2: change foo" files: foo2
  |
  o  1 "1: add bar quux; copy foo to copied" files: copied2
  |
  o  0 "0: add foo baz dir/" files: dir2/dir3/file dir2/dir3/subdir/file3 foo2
  
  $ hg -R renames.repo verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 5 changesets, 7 total revisions

  $ hg -R renames.repo manifest --debug
  d43feacba7a4f1f2080dde4a4b985bd8a0236d46 644   copied2
  3e20847584beff41d7cd16136b7331ab3d754be0 644   dir2/dir3/file
  5fe139720576e18e34bcc9f79174db8897c8afe9 644   dir2/dir3/subdir/file3
  9a7b52012991e4873687192c3e17e61ba3e837a3 644   foo2
  $ hg --cwd renames.repo debugrename copied2
  copied2 renamed from foo2:2ed2a3912a0b24502043eae84ee4b279c18b90dd

copied:

  $ hg --cwd source cat copied
  foo

copied2:

  $ hg --cwd renames.repo cat copied2
  foo

filemap errors

  $ cat > errors.fmap <<EOF
  > include dir/ # beware that comments changes error line numbers!
  > exclude /dir
  > rename dir//dir /dir//dir/ "out of sync"
  > include
  > EOF
  $ hg -q convert --filemap errors.fmap source errors.repo
  errors.fmap:3: superfluous / in include '/dir'
  errors.fmap:3: superfluous / in rename '/dir'
  errors.fmap:4: unknown directive 'out of sync'
  errors.fmap:5: path to exclude is missing
  abort: errors in filemap
  [255]

test branch closing revision pruning if branch is pruned

  $ hg init branchpruning
  $ cd branchpruning
  $ hg branch foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ hg ci --close-branch -m closefoo
  $ hg up 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch empty
  marked working directory as branch empty
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m emptybranch
  $ hg ci --close-branch -m closeempty
  $ hg up 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch default
  marked working directory as branch default
  (branches are permanent and global, did you want a bookmark?)
  $ echo b > b
  $ hg ci -Am addb
  adding b
  $ hg ci --close-branch -m closedefault
  $ cat > filemap <<EOF
  > include b
  > EOF
  $ cd ..
  $ hg convert branchpruning branchpruning-hg1
  initializing destination branchpruning-hg1 repository
  scanning source...
  sorting...
  converting...
  5 adda
  4 closefoo
  3 emptybranch
  2 closeempty
  1 addb
  0 closedefault
  $ glog -R branchpruning-hg1
  o  5 "closedefault" files:
  |
  o  4 "addb" files: b
  |
  | o  3 "closeempty" files:
  | |
  | o  2 "emptybranch" files:
  |/
  | o  1 "closefoo" files:
  |/
  o  0 "adda" files: a
  

exercise incremental conversion at the same time

  $ hg convert -r0 --filemap branchpruning/filemap branchpruning branchpruning-hg2
  initializing destination branchpruning-hg2 repository
  scanning source...
  sorting...
  converting...
  0 adda
  $ hg convert -r4 --filemap branchpruning/filemap branchpruning branchpruning-hg2
  scanning source...
  sorting...
  converting...
  0 addb
  $ hg convert --filemap branchpruning/filemap branchpruning branchpruning-hg2
  scanning source...
  sorting...
  converting...
  3 closefoo
  2 emptybranch
  1 closeempty
  0 closedefault
  $ glog -R branchpruning-hg2
  o  1 "closedefault" files:
  |
  o  0 "addb" files: b
  

Test rebuilding of map with unknown revisions in shamap - it used to crash

  $ cd branchpruning
  $ hg up -r 2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merging something'
  $ cd ..
  $ echo "53792d18237d2b64971fa571936869156655338d 6d955580116e82c4b029bd30f321323bae71a7f0" >> branchpruning-hg2/.hg/shamap
  $ hg convert --filemap branchpruning/filemap branchpruning branchpruning-hg2 --debug
  run hg source pre-conversion action
  run hg sink pre-conversion action
  scanning source...
  scanning: 1 revisions
  sorting...
  converting...
  0 merging something
  source: 2503605b178fe50e8fbbb0e77b97939540aa8c87
  converting: 0/1 revisions (0.00%)
  unknown revmap source: 53792d18237d2b64971fa571936869156655338d
  run hg sink post-conversion action
  run hg source post-conversion action


filemap rename undoing revision rename

  $ hg init renameundo
  $ cd renameundo
  $ echo 1 > a
  $ echo 1 > c
  $ hg ci -qAm add
  $ hg mv -q a b/a
  $ hg mv -q c b/c
  $ hg ci -qm rename
  $ echo 2 > b/a
  $ echo 2 > b/c
  $ hg ci -qm modify
  $ cd ..

  $ echo "rename b ." > renameundo.fmap
  $ hg convert --filemap renameundo.fmap renameundo renameundo2
  initializing destination renameundo2 repository
  scanning source...
  sorting...
  converting...
  2 add
  1 rename
  filtering out empty revision
  repository tip rolled back to revision 0 (undo commit)
  0 modify
  $ glog -R renameundo2
  o  1 "modify" files: a c
  |
  o  0 "add" files: a c
  


test merge parents/empty merges pruning

  $ glog()
  > {
  >     hg glog --template '{rev}:{node|short}@{branch} "{desc}" files: {files}\n' "$@"
  > }

test anonymous branch pruning

  $ hg init anonymousbranch
  $ cd anonymousbranch
  $ echo a > a
  $ echo b > b
  $ hg ci -Am add
  adding a
  adding b
  $ echo a >> a
  $ hg ci -m changea
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> b
  $ hg ci -m changeb
  created new head
  $ hg up 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ cd ..

  $ cat > filemap <<EOF
  > include a
  > EOF
  $ hg convert --filemap filemap anonymousbranch anonymousbranch-hg
  initializing destination anonymousbranch-hg repository
  scanning source...
  sorting...
  converting...
  3 add
  2 changea
  1 changeb
  0 merge
  $ glog -R anonymousbranch
  @    3:c71d5201a498@default "merge" files:
  |\
  | o  2:607eb44b17f9@default "changeb" files: b
  | |
  o |  1:1f60ea617824@default "changea" files: a
  |/
  o  0:0146e6129113@default "add" files: a b
  
  $ glog -R anonymousbranch-hg
  o  1:cda818e7219b@default "changea" files: a
  |
  o  0:c334dc3be0da@default "add" files: a
  
  $ cat anonymousbranch-hg/.hg/shamap
  0146e6129113dba9ac90207cfdf2d7ed35257ae5 c334dc3be0daa2a4e9ce4d2e2bdcba40c09d4916
  1f60ea61782421edf8d051ff4fcb61b330f26a4a cda818e7219b5f7f3fb9f49780054ed6a1905ec3
  607eb44b17f9348cd5cbd26e16af87ba77b0b037 c334dc3be0daa2a4e9ce4d2e2bdcba40c09d4916
  c71d5201a498b2658d105a6bf69d7a0df2649aea cda818e7219b5f7f3fb9f49780054ed6a1905ec3

  $ cat > filemap <<EOF
  > include b
  > EOF
  $ hg convert --filemap filemap anonymousbranch anonymousbranch-hg2
  initializing destination anonymousbranch-hg2 repository
  scanning source...
  sorting...
  converting...
  3 add
  2 changea
  1 changeb
  0 merge
  $ glog -R anonymousbranch
  @    3:c71d5201a498@default "merge" files:
  |\
  | o  2:607eb44b17f9@default "changeb" files: b
  | |
  o |  1:1f60ea617824@default "changea" files: a
  |/
  o  0:0146e6129113@default "add" files: a b
  
  $ glog -R anonymousbranch-hg2
  o  1:62dd350b0df6@default "changeb" files: b
  |
  o  0:4b9ced861657@default "add" files: b
  
  $ cat anonymousbranch-hg2/.hg/shamap
  0146e6129113dba9ac90207cfdf2d7ed35257ae5 4b9ced86165703791653059a1db6ed864630a523
  1f60ea61782421edf8d051ff4fcb61b330f26a4a 4b9ced86165703791653059a1db6ed864630a523
  607eb44b17f9348cd5cbd26e16af87ba77b0b037 62dd350b0df695f7d2c82a02e0499b16fd790f22
  c71d5201a498b2658d105a6bf69d7a0df2649aea 62dd350b0df695f7d2c82a02e0499b16fd790f22

test named branch pruning

  $ hg init namedbranch
  $ cd namedbranch
  $ echo a > a
  $ echo b > b
  $ hg ci -Am add
  adding a
  adding b
  $ echo a >> a
  $ hg ci -m changea
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  $ echo b >> b
  $ hg ci -m changeb
  $ hg up default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ cd ..

  $ cat > filemap <<EOF
  > include a
  > EOF
  $ hg convert --filemap filemap namedbranch namedbranch-hg
  initializing destination namedbranch-hg repository
  scanning source...
  sorting...
  converting...
  3 add
  2 changea
  1 changeb
  0 merge
  $ glog -R namedbranch
  @    3:73899bcbe45c@default "merge" files:
  |\
  | o  2:8097982d19fc@foo "changeb" files: b
  | |
  o |  1:1f60ea617824@default "changea" files: a
  |/
  o  0:0146e6129113@default "add" files: a b
  
  $ glog -R namedbranch-hg
  o  1:cda818e7219b@default "changea" files: a
  |
  o  0:c334dc3be0da@default "add" files: a
  

  $ cd namedbranch
  $ hg --config extensions.mq= strip tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/namedbranch/.hg/strip-backup/73899bcbe45c-backup.hg (glob)
  $ hg up foo
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ cd ..

  $ hg convert --filemap filemap namedbranch namedbranch-hg2
  initializing destination namedbranch-hg2 repository
  scanning source...
  sorting...
  converting...
  3 add
  2 changea
  1 changeb
  0 merge
  $ glog -R namedbranch
  @    3:e1959de76e1b@foo "merge" files:
  |\
  | o  2:8097982d19fc@foo "changeb" files: b
  | |
  o |  1:1f60ea617824@default "changea" files: a
  |/
  o  0:0146e6129113@default "add" files: a b
  
  $ glog -R namedbranch-hg2
  o    2:dcf314454667@foo "merge" files:
  |\
  | o  1:cda818e7219b@default "changea" files: a
  |/
  o  0:c334dc3be0da@default "add" files: a
  
