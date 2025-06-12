  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/proj2/foo/x = bbb\n
  > |
  > A  # A/proj1/foo/x = aaa\n
  >    # drawdag.defaultfiles=false
  > EOS

  $ hg go -q $B

Make a copy.  Note that this is not a subtree copy.
  $ cp -r proj1/foo proj1/bar
  $ cp -r proj2/foo proj2/bar
  $ hg add proj1/bar proj2/bar
  adding proj1/bar/x
  adding proj2/bar/x
  $ hg commit -m "copy foo to bar"

  $ echo ccc > proj1/foo/x
  $ echo ccc > proj2/foo/x
  $ hg commit -m "C"

  $ echo ddd > proj1/bar/x
  $ echo ddd > proj2/bar/x
  $ hg commit -m "D"

  $ tglog
  @  2ef2c3679bcb 'D'
  │
  o  89eb930a3ad2 'C'
  │
  o  4c687f77cec5 'copy foo to bar'
  │
  o  545926ee9897 'B'
  │
  o  7b1f8515a385 'A'

  $ hg subtree merge --from-path proj1/bar --to-path proj1/foo
  searching for merge base ...
  merge base: 545926ee9897
  merging proj1/foo/x and proj1/bar/x to proj1/foo/x
  warning: 1 conflicts while merging proj1/foo/x! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ echo cdcdcd > proj1/foo/x
  $ hg resolve --mark proj1/foo/x
  (no more unresolved files)
  $ hg commit -m "merge proj1"

  $ hg subtree inspect -r .
  {
    "merges": [
      {
        "version": 1,
        "from_commit": "2ef2c3679bcb5c7873d00820a1ca619dbf736051",
        "from_path": "proj1/bar",
        "to_path": "proj1/foo"
      }
    ]
  }

  $ hg subtree merge --from-path proj2/bar --to-path proj2/foo -r .^
  searching for merge base ...
  merge base: 545926ee9897
  merging proj2/foo/x and proj2/bar/x to proj2/foo/x
  warning: 1 conflicts while merging proj2/foo/x! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ echo dcdcdc > proj2/foo/x
  $ hg resolve --mark proj2/foo/x
  (no more unresolved files)
  $ hg commit -m "merge proj2"

  $ hg subtree inspect -r .
  {
    "merges": [
      {
        "version": 1,
        "from_commit": "2ef2c3679bcb5c7873d00820a1ca619dbf736051",
        "from_path": "proj2/bar",
        "to_path": "proj2/foo"
      }
    ]
  }

  $ hg fold --from .^ -m "merge proj1 and proj2"
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg subtree inspect -r .
  {
    "merges": [
      {
        "version": 1,
        "from_commit": "2ef2c3679bcb5c7873d00820a1ca619dbf736051",
        "from_path": "proj1/bar",
        "to_path": "proj1/foo"
      },
      {
        "version": 1,
        "from_commit": "2ef2c3679bcb5c7873d00820a1ca619dbf736051",
        "from_path": "proj2/bar",
        "to_path": "proj2/foo"
      }
    ]
  }

  $ echo eee > proj1/foo/x
  $ echo eee > proj2/bar/x
  $ hg commit -m "E"

  $ tglog
  @  859a27dec08e 'E'
  │
  o  a6d4fe868877 'merge proj1 and proj2'
  │
  o  2ef2c3679bcb 'D'
  │
  o  89eb930a3ad2 'C'
  │
  o  4c687f77cec5 'copy foo to bar'
  │
  o  545926ee9897 'B'
  │
  o  7b1f8515a385 'A'

  $ hg subtree merge --from-path proj1/bar --to-path proj1/foo
  searching for merge base ...
  found the last subtree merge commit a6d4fe868877
  merge base: 2ef2c3679bcb
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg commit -m "second merge proj1" 
  nothing changed
  [1]
  $ hg subtree merge --from-path proj2/bar --to-path proj2/foo -r .^
  searching for merge base ...
  found the last subtree merge commit a6d4fe868877
  merge base: 2ef2c3679bcb
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg commit -m "second merge proj2"
  nothing changed
  [1]
  $ hg fold --from .^ -m "second merge proj1 and proj2"
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ tglog
  @  d197b2cd180b 'second merge proj1 and proj2'
  │
  o  2ef2c3679bcb 'D'
  │
  o  89eb930a3ad2 'C'
  │
  o  4c687f77cec5 'copy foo to bar'
  │
  o  545926ee9897 'B'
  │
  o  7b1f8515a385 'A'

  $ hg subtree inspect -r .
  {
    "merges": [
      {
        "version": 1,
        "from_commit": "2ef2c3679bcb5c7873d00820a1ca619dbf736051",
        "from_path": "proj1/bar",
        "to_path": "proj1/foo"
      },
      {
        "version": 1,
        "from_commit": "2ef2c3679bcb5c7873d00820a1ca619dbf736051",
        "from_path": "proj2/bar",
        "to_path": "proj2/foo"
      }
    ]
  }
