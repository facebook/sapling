Skip for now

  $ exit 80

  $ echo '[phase]' >> $HGRCPATH
  $ echo 'publish=False' >> $HGRCPATH
  $ echo '[experimental]' >> $HGRCPATH
  $ echo 'bundle2-exp=True' >> $HGRCPATH
  $ echo '[ui]' >> $HGRCPATH
  $ echo 'ssh = python "$TESTDIR/dummyssh"' >> $HGRCPATH

Set up a repo

  $ hg init repo1
  $ cd repo1
  $ touch a
  $ hg add a
  $ hg ci -m a
  $ touch b
  $ hg add b
  $ hg ci -m b

Pull it using the unholy extension

  $ hg init ../repo2
  $ cd ../repo2
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "b2xcompat = $TESTDIR/../b2xcompat.py" >> $HGRCPATH
  $ hg pull ../repo1 --debug
  pulling from ../repo1
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  query 1; heads
  requesting all changes
  2 changesets found
  list of changesets:
  3903775176ed42b1458a6281db4a0ccf4d9f287a
  0e067c57feba1a5694ca4844f05588bb1bf82342
  listing keys for "phase"
  listing keys for "bookmarks"
  start emission of HG2Y stream
  bundle parameter: 
  start of parts
  bundle part: "b2x:changegroup"
  bundling: 1/2 changesets (50.00%)
  bundling: 2/2 changesets (100.00%)
  bundling: 1/2 manifests (50.00%)
  bundling: 2/2 manifests (100.00%)
  bundling: a 1/2 files (50.00%)
  bundling: b 2/2 files (100.00%)
  bundle part: "b2x:listkeys"
  bundle part: "b2x:listkeys"
  end of bundle
  start processing of HG2Y stream
  reading bundle2 stream parameters
  start extraction of bundle2 parts
  part header size: 33
  part type: "B2X:CHANGEGROUP"
  part id: "0"
  part parameters: 1
  found a handler for part 'b2x:changegroup'
  adding changesets
  payload chunk size: 934
  payload chunk size: 0
  changesets: 1 chunks
  add changeset 3903775176ed
  changesets: 2 chunks
  add changeset 0e067c57feba
  adding manifests
  manifests: 1/2 chunks (50.00%)
  manifests: 2/2 chunks (100.00%)
  adding file changes
  adding a revisions
  files: 1/2 chunks (50.00%)
  adding b revisions
  files: 2/2 chunks (100.00%)
  added 2 changesets with 2 changes to 2 files
  couldn't read revision branch cache names: [Errno 2] No such file or directory: '$TESTTMP/repo2/.hg/cache/rbc-names-v1'
  part header size: 35
  part type: "B2X:LISTKEYS"
  part id: "1"
  part parameters: 1
  found a handler for part 'b2x:listkeys'
  payload chunk size: 0
  part header size: 39
  part type: "B2X:LISTKEYS"
  part id: "2"
  part parameters: 1
  found a handler for part 'b2x:listkeys'
  payload chunk size: 0
  part header size: 0
  end of bundle2 stream
  checking for updated bookmarks
  listing keys for "phases"
  updating the branch cache
  (run 'hg update' to get a working copy)

Push back

  $ hg up
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch c
  $ hg add c
  $ hg ci -m c
  $ hg push ../repo1 --debug
  pushing to ../repo1
  query 1; heads
  searching for changes
  all remote heads known locally
  listing keys for "phases"
  checking for updated bookmarks
  listing keys for "bookmarks"
  listing keys for "bookmarks"
  1 changesets found
  list of changesets:
  991a3460af53952d10ec8a295d3d2cc2e5fa9690
  start emission of HG2Y stream
  bundle parameter: 
  start of parts
  bundle part: "b2x:replycaps"
  bundle part: "b2x:check:heads"
  bundle part: "b2x:changegroup"
  bundling: 1/1 changesets (100.00%)
  bundling: 1/1 manifests (100.00%)
  bundling: c 1/1 files (100.00%)
  bundle part: "b2x:pushkey"
  end of bundle
  start processing of HG2Y stream
  reading bundle2 stream parameters
  start extraction of bundle2 parts
  part header size: 20
  part type: "B2X:REPLYCAPS"
  part id: "0"
  part parameters: 0
  found a handler for part 'b2x:replycaps'
  payload chunk size: 117
  payload chunk size: 0
  part header size: 22
  part type: "B2X:CHECK:HEADS"
  part id: "1"
  part parameters: 0
  found a handler for part 'b2x:check:heads'
  part header size: 33
  part type: "B2X:CHANGEGROUP"
  part id: "2"
  part parameters: 1
  found a handler for part 'b2x:changegroup'
  part header size: 92
  part type: "B2X:PUSHKEY"
  part id: "3"
  part parameters: 4
  found a handler for part 'b2x:pushkey'
  payload chunk size: 0
  part header size: 0
  end of bundle2 stream
  updating the branch cache
  start emission of HG2Y stream
  bundle parameter: 
  start of parts
  bundle part: "b2x:output"
  bundle part: "b2x:reply:changegroup"
  bundle part: "b2x:output"
  bundle part: "b2x:reply:pushkey"
  bundle part: "b2x:output"
  end of bundle
  start processing of HG2Y stream
  reading bundle2 stream parameters
  start extraction of bundle2 parts
  part header size: 31
  part type: "b2x:output"
  part id: "0"
  part parameters: 1
  found a handler for part 'b2x:output'
  payload chunk size: 45
  payload chunk size: 0
  remote: payload chunk size: 20
  remote: payload chunk size: 0
  part header size: 51
  part type: "b2x:reply:changegroup"
  part id: "1"
  part parameters: 2
  found a handler for part 'b2x:reply:changegroup'
  payload chunk size: 0
  part header size: 31
  part type: "b2x:output"
  part id: "2"
  part parameters: 1
  found a handler for part 'b2x:output'
  payload chunk size: 273
  payload chunk size: 0
  remote: adding changesets
  remote: payload chunk size: 480
  remote: payload chunk size: 0
  remote: changesets: 1 chunks
  remote: add changeset 991a3460af53
  remote: adding manifests
  remote: manifests: 1/1 chunks (100.00%)
  remote: adding file changes
  remote: adding c revisions
  remote: files: 1/1 chunks (100.00%)
  remote: added 1 changesets with 1 changes to 1 files
  part header size: 47
  part type: "B2X:REPLY:PUSHKEY"
  part id: "3"
  part parameters: 2
  found a handler for part 'b2x:reply:pushkey'
  payload chunk size: 0
  part header size: 31
  part type: "b2x:output"
  part id: "4"
  part parameters: 1
  found a handler for part 'b2x:output'
  payload chunk size: 66
  payload chunk size: 0
  remote: pushing key for "phases:991a3460af53952d10ec8a295d3d2cc2e5fa9690"
  part header size: 0
  end of bundle2 stream
  listing keys for "phases"
  try to push obsolete markers to remote
