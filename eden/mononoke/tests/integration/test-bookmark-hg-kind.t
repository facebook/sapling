# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
Disable boookmarks cache because we manually modify bookmarks table
  $ LIST_KEYS_PATTERNS_MAX=6 setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests

  $ enable amend infinitepush infinitepushbackup remotenames

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

  $ start_and_wait_for_mononoke_server
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
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --to "not_pull_default" --create
  pushing rev 907767d421e4 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark not_pull_default
  searching for changes
  exporting bookmark not_pull_default
  $ touch c && hg addremove && hg ci -q -m 'add c'
  adding c
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --to "scratch" --create
  pushing rev b2d646f64a99 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark scratch
  searching for changes
  exporting bookmark scratch
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks SET hg_kind = CAST('scratch' AS BLOB) WHERE CAST(name AS TEXT) LIKE 'scratch';"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks SET hg_kind = CAST('publishing' AS BLOB) WHERE CAST(name AS TEXT) LIKE 'not_pull_default';"
  $ flush_mononoke_bookmarks
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT name, hg_kind FROM bookmarks;'
  master_bookmark|pull_default
  not_pull_default|publishing
  scratch|scratch
  $ tglogpnr
  @  b2d646f64a99 public 'add c'  default/scratch
  │
  o  907767d421e4 public 'add b'  default/not_pull_default
  │
  o  ac82d8b1f7c4 public 'add a'  default/master_bookmark
  
test publishing
  $ cd "$TESTTMP/repo-pull"
  $ tglogpnr
  o  ac82d8b1f7c4 public 'add a'  default/master_bookmark
  
  $ hgmn pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hgmn up 907767d421e4cb28c7978bedef8ccac7242b155e
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgmn up b2d646f64a9978717516887968786c6b7a33edf9
  pulling 'b2d646f64a9978717516887968786c6b7a33edf9' from 'mononoke://$LOCALIP:$LOCAL_PORT/repo'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglogpnr
  @  b2d646f64a99 draft 'add c'
  │
  o  907767d421e4 draft 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'  default/master_bookmark
  
  $ hgmn bookmarks
  no bookmarks set
  $ hgmn bookmarks --list-remote "*"
     master_bookmark           ac82d8b1f7c418c61a493ed229ffaa981bda8e90
     not_pull_default          907767d421e4cb28c7978bedef8ccac7242b155e
     scratch                   b2d646f64a9978717516887968786c6b7a33edf9
Exercise the limit (5 bookmarks should be allowed, this was our limit)
  $ cd ../repo-push
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --to "more/1" --create >/dev/null 2>&1
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --to "more/2" --create >/dev/null 2>&1
  $ hgmn bookmarks --list-remote "*"
     master_bookmark           ac82d8b1f7c418c61a493ed229ffaa981bda8e90
     more/1                    b2d646f64a9978717516887968786c6b7a33edf9
     more/2                    b2d646f64a9978717516887968786c6b7a33edf9
     not_pull_default          907767d421e4cb28c7978bedef8ccac7242b155e
     scratch                   b2d646f64a9978717516887968786c6b7a33edf9
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks SET hg_kind = CAST('scratch' AS BLOB) WHERE CAST(name AS TEXT) LIKE 'more/%';"
  $ flush_mononoke_bookmarks
Exercise the limit (6 bookmarks should fail)
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --to "more/3" --create >/dev/null 2>&1
  $ hgmn bookmarks --list-remote "*"
  remote: Command failed
  remote:   Error:
  remote:     Bookmark query was truncated after 6 results, use a more specific prefix search.
  remote: 
  remote:   Root cause:
  remote:     Bookmark query was truncated after 6 results, use a more specific prefix search.
  remote: 
  remote:   Debug context:
  remote:     "Bookmark query was truncated after 6 results, use a more specific prefix search."
  abort: unexpected EOL, expected netstring digit
  [255]

Narrowing down our query should fix it:
  $ hgmn bookmarks --list-remote "more/*"
     more/1                    b2d646f64a9978717516887968786c6b7a33edf9
     more/2                    b2d646f64a9978717516887968786c6b7a33edf9
     more/3                    b2d646f64a9978717516887968786c6b7a33edf9
