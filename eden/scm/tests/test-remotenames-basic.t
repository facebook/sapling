#debugruntest-compatible
  $ enable remotenames
  $ setconfig experimental.allowfilepeer=True
  > mkcommit()
  > {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "add $1"
  > }


Test that remotenames works on a repo without any names file

  $ hg init alpha
  $ cd alpha
  $ mkcommit a
  $ mkcommit b
  $ hg log -r 'upstream()'
  $ hg log -r . -T '{remotenames} {remotebookmarks}\n'
   

Continue testing

  $ mkcommit c
  $ cd ..
  $ hg clone alpha beta
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd beta
  $ hg book babar
  $ mkcommit d
  $ cd ..

  $ hg init gamma
  $ cd gamma
  $ cat > .hg/hgrc <<EOF
  > [paths]
  > default = ../alpha
  > alpha = ../alpha
  > beta = ../beta
  > EOF
  $ hg pull
  pulling from $TESTTMP/alpha
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  $ hg pull beta
  pulling from $TESTTMP/beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg co -C default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit e

graph shows tags for the branch heads of each path
  $ hg log --graph
  @  commit:      9d206ffc875e
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     add e
  │
  o  commit:      47d2a3944de8
  │  bookmark:    beta/babar
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     add d
  │
  o  commit:      4538525df7e2
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     add c
  │
  o  commit:      7c3bad9141dc
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     add b
  │
  o  commit:      1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  

make sure we can list remote bookmarks with --all

  $ hg bookmarks --all
  no bookmarks set
     beta/babar                47d2a3944de8

  $ hg bookmarks --all -T json
  [
   {
    "node": "47d2a3944de8b013de3be9578e8e344ea2e6c097",
    "remotebookmark": "beta/babar"
   }
  ]
  $ hg bookmarks --remote
     beta/babar                47d2a3944de8

Verify missing node doesnt break remotenames

  $ hg dbsh << 'EOS'
  > ml["remotenames"] = ml["remotenames"] + b"18f8e0f8ba54270bf158734c781327581cf43634 bookmarks beta/foo\n"
  > ml.commit("add unknown ref to remotenames")
  > EOS
  $ hg book --remote --config remotenames.resolvenodes=False
     beta/babar                47d2a3944de8

But does break if the missing node is considered essential:

  $ hg book --remote --config remotenames.selectivepulldefault=foo
     beta/babar                47d2a3944de8
  abort: remotename entry beta/foo (18f8e0f8ba54270bf158734c781327581cf43634) cannot be found: 00changelog.i@18f8e0f8ba54: no node!
  (try 'hg doctor' to attempt to fix it)
  [255]

make sure bogus revisions in .hg/store/remotenames do not break hg
  $ echo deadbeefdeadbeefdeadbeefdeadbeefdeadbeef default/default >> \
  > .hg/store/remotenames
  $ hg parents
  commit:      9d206ffc875e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add e
  
Verify that the revsets operate as expected:
  $ hg log -r 'not pushed()'
  commit:      9d206ffc875e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add e
  


Upstream without configuration is synonymous with upstream('default'):
  $ hg log -r 'not upstream()'
  commit:      1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  commit:      7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add b
  
  commit:      4538525df7e2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add c
  
  commit:      47d2a3944de8
  bookmark:    beta/babar
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add d
  
  commit:      9d206ffc875e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add e
  

but configured, it'll do the expected thing:
  $ echo '[remotenames]' >> .hg/hgrc
  $ echo 'upstream=alpha' >> .hg/hgrc
  $ hg log --graph -r 'not upstream()'
  @  commit:      9d206ffc875e
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     add e
  │
  o  commit:      47d2a3944de8
  │  bookmark:    beta/babar
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     add d
  │
  o  commit:      4538525df7e2
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     add c
  │
  o  commit:      7c3bad9141dc
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     add b
  │
  o  commit:      1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  
  $ hg log --limit 2 --graph -r 'heads(upstream())'

Test remotenames revset and keyword

  $ hg log -r 'remotenames()' \
  >   --template '{node|short} {remotenames}\n'
  47d2a3944de8 beta/babar

Test remotebookmark revsets

  $ hg log -r 'remotebookmark()' \
  >   --template '{node|short} {remotebookmarks}\n'
  47d2a3944de8 beta/babar
  $ hg log -r 'remotebookmark("beta/babar")' \
  >   --template '{node|short} {remotebookmarks}\n'
  47d2a3944de8 beta/babar
  $ hg log -r 'remotebookmark("beta/stable")' \
  >   --template '{node|short} {remotebookmarks}\n'
  abort: no remote bookmarks exist that match 'beta/stable'!
  [255]
  $ hg log -r 'remotebookmark("re:beta/.*")' \
  >   --template '{node|short} {remotebookmarks}\n'
  47d2a3944de8 beta/babar
  $ hg log -r 'remotebookmark("re:gamma/.*")' \
  >   --template '{node|short} {remotebookmarks}\n'
  abort: no remote bookmarks exist that match 're:gamma/.*'!
  [255]


Test custom paths dont override default
  $ cd ..
  $ cd alpha
  $ hg book foo bar baz
  $ cd ..
  $ hg init path_overrides
  $ cd path_overrides
  $ hg path -a default ../alpha
  $ hg path -a custom ../alpha
  $ hg pull
  pulling from $TESTTMP/alpha
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  $ hg book --remote
     default/bar               4538525df7e2
     default/baz               4538525df7e2
     default/foo               4538525df7e2


Test json formatted bookmarks with tracking data
  $ cd ..
  $ hg init delta
  $ cd delta
  $ hg book mimimi -t lalala
  $ hg book -v -T json
  [
   {
    "active": true,
    "bookmark": "mimimi",
    "node": "0000000000000000000000000000000000000000",
    "tracking": "lalala"
   }
  ]
  $ hg book -v
   * mimimi                    000000000000           [lalala]
