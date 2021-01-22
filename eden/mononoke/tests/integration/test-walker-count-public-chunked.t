# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_pre_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  $ blobimport repo-hg/.hg repo --derived-data-type=blame --derived-data-type=changeset_info --derived-data-type=deleted_manifest --derived-data-type=fastlog --derived-data-type=fsnodes --derived-data-type=skeleton_manifests --derived-data-type=unodes

bonsai core data, deep, unchunked. This is the base case
  $ mononoke_walker -L sizing scrub -q -b master_bookmark -I bonsai 2>&1 | strip_glog
  Walking edge types [BookmarkToChangeset, ChangesetToBonsaiParent, ChangesetToFileContent]
  Walking node types [Bookmark, Changeset, FileContent]
  Seen,Loaded: 7,7
  * Type:Walked,Checks,Children Bookmark:1,1,2 Changeset:3,* FileContent:3,3,0 (glob)

bonsai core data, chunked, shallow.  Shallow walk with chunked commits should still visit all changesets, but no bookmark
  $ mononoke_walker -L sizing scrub -q -p Changeset --chunk-size=2 -I shallow -i bonsai -i FileContent 2>&1 | strip_glog
  Walking edge types [ChangesetToFileContent]
  Walking node types [Changeset, FileContent]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Changeset:2,*,4 FileContent:2,2,0 (glob)
  Deferred: 0
  Seen,Loaded: 2,2
  * Type:Walked,Checks,Children Changeset:3,*,6 FileContent:3,3,0 (glob)
  Deferred: 0
  Completed in 2 chunks of 2

bonsai core data, chunked, deep. Should still visit all changesets, but no bookmark, second chunk has one deferred edge to process
  $ mononoke_walker -L sizing scrub -q -p Changeset --chunk-size=2 -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Walking edge types [ChangesetToBonsaiParent, ChangesetToFileContent]
  Walking node types [Changeset, FileContent]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Changeset:2,*,4 FileContent:2,2,0 (glob)
  Deferred: 1
  Seen,Loaded: 3,3
  * Type:Walked,Checks,Children Changeset:4,*,6 FileContent:3,*,0 (glob)
  Deferred: 0
  Completed in 2 chunks of 2
