Setting up test

  $ hg init test
  $ cd test
  $ echo 0 > afile
  $ hg add afile
  $ hg commit -m "0.0"
  $ echo 1 >> afile
  $ hg commit -m "0.1"
  $ echo 2 >> afile
  $ hg commit -m "0.2"
  $ echo 3 >> afile
  $ hg commit -m "0.3"
  $ hg update -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 1 >> afile
  $ hg commit -m "1.1"
  created new head
  $ echo 2 >> afile
  $ hg commit -m "1.2"
  $ echo "a line" > fred
  $ echo 3 >> afile
  $ hg add fred
  $ hg commit -m "1.3"
  $ hg mv afile adifferentfile
  $ hg commit -m "1.3m"
  $ hg update -C 3
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg mv afile anotherfile
  $ hg commit -m "0.3m"
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 9 changesets, 7 total revisions
  $ cd ..
  $ hg init empty

Bundle and phase

  $ hg -R test phase --force --secret 0
  $ hg -R test bundle phase.hg empty
  searching for changes
  no changes found (ignored 9 secret changesets)
  [1]
  $ hg -R test phase --draft -r 'head()'

Bundle --all

  $ hg -R test bundle --all all.hg
  9 changesets found

Bundle test to full.hg

  $ hg -R test bundle full.hg empty
  searching for changes
  9 changesets found

Unbundle full.hg in test

  $ hg -R test unbundle full.hg
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 4 files
  (run 'hg update' to get a working copy)

Verify empty

  $ hg -R empty heads
  [1]
  $ hg -R empty verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  0 files, 0 changesets, 0 total revisions

Pull full.hg into test (using --cwd)

  $ hg --cwd test pull ../full.hg
  pulling from ../full.hg
  searching for changes
  no changes found

Verify that there are no leaked temporary files after pull (issue2797)

  $ ls test/.hg | grep .hg10un
  [1]

Pull full.hg into empty (using --cwd)

  $ hg --cwd empty pull ../full.hg
  pulling from ../full.hg
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 7 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

Rollback empty

  $ hg -R empty rollback
  repository tip rolled back to revision -1 (undo pull)

Pull full.hg into empty again (using --cwd)

  $ hg --cwd empty pull ../full.hg
  pulling from ../full.hg
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 7 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

Pull full.hg into test (using -R)

  $ hg -R test pull full.hg
  pulling from full.hg
  searching for changes
  no changes found

Pull full.hg into empty (using -R)

  $ hg -R empty pull full.hg
  pulling from full.hg
  searching for changes
  no changes found

Rollback empty

  $ hg -R empty rollback
  repository tip rolled back to revision -1 (undo pull)

Pull full.hg into empty again (using -R)

  $ hg -R empty pull full.hg
  pulling from full.hg
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 7 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

Log -R full.hg in fresh empty

  $ rm -r empty
  $ hg init empty
  $ cd empty
  $ hg -R bundle://../full.hg log
  changeset:   8:aa35859c02ea
  tag:         tip
  parent:      3:eebf5a27f8ca
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.3m
  
  changeset:   7:a6a34bfa0076
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.3m
  
  changeset:   6:7373c1169842
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.3
  
  changeset:   5:1bb50a9436a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.2
  
  changeset:   4:095197eb4973
  parent:      0:f9ee2f85a263
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.1
  
  changeset:   3:eebf5a27f8ca
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.3
  
  changeset:   2:e38ba6f5b7e0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.2
  
  changeset:   1:34c2bf6b0626
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.1
  
  changeset:   0:f9ee2f85a263
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.0
  
Make sure bundlerepo doesn't leak tempfiles (issue2491)

  $ ls .hg
  00changelog.i
  cache
  requires
  store

Pull ../full.hg into empty (with hook)

  $ echo "[hooks]" >> .hg/hgrc
  $ echo "changegroup = printenv.py changegroup" >> .hg/hgrc

doesn't work (yet ?)

hg -R bundle://../full.hg verify

  $ hg pull bundle://../full.hg
  pulling from bundle:../full.hg
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 7 changes to 4 files (+1 heads)
  changegroup hook: HG_NODE=f9ee2f85a263049e9ae6d37a0e67e96194ffb735 HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=bundle:../full.hg (glob)
  (run 'hg heads' to see heads, 'hg merge' to merge)

