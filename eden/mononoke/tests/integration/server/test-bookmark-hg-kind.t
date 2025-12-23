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

  $ enable amend commitcloud

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ quiet testtool_drawdag -R repo <<EOF
  > A
  > # modify: A "a" "content"
  > # bookmark: A master_bookmark
  > EOF

  $ cd $TESTTMP

setup repo-push, repo-pull
  $ hg clone -q mono:repo repo-push --noupdate
  $ hg clone -q mono:repo repo-pull --noupdate

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT name, hg_kind FROM bookmarks;'
  master_bookmark|pull_default
start mononoke

  $ start_and_wait_for_mononoke_server
create new bookmarks, then update their properties
  $ cd repo-push
  $ hg pull -q
  $ hg up -q "min(all())"
  $ touch b && hg addremove && hg ci -q -m 'add b'
  adding b
  $ hg push -r . --to "not_pull_default" --create
  pushing rev 8db75f0f53d8 to destination mono:repo bookmark not_pull_default
  searching for changes
  exporting bookmark not_pull_default
  $ touch c && hg addremove && hg ci -q -m 'add c'
  adding c
  $ hg push -r . --to "scratch" --create
  pushing rev 20396342f200 to destination mono:repo bookmark scratch
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
  @  20396342f200 draft 'add c'
  │
  o  8db75f0f53d8 draft 'add b'
  │
  o  43806d3afe2b public 'A'  remote/master_bookmark
  

test publishing
  $ cd "$TESTTMP/repo-pull"
  $ tglogpnr
  $ hg pull
  pulling from mono:repo
  imported commit graph for 1 commit (1 segment)
  $ hg up 8db75f0f53d84e2684b2368e760cfb3555875a53
  pulling '8db75f0f53d84e2684b2368e760cfb3555875a53' from 'mono:repo'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up 20396342f20004f5f3c139f12681b3dd07b08d8b
  pulling '20396342f20004f5f3c139f12681b3dd07b08d8b' from 'mono:repo'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglogpnr
  @  20396342f200 draft 'add c'
  │
  o  8db75f0f53d8 draft 'add b'
  │
  o  43806d3afe2b public 'A'  remote/master_bookmark
  

  $ hg bookmarks
  no bookmarks set
  $ hg bookmarks --list-remote "*"
     master_bookmark           43806d3afe2b66f1765d44d1191c66cd3adbbe93
     not_pull_default          8db75f0f53d84e2684b2368e760cfb3555875a53
     scratch                   20396342f20004f5f3c139f12681b3dd07b08d8b
Exercise the limit (5 bookmarks should be allowed, this was our limit)
  $ cd ../repo-push
  $ hg push -r . --to "more/1" --create >/dev/null 2>&1
  $ hg push -r . --to "more/2" --create >/dev/null 2>&1
  $ hg bookmarks --list-remote "*"
     master_bookmark           43806d3afe2b66f1765d44d1191c66cd3adbbe93
     more/1                    20396342f20004f5f3c139f12681b3dd07b08d8b
     more/2                    20396342f20004f5f3c139f12681b3dd07b08d8b
     not_pull_default          8db75f0f53d84e2684b2368e760cfb3555875a53
     scratch                   20396342f20004f5f3c139f12681b3dd07b08d8b
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks SET hg_kind = CAST('scratch' AS BLOB) WHERE CAST(name AS TEXT) LIKE 'more/%';"
  $ flush_mononoke_bookmarks
Exercise the limit (6 bookmarks should fail)
  $ hg push -r . --to "more/3" --create >/dev/null 2>&1
  $ hg bookmarks --list-remote "*"
  remote: Command failed
  remote:   Error:
  remote:     Bookmark query was truncated after 6 results, use a more specific prefix search.
  abort: unexpected EOL, expected netstring digit
  [255]

Narrowing down our query should fix it:
  $ hg bookmarks --list-remote "more/*"
     more/1                    20396342f20004f5f3c139f12681b3dd07b08d8b
     more/2                    20396342f20004f5f3c139f12681b3dd07b08d8b
     more/3                    20396342f20004f5f3c139f12681b3dd07b08d8b
