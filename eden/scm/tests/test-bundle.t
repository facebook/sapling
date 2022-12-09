#chg-compatible
#debugruntest-compatible
  $ configure modernclient

  $ setconfig format.usegeneraldelta=yes
We're bundling local clones here
  $ setconfig exchange.httpcommitlookup=False

Setting up test

  $ newclientrepo test
  $ echo 0 > afile
  $ hg add afile
  $ hg commit -m "0.0"
  $ echo 1 >> afile
  $ hg commit -m "0.1"
  $ echo 2 >> afile
  $ hg commit -m "0.2"
  $ echo 3 >> afile
  $ hg commit -m "0.3"
  $ hg push -q -r . --to head1 --create
  $ hg goto -C 'desc(0.0)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 1 >> afile
  $ hg commit -m "1.1"
  $ echo 2 >> afile
  $ hg commit -m "1.2"
  $ echo "a line" > fred
  $ echo 3 >> afile
  $ hg add fred
  $ hg commit -m "1.3"
  $ hg mv afile adifferentfile
  $ hg commit -m "1.3m"
  $ hg push -q -r . --to head2 --create
  $ hg goto -C 'desc(0.3)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg mv afile anotherfile
  $ hg commit -m "0.3m"
  $ hg push -q -r . --to head3 --create
  $ cd ..
  $ newclientrepo empty
  $ cd ..

Bundle --all

  $ hg -R test bundle --all all.hg
  9 changesets found

Bundle test to full.hg

  $ hg -R test bundle full.hg test:empty_server
  searching for changes
  9 changesets found

Unbundle full.hg in test

  $ hg -R test unbundle full.hg
  adding changesets
  adding manifests
  adding file changes

Verify empty

  $ hg -R empty heads
  [1]

Pull full.hg into test (using --cwd)

  $ hg --cwd test unbundle ../full.hg
  adding changesets
  adding manifests
  adding file changes

Verify that there are no leaked temporary files after pull (issue2797)

  $ ls test/.hg | grep .hg10un
  [1]

Pull full.hg into empty (using --cwd)

  $ hg --cwd empty unbundle ../full.hg
  adding changesets
  adding manifests
  adding file changes

Rollback empty

  $ hg -R empty debugstrip 'desc(0.0)' --no-backup

Pull full.hg into empty again (using --cwd)

  $ hg --cwd empty unbundle ../full.hg
  adding changesets
  adding manifests
  adding file changes

Pull full.hg into test (using -R)

  $ hg -R test unbundle full.hg
  adding changesets
  adding manifests
  adding file changes

Pull full.hg into empty (using -R)

  $ hg -R empty unbundle full.hg
  adding changesets
  adding manifests
  adding file changes

Rollback empty

  $ hg -R empty debugstrip 'desc(0.0)' --no-backup

Pull full.hg into empty again (using -R)

  $ hg -R empty unbundle full.hg
  adding changesets
  adding manifests
  adding file changes

  $ rm -r empty empty_server
  $ newclientrepo empty
Pull ../full.hg into empty (with hook)

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > changegroup = sh -c "env | grep '^HG_' | sort"
  > EOF

  $ hg unbundle ../full.hg
  adding changesets
  adding manifests
  adding file changes
  HG_BUNDLE2=1
  HG_HOOKNAME=changegroup
  HG_HOOKTYPE=changegroup
  HG_NODE=f9ee2f85a263049e9ae6d37a0e67e96194ffb735
  HG_NODE_LAST=aa35859c02ea8bd48da5da68cd2740ac71afcbaf
  HG_SOURCE=unbundle
  HG_TXNID=TXN:$ID$
  HG_URL=bundle:../full.hg

Rollback empty

  $ hg debugstrip 'desc(0.0)' --no-backup
  $ cd ..

Pull full.hg into empty again (using -R; with hook)

  $ hg -R empty unbundle full.hg
  adding changesets
  adding manifests
  adding file changes
  HG_BUNDLE2=1
  HG_HOOKNAME=changegroup
  HG_HOOKTYPE=changegroup
  HG_NODE=f9ee2f85a263049e9ae6d37a0e67e96194ffb735
  HG_NODE_LAST=aa35859c02ea8bd48da5da68cd2740ac71afcbaf
  HG_SOURCE=unbundle
  HG_TXNID=TXN:$ID$
  HG_URL=bundle:full.hg

