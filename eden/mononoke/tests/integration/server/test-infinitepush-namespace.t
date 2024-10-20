# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^(infinitepush1|infinitepush2)/.+$' setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > infinitepush=
  > commitcloud=
  > [infinitepush]
  > server=False
  > branchpattern=re:(infinitepush1|bad)/.+
  > EOF

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ touch a && hg addremove && hg ci -q -ma
  adding a
  $ hg log -T '{short(node)}\n'
  3903775176ed

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-push and repo-pull
  $ hg clone -q mono:repo repo-push --noupdate
  $ hg clone -q mono:repo repo-pull --noupdate

blobimport

  $ blobimport repo/.hg repo

start mononoke

  $ start_and_wait_for_mononoke_server
Prepare push
  $ cd repo-push
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ hg addremove -q
  $ hg ci -m new

Valid infinitepush, with pushrebase disabled
  $ hg push -r . --to "infinitepush1/123" --create
  pushing to mono:repo
  searching for changes

Valid infinitepush, with pushrebase enabled
  $ hg push -r . --to "infinitepush1/456" --create --config extensions.pushrebase=
  pushing to mono:repo
  searching for changes

Invalid infinitepush, with pushrebase disabled
  $ hg push -r . --to "bad/123" --create
  pushing to mono:repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing an infinitepush
  remote: 
  remote:   Root cause:
  remote:     Invalid scratch bookmark: bad/123 (scratch bookmarks must match pattern ^(infinitepush1|infinitepush2)/.+$)
  remote: 
  remote:   Caused by:
  remote:     Failed to create scratch bookmark
  remote:   Caused by:
  remote:     Invalid scratch bookmark: bad/123 (scratch bookmarks must match pattern ^(infinitepush1|infinitepush2)/.+$)
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing an infinitepush",
  remote:         source: Error {
  remote:             context: "Failed to create scratch bookmark",
  remote:             source: InvalidScratchBookmark {
  remote:                 bookmark: BookmarkKey {
  remote:                     name: BookmarkName {
  remote:                         bookmark: "bad/123",
  remote:                     },
  remote:                     category: Branch,
  remote:                 },
  remote:                 pattern: "^(infinitepush1|infinitepush2)/.+$",
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Invalid infinitepush, with pushrebase enabled
  $ hg push -r . --to "bad/456" --create --config extensions.pushrebase=
  pushing to mono:repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing an infinitepush
  remote: 
  remote:   Root cause:
  remote:     Invalid scratch bookmark: bad/456 (scratch bookmarks must match pattern ^(infinitepush1|infinitepush2)/.+$)
  remote: 
  remote:   Caused by:
  remote:     Failed to create scratch bookmark
  remote:   Caused by:
  remote:     Invalid scratch bookmark: bad/456 (scratch bookmarks must match pattern ^(infinitepush1|infinitepush2)/.+$)
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing an infinitepush",
  remote:         source: Error {
  remote:             context: "Failed to create scratch bookmark",
  remote:             source: InvalidScratchBookmark {
  remote:                 bookmark: BookmarkKey {
  remote:                     name: BookmarkName {
  remote:                         bookmark: "bad/456",
  remote:                     },
  remote:                     category: Branch,
  remote:                 },
  remote:                 pattern: "^(infinitepush1|infinitepush2)/.+$",
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Valid push, with pushrebase disabled
  $ hg push -r . --to "plain/123" --create
  pushing rev 47da8b81097c to destination mono:repo bookmark plain/123
  searching for changes
  no changes found
  exporting bookmark plain/123

Valid push, with pushrebase enabled
  $ hg push -r . --to "plain/456" --create --config extensions.pushrebase=
  pushing rev 47da8b81097c to destination mono:repo bookmark plain/456
  searching for changes
  no changes found
  exporting bookmark plain/456

Invalid push, with pushrebase disabled
  $ hg push -r . --to "infinitepush2/123" --create
  pushing rev 47da8b81097c to destination mono:repo bookmark infinitepush2/123
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:   Root cause:
  remote:     Invalid publishing bookmark: infinitepush2/123 (only scratch bookmarks may match pattern ^(infinitepush1|infinitepush2)/.+$)
  remote: 
  remote:   Caused by:
  remote:     Failed to create bookmark
  remote:   Caused by:
  remote:     Invalid publishing bookmark: infinitepush2/123 (only scratch bookmarks may match pattern ^(infinitepush1|infinitepush2)/.+$)
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a push",
  remote:         source: Error {
  remote:             context: "Failed to create bookmark",
  remote:             source: InvalidPublishingBookmark {
  remote:                 bookmark: BookmarkKey {
  remote:                     name: BookmarkName {
  remote:                         bookmark: "infinitepush2/123",
  remote:                     },
  remote:                     category: Branch,
  remote:                 },
  remote:                 pattern: "^(infinitepush1|infinitepush2)/.+$",
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Invalid push, with pushrebase enabled
  $ hg push -r . --to "infinitepush2/456" --create --config extensions.pushrebase=
  pushing rev 47da8b81097c to destination mono:repo bookmark infinitepush2/456
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a bookmark-only pushrebase
  remote: 
  remote:   Root cause:
  remote:     Invalid publishing bookmark: infinitepush2/456 (only scratch bookmarks may match pattern ^(infinitepush1|infinitepush2)/.+$)
  remote: 
  remote:   Caused by:
  remote:     Failed to create bookmark
  remote:   Caused by:
  remote:     Invalid publishing bookmark: infinitepush2/456 (only scratch bookmarks may match pattern ^(infinitepush1|infinitepush2)/.+$)
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a bookmark-only pushrebase",
  remote:         source: Error {
  remote:             context: "Failed to create bookmark",
  remote:             source: InvalidPublishingBookmark {
  remote:                 bookmark: BookmarkKey {
  remote:                     name: BookmarkName {
  remote:                         bookmark: "infinitepush2/456",
  remote:                     },
  remote:                     category: Branch,
  remote:                 },
  remote:                 pattern: "^(infinitepush1|infinitepush2)/.+$",
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Check everything is as expected
  $ cd ..
  $ cd repo-pull
  $ hg pull
  pulling from mono:repo
  $ hg bookmarks --remote
     remote/master_bookmark           3903775176ed42b1458a6281db4a0ccf4d9f287a
     remote/plain/123                 47da8b81097c5534f3eb7947a8764dd323cffe3d
     remote/plain/456                 47da8b81097c5534f3eb7947a8764dd323cffe3d
  $ hg bookmarks --list-remote "*"
     infinitepush1/123         47da8b81097c5534f3eb7947a8764dd323cffe3d
     infinitepush1/456         47da8b81097c5534f3eb7947a8764dd323cffe3d
     master_bookmark           3903775176ed42b1458a6281db4a0ccf4d9f287a
     plain/123                 47da8b81097c5534f3eb7947a8764dd323cffe3d
     plain/456                 47da8b81097c5534f3eb7947a8764dd323cffe3d
