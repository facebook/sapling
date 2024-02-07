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

Try scrubbing hg file contents
  $ mononoke_walker scrub --chunk-direction=OldestFirst --chunk-by-public=BonsaiHgMapping -I deep \
  > -i=bonsai  -i=hg -i=FileContent -x=HgFileNode -I=marker -q 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Repo bounds: (1, 7), repo: repo
  Starting chunk 1 with bounds (1, 7), repo: repo
  Execution error: Could not step to OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None } via None in repo repo (glob)
  
  Caused by:
      0: Error while deserializing changeset retrieved from key 'hgchangeset.sha1.*' (glob)
      1: can't get time/extra
      2: not enough parts
  Error: Execution failed

Try scrubbing hg filenodes
  $ mononoke_walker scrub --chunk-direction=OldestFirst --chunk-by-public=BonsaiHgMapping -I deep \
  > -i=bonsai  -i=hg -x=FileContent -x=HgFileEnvelope -i=HgFileNode -q 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Repo bounds: (1, 7), repo: repo
  Starting chunk 1 with bounds (1, 7), repo: repo
  Execution error: Could not step to OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None } via None in repo repo (glob)
  
  Caused by:
      0: Error while deserializing changeset retrieved from key 'hgchangeset.sha1.*' (glob)
      1: can't get time/extra
      2: not enough parts
  Error: Execution failed

Basic case, deep scrub of the good branch still works
  $ mononoke_walker scrub -I deep -q -b good 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Seen,Loaded: 25,25, repo: repo
  Bytes/s,*, repo: repo (glob)
  Walked*, repo: repo (glob)

Basic case, deep scrub of the bad branch does work only because of current mitigations
and only because the bad commit is a head of the branch.
  $ mononoke_walker scrub -I deep -q -b bad 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Seen,Loaded: 2,2, repo: repo
  Bytes/s,*, repo: repo (glob)
  Walked*, repo: repo (glob)

Basic case, deep scrub of the bad branch that doesn't have bad commit at head.
  $ mononoke_walker scrub -I deep -q -r HgChangeset:d4775aa0d65c35f8b71fbf9ea44a759b8d817ce7 2>&1 | strip_glog
  Walking edge types *, repo: repo (glob)
  Walking node types *, repo: repo (glob)
  Execution error: Could not step to OutgoingEdge { label: HgChangesetToHgParent, target: HgChangesetViaBonsai(ChangesetKey { inner: HgChangesetId(HgNodeHash(Sha1(6d7e2d6f0ed4ce975af19d70754704b279e4fd35))), filenode_known_derived: false }), path: None } via Some(EmptyRoute) in repo repo
  
  Caused by:
      0: Error while deserializing changeset retrieved from key 'hgchangeset.sha1.6d7e2d6f0ed4ce975af19d70754704b279e4fd35'
      1: can't get time/extra
      2: not enough parts
  Error: Execution failed
