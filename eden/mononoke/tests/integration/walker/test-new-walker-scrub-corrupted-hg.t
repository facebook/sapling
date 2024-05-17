# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Unpack repo with corrupted hg commit - the author has a newline.
repo structure is a follows
> A-B-C
>    \-D
>     \-E-F
> # bookmark: C good
> # bookmark: D bad
> EOF

Commits D and E have corrupted author field containing newline ("te\nst")

NOTE: we're using only the monsql and blobstore dirs from the tarred repo dir.
Configs are generated from scratch so we don't have to update them in fixtures too often

  $ REPONAME="repo" setup_common_config "blob_files"
  $ cd "$TESTTMP"
  $ tar --strip-components=1 -xf "$TEST_FIXTURES/fixtures/repo_with_newline_author_commit.tar.xz" repo_with_newline_author_commit/blobstore repo_with_newline_author_commit/monsql
Check that indeed the bad commit is bad
  $ mononoke_newadmin fetch -R repo -B bad
  Error: Failed to load changeset c567ecc582f8822cf1529a127dec105db78a440fbeaa21221ce2abc4affff6ec
  
  Caused by:
      0: invalid Thrift structure 'BonsaiChangeset': Invalid changeset
      1: invalid bonsai changeset: commit author contains a newline at offset 2
  [1]

Try scrubbing hg file contents (as we regularly do in production)
  $ mononoke_walker scrub --chunk-direction=OldestFirst --chunk-by-public=BonsaiHgMapping -I deep \
  > -i=bonsai  -i=hg -i=FileContent -x=HgFileNode -I=marker \
  > --exclude-node BonsaiHgMapping:c567ecc582f8822cf1529a127dec105db78a440fbeaa21221ce2abc4affff6ec \
  > --exclude-node BonsaiHgMapping:17090c5bf061aa21f7aa2393796b7bb9a20c81a3940b2aaab7683f1e10c67978 \
  > -q 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Repo bounds: (1, 7), repo: repo
  Starting chunk 1 with bounds (1, 7), repo: repo
  Suppressing edge OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None }, repo: repo (glob)
  Suppressing edge OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None }, repo: repo (glob)
  Seen,Loaded: 4,4, repo: repo
  Bytes/s,*, repo: repo* (glob)
  Walked*, repo: repo* (glob)
  Deferred: 0, repo: repo
  Completed in 1 chunks of size 100000, repo: repo

Try scrubbing hg filenodes (as we regularly do in production)
  $ mononoke_walker scrub --chunk-direction=OldestFirst --chunk-by-public=BonsaiHgMapping -I deep \
  > -i=bonsai  -i=hg -i=FileContent -x=HgFileEnvelope -i=HgFileNode \
  > --exclude-node BonsaiHgMapping:c567ecc582f8822cf1529a127dec105db78a440fbeaa21221ce2abc4affff6ec \
  > --exclude-node BonsaiHgMapping:17090c5bf061aa21f7aa2393796b7bb9a20c81a3940b2aaab7683f1e10c67978 \
  > -q 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Repo bounds: (1, 7), repo: repo
  Starting chunk 1 with bounds (1, 7), repo: repo
  Suppressing edge OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None }, repo: repo (glob)
  Suppressing edge OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None }, repo: repo (glob)
  Seen,Loaded: 4,4, repo: repo
  Bytes/s,*, repo: repo* (glob)
  Walked*, repo: repo* (glob)
  Deferred: 0, repo: repo
  Completed in 1 chunks of size 100000, repo: repo

Basic case, deep scrub of the good branch still works
  $ mononoke_walker scrub -I deep -q -b good 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Seen,Loaded: 25,25, repo: repo
  Bytes/s,*, repo: repo (glob)
  Walked*, repo: repo (glob)

Basic case, deep scrub of the bad branch does work only because of current mitigations
and only because the bad commit is a head of the branch.
  $ mononoke_walker scrub -I deep -q -b bad \
  > --exclude-node Changeset:c567ecc582f8822cf1529a127dec105db78a440fbeaa21221ce2abc4affff6ec \
  > 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Seen,Loaded: 1,1, repo: repo
  Bytes/s,*, repo: repo (glob)
  Walked*, repo: repo (glob)

Basic case, deep scrub of the bad branch that doesn't have bad commit at head.
  $ mononoke_walker scrub -I deep -q -r HgChangeset:d4775aa0d65c35f8b71fbf9ea44a759b8d817ce7 \
  > --exclude-node BonsaiHgMapping:17090c5bf061aa21f7aa2393796b7bb9a20c81a3940b2aaab7683f1e10c67978 \
  > --exclude-node Changeset:17090c5bf061aa21f7aa2393796b7bb9a20c81a3940b2aaab7683f1e10c67978 \
  > --exclude-node HgChangeset:6d7e2d6f0ed4ce975af19d70754704b279e4fd35 \
  > --exclude-node HgChangesetViaBonsai:6d7e2d6f0ed4ce975af19d70754704b279e4fd35 \
  > 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Seen,Loaded: 34,34, repo: repo
  Bytes/s,*, repo: repo (glob)
  Walked*, repo: repo (glob)
