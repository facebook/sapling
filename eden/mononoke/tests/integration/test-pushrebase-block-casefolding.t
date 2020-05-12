# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ DISALLOW_NON_PUSHREBASE=1 BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'
  $ hg up -q master_bookmark

Create commit which only differs in case
  $ touch foo.txt Foo.txt
  $ hg ci -Aqm commit1

Push the commit
  $ hgmn push -r . --to master_bookmark
  pushing rev 143fbdc73580 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Error while uploading data for changesets, hashes: [HgChangesetId(HgNodeHash(Sha1(143fbdc73580e33c8432457df2a10e1038936a72)))]
  remote: 
  remote:   Root cause:
  remote:     CaseConflict: the changes introduced by this commit have conflicting case. The first offending path is 'foo.txt'. Resolve the conflict.
  remote: 
  remote:   Caused by:
  remote:     While creating Changeset Some(HgNodeHash(Sha1(143fbdc73580e33c8432457df2a10e1038936a72))), uuid: * (glob)
  remote:   Caused by:
  remote:     While computing changed files
  remote:   Caused by:
  remote:     CaseConflict: the changes introduced by this commit have conflicting case. The first offending path is 'foo.txt'. Resolve the conflict.
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "Error while uploading data for changesets, hashes: [HgChangesetId(HgNodeHash(Sha1(143fbdc73580e33c8432457df2a10e1038936a72)))]",
  remote:         source: SharedError {
  remote:             error: Error {
  remote:                 context: "While creating Changeset Some(HgNodeHash(Sha1(143fbdc73580e33c8432457df2a10e1038936a72))), uuid: *", (glob)
  remote:                 source: Error {
  remote:                     context: "While computing changed files",
  remote:                     source: InternalCaseConflict(
  remote:                         MPath("foo.txt"),
  remote:                     ),
  remote:                 },
  remote:             },
  remote:         },
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