Unbundle incremental bundles into fresh empty in one go

  $ rm -r empty empty_server
  $ newclientrepo empty
  $ cd ..
  $ hg -R test bundle --base null -r 'desc(0.0)' ../0.hg
  1 changesets found
  $ hg -R test bundle --base 'desc(0.0)'    -r 'desc(0.1)' ../1.hg
  1 changesets found
  $ hg -R empty unbundle -u ../0.hg ../1.hg
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

View full contents of the bundle
  $ hg -R test bundle --base null -r eebf5a27f8ca9b92ade529321141c1561cc4a9c2  ../partial.hg
  4 changesets found

test for 540d1059c802

test for 540d1059c802

  $ newclientrepo orig
  $ echo foo > foo
  $ hg add foo
  $ hg ci -m 'add foo'
  $ hg push -q -r . --to book --create

  $ newclientrepo copy test:orig_server book

  $ echo >> foo
  $ hg ci -m 'change foo'
  $ hg bundle ../bundle.hg test:orig_server
  searching for changes
  1 changesets found

  $ cd ..

test for https://bz.mercurial-scm.org/1144

test that verify bundle does not traceback

bundle single branch

  $ newclientrepo branchy
  $ echo a >a
  $ echo x >x
  $ hg ci -Ama
  adding a
  adding x
  $ hg push -q -r . --to head0 --create
  $ echo c >c
  $ echo xx >x
  $ hg ci -Amc
  adding c
  $ echo c1 >c1
  $ hg ci -Amc1
  adding c1
  $ hg push -q -r . --to head2 --create
  $ hg up 'desc(a)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b >b
  $ hg ci -Amb
  adding b
  $ echo b1 >b1
  $ echo xx >x
  $ hg ci -Amb1
  adding b1

== bundling

  $ hg bundle bundle.hg test:branchy_server --debug --config progress.debug=true
  query 1; heads
  searching for changes
  local heads: 2; remote heads: 1 (explicit: 0); initial common: 1
  sampling from both directions (1 of 1)
  sampling undecided commits (1 of 1)
  progress: searching: checking 1 commits, 0 left 2 queries
  query 2; still undecided: 1, sample size is: 1
  progress: searching (end)
  2 total queries in 0.0000s
  2 changesets found
  list of changesets:
  1a38c1b849e8b70c756d2d80b0b9a3ac0b7ea11a
  057f4db07f61970e1c11e83be79e9d08adc4dc31
  bundle2-output-bundle: "HG20", (1 params) 2 parts total
  bundle2-output-part: "changegroup" (params: 1 mandatory 1 advisory) streamed payload
  progress: bundling: 1/2 changesets (50.00%)
  progress: bundling: 2/2 changesets (100.00%)
  progress: bundling (end)
  progress: manifests: 1/2 (50.00%)
  progress: manifests: 2/2 (100.00%)
  progress: manifests (end)
  progress: bundling: b 1/3 files (33.33%)
  progress: bundling: b1 2/3 files (66.67%)
  progress: bundling: x 3/3 files (100.00%)
  progress: bundling (end)
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload


== Test bundling no commits

  $ hg bundle -r 'public()' no-output.hg
  abort: no commits to bundle
  [255]

  $ cd ..

When user merges to the revision existing only in the bundle,
it should show warning that second parent of the working
directory does not exist

  $ newclientrepo update2bundled
  $ cat <<EOF >> .hg/hgrc
  > [extensions]
  > strip =
  > EOF
  $ echo "aaa" >> a
  $ hg commit -A -m 0
  adding a
  $ echo "bbb" >> b
  $ hg commit -A -m 1
  adding b
  $ echo "ccc" >> c
  $ hg commit -A -m 2
  adding c
  $ hg goto -r 'desc(1)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "ddd" >> d
  $ hg commit -A -m 3
  adding d
  $ hg goto -r 'desc(2)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -G
  o  commit:      8bd3e1f196af
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     3
  │
  │ @  commit:      4652c276ac4f
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     2
  │
  o  commit:      a01eca7af26d
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     1
  │
  o  commit:      4fe08cd4693e
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     0
  
  $ hg bundle --base 'desc(1)' -r 'desc(3)' ../update2bundled.hg
  1 changesets found
  $ hg debugstrip -r 'desc(3)'
