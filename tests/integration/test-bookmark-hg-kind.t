  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ LIST_KEYS_PATTERNS_MAX=6 setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend=
  > infinitepush=
  > infinitepushbackup=
  > EOF

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -m 'add a'
  adding a
  $ hg log -T '{short(node)}\n'
  ac82d8b1f7c4

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-push, repo-pull
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport

  $ blobimport repo-hg/.hg repo
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT name, hg_kind FROM bookmarks;'
  master_bookmark|pull_default
start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

create new bookmarks, then update their properties
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > EOF
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch b && hg addremove && hg ci -q -m 'add b'
  adding b
  $ hgmn push ssh://user@dummy/repo -r . --to "not_pull_default" --create
  pushing rev 907767d421e4 to destination ssh://user@dummy/repo bookmark not_pull_default
  searching for changes
  exporting bookmark not_pull_default
  $ touch c && hg addremove && hg ci -q -m 'add c'
  adding c
  $ hgmn push ssh://user@dummy/repo -r . --to "scratch" --create
  pushing rev b2d646f64a99 to destination ssh://user@dummy/repo bookmark scratch
  searching for changes
  exporting bookmark scratch
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks SET hg_kind = CAST('scratch' AS BLOB) WHERE name LIKE 'scratch';"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks SET hg_kind = CAST('publishing' AS BLOB) WHERE name LIKE 'not_pull_default';"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT name, hg_kind FROM bookmarks;'
  master_bookmark|pull_default
  not_pull_default|publishing
  scratch|scratch
  $ tglogpnr
  @  b2d646f64a99 public 'add c'
  |
  o  907767d421e4 public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  
test publishing
  $ cd "$TESTTMP/repo-pull"
  $ tglogpnr
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  
  $ hgmn pull
  pulling from ssh://user@dummy/rep* (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 907767d421e4
  $ hgmn up 907767d421e4cb28c7978bedef8ccac7242b155e
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgmn up b2d646f64a9978717516887968786c6b7a33edf9
  'b2d646f64a9978717516887968786c6b7a33edf9' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets b2d646f64a99
  'b2d646f64a9978717516887968786c6b7a33edf9' found remotely
  pull finished in * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglogpnr
  @  b2d646f64a99 draft 'add c'
  |
  o  907767d421e4 public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  
  $ hgmn bookmarks
     master_bookmark           0:ac82d8b1f7c4
  $ hgmn bookmarks --list-remote "*"
     master_bookmark           ac82d8b1f7c418c61a493ed229ffaa981bda8e90
     not_pull_default          907767d421e4cb28c7978bedef8ccac7242b155e
     scratch                   b2d646f64a9978717516887968786c6b7a33edf9
Exercise the limit (5 bookmarks should be allowed, this was our limit)
  $ cd ../repo-push
  $ hgmn push ssh://user@dummy/repo -r . --to "more/1" --create >/dev/null 2>&1
  $ hgmn push ssh://user@dummy/repo -r . --to "more/2" --create >/dev/null 2>&1
  [1]
  $ hgmn bookmarks --list-remote "*"
     master_bookmark           ac82d8b1f7c418c61a493ed229ffaa981bda8e90
     more/1                    b2d646f64a9978717516887968786c6b7a33edf9
     more/2                    b2d646f64a9978717516887968786c6b7a33edf9
     not_pull_default          907767d421e4cb28c7978bedef8ccac7242b155e
     scratch                   b2d646f64a9978717516887968786c6b7a33edf9
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks SET hg_kind = CAST('scratch' AS BLOB) WHERE name LIKE 'more/%';"
Exercise the limit (6 bookmarks should fail)
  $ hgmn push ssh://user@dummy/repo -r . --to "more/3" --create >/dev/null 2>&1
  $ hgmn bookmarks --list-remote "*"
  remote: Command failed
  remote:   Error:
  remote:     Bookmark query was truncated after 6 results, use a more specific prefix search.
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Bookmark query was truncated after 6 results, use a more specific prefix search.",
  remote:     }
  abort: unexpected response: empty string
  [255]

Narrowing down our query should fix it:
  $ hgmn bookmarks --list-remote "more/*"
     more/1                    b2d646f64a9978717516887968786c6b7a33edf9
     more/2                    b2d646f64a9978717516887968786c6b7a33edf9
     more/3                    b2d646f64a9978717516887968786c6b7a33edf9