Rollback empty

  $ hg rollback
  repository tip rolled back to revision -1 (undo pull)
  $ cd ..

Log -R bundle:empty+full.hg

  $ hg -R bundle:empty+full.hg log --template="{rev} "; echo ""
  8 7 6 5 4 3 2 1 0 

Pull full.hg into empty again (using -R; with hook)

  $ hg -R empty pull full.hg
  pulling from full.hg
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 7 changes to 4 files (+1 heads)
  changegroup hook: HG_NODE=f9ee2f85a263049e9ae6d37a0e67e96194ffb735 HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=bundle:empty+full.hg (glob)
  (run 'hg heads' to see heads, 'hg merge' to merge)

Create partial clones

  $ rm -r empty
  $ hg init empty
  $ hg clone -r 3 test partial
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone partial partial2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd partial

Log -R full.hg in partial

  $ hg -R bundle://../full.hg log -T phases
  changeset:   8:aa35859c02ea
  tag:         tip
  phase:       draft
  parent:      3:eebf5a27f8ca
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.3m
  
  changeset:   7:a6a34bfa0076
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.3m
  
  changeset:   6:7373c1169842
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.3
  
  changeset:   5:1bb50a9436a7
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.2
  
  changeset:   4:095197eb4973
  phase:       draft
  parent:      0:f9ee2f85a263
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.1
  
  changeset:   3:eebf5a27f8ca
  phase:       public
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.3
  
  changeset:   2:e38ba6f5b7e0
  phase:       public
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.2
  
  changeset:   1:34c2bf6b0626
  phase:       public
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.1
  
  changeset:   0:f9ee2f85a263
  phase:       public
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.0
  

Incoming full.hg in partial

  $ hg incoming bundle://../full.hg
  comparing with bundle:../full.hg
  searching for changes
  changeset:   4:095197eb4973
  parent:      0:f9ee2f85a263
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.1
  
  changeset:   5:1bb50a9436a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.2
  
  changeset:   6:7373c1169842
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.3
  
  changeset:   7:a6a34bfa0076
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.3m
  
  changeset:   8:aa35859c02ea
  tag:         tip
  parent:      3:eebf5a27f8ca
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.3m
  

Outgoing -R full.hg vs partial2 in partial

  $ hg -R bundle://../full.hg outgoing ../partial2
  comparing with ../partial2
  searching for changes
  changeset:   4:095197eb4973
  parent:      0:f9ee2f85a263
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.1
  
  changeset:   5:1bb50a9436a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.2
  
  changeset:   6:7373c1169842
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.3
  
  changeset:   7:a6a34bfa0076
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.3m
  
  changeset:   8:aa35859c02ea
  tag:         tip
  parent:      3:eebf5a27f8ca
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.3m
  

Outgoing -R does-not-exist.hg vs partial2 in partial

  $ hg -R bundle://../does-not-exist.hg outgoing ../partial2
  abort: *../does-not-exist.hg* (glob)
  [255]
  $ cd ..

hide outer repo
  $ hg init

Direct clone from bundle (all-history)

  $ hg clone full.hg full-clone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 7 changes to 4 files (+1 heads)
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R full-clone heads
  changeset:   8:aa35859c02ea
  tag:         tip
  parent:      3:eebf5a27f8ca
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.3m
  
  changeset:   7:a6a34bfa0076
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1.3m
  
  $ rm -r full-clone

When cloning from a non-copiable repository into '', do not
recurse infinitely (issue2528)

  $ hg clone full.hg ''
  abort: empty destination path is not valid
  [255]

test for http://mercurial.selenic.com/bts/issue216

Unbundle incremental bundles into fresh empty in one go

  $ rm -r empty
  $ hg init empty
  $ hg -R test bundle --base null -r 0 ../0.hg
  1 changesets found
  $ hg -R test bundle --base 0    -r 1 ../1.hg
  1 changesets found
  $ hg -R empty unbundle -u ../0.hg ../1.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

View full contents of the bundle
  $ hg -R test bundle --base null -r 3  ../partial.hg
  4 changesets found
  $ cd test
  $ hg -R ../../partial.hg log -r "bundle()"
  changeset:   0:f9ee2f85a263
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.0
  
  changeset:   1:34c2bf6b0626
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.1
  
  changeset:   2:e38ba6f5b7e0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.2
  
  changeset:   3:eebf5a27f8ca
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0.3
  
  $ cd ..

test for 540d1059c802

test for 540d1059c802

  $ hg init orig
  $ cd orig
  $ echo foo > foo
  $ hg add foo
  $ hg ci -m 'add foo'

  $ hg clone . ../copy
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg tag foo

  $ cd ../copy
  $ echo >> foo
  $ hg ci -m 'change foo'
  $ hg bundle ../bundle.hg ../orig
  searching for changes
  1 changesets found

  $ cd ../orig
  $ hg incoming ../bundle.hg
  comparing with ../bundle.hg
  searching for changes
  changeset:   2:ed1b79f46b9a
  tag:         tip
  parent:      0:bbd179dfa0a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo
  
  $ cd ..

test bundle with # in the filename (issue2154):

  $ cp bundle.hg 'test#bundle.hg'
  $ cd orig
  $ hg incoming '../test#bundle.hg'
  comparing with ../test
  abort: unknown revision 'bundle.hg'!
  [255]

note that percent encoding is not handled:

  $ hg incoming ../test%23bundle.hg
  abort: repository ../test%23bundle.hg not found!
  [255]
  $ cd ..

test to bundle revisions on the newly created branch (issue3828):

  $ hg -q clone -U test test-clone
  $ cd test

  $ hg -q branch foo
  $ hg commit -m "create foo branch"
  $ hg -q outgoing ../test-clone
  9:b4f5acb1ee27
  $ hg -q bundle --branch foo foo.hg ../test-clone
  $ hg -R foo.hg -q log -r "bundle()"
  9:b4f5acb1ee27

  $ cd ..

test for http://mercurial.selenic.com/bts/issue1144

test that verify bundle does not traceback

partial history bundle, fails w/ unknown parent

  $ hg -R bundle.hg verify
  abort: 00changelog.i@bbd179dfa0a7: unknown parent!
  [255]

full history bundle, refuses to verify non-local repo

  $ hg -R all.hg verify
  abort: cannot verify bundle or remote repos
  [255]

but, regular verify must continue to work

  $ hg -R orig verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 2 total revisions

diff against bundle

  $ hg init b
  $ cd b
  $ hg -R ../all.hg diff -r tip
  diff -r aa35859c02ea anotherfile
  --- a/anotherfile	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,4 +0,0 @@
  -0
  -1
  -2
  -3
  $ cd ..

bundle single branch

  $ hg init branchy
  $ cd branchy
  $ echo a >a
  $ echo x >x
  $ hg ci -Ama
  adding a
  adding x
  $ echo c >c
  $ echo xx >x
  $ hg ci -Amc
  adding c
  $ echo c1 >c1
  $ hg ci -Amc1
  adding c1
  $ hg up 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b >b
  $ hg ci -Amb
  adding b
  created new head
  $ echo b1 >b1
  $ echo xx >x
  $ hg ci -Amb1
  adding b1
  $ hg clone -q -r2 . part

== bundling via incoming

  $ hg in -R part --bundle incoming.hg --template "{node}\n" .
  comparing with .
  searching for changes
  1a38c1b849e8b70c756d2d80b0b9a3ac0b7ea11a
  057f4db07f61970e1c11e83be79e9d08adc4dc31

== bundling

  $ hg bundle bundle.hg part --debug --config progress.debug=true
  query 1; heads
  searching for changes
  all remote heads known locally
  2 changesets found
  list of changesets:
  1a38c1b849e8b70c756d2d80b0b9a3ac0b7ea11a
  057f4db07f61970e1c11e83be79e9d08adc4dc31
  bundling: 1/2 changesets (50.00%)
  bundling: 2/2 changesets (100.00%)
  bundling: 1/2 manifests (50.00%)
  bundling: 2/2 manifests (100.00%)
  bundling: b 1/3 files (33.33%)
  bundling: b1 2/3 files (66.67%)
  bundling: x 3/3 files (100.00%)

== Test for issue3441

  $ hg clone -q -r0 . part2
  $ hg -q -R part2 pull bundle.hg
  $ hg -R part2 verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 3 changesets, 5 total revisions

  $ cd ..
