# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"

  $ testtool_drawdag -R repo --derive-all << EOF
  > C
  > |
  > B
  > |
  > A
  > # bookmark: C master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Base case, check can walk fine
  $ mononoke_walker scrub -I deep -q -b master_bookmark 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 43,43

Delete a gitsha1 alias so that we get errors
  $ ls $TESTTMP/blobstore/blobs/* | wc -l
  138
  $ rm $TESTTMP/blobstore/blobs/*.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c*
  $ ls $TESTTMP/blobstore/blobs/* | wc -l
  137

Check we get an error due to the missing aliases
  $ mononoke_walker scrub -I deep -q -b master_bookmark 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types * (glob)
  [INFO] Walking node types * (glob)
  [ERROR] Execution error: Could not step to OutgoingEdge { label: FileContentMetadataV2ToGitSha1Alias, target: AliasContentMapping(AliasKey(GitSha1(GitSha1(96d80cd6c4e7158dbebd0849f4fb7ce513e5828c)))), path: None } via Some(EmptyRoute) in repo repo
  
  Caused by:
      alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c is missing
  Error: Execution failed

Check error as data fails if not in readonly-storage mode
  $ mononoke_walker --with-readonly-storage=false scrub --error-as-data-node-type AliasContentMapping -I deep -q -b master_bookmark 2>&1 | grep -vE "(Bytes|Walked)/s"
  [ERROR] Execution error: Error as data could mean internal state is invalid, run with --with-readonly-storage=true to ensure no risk of persisting it
  Error: Execution failed

Check counts with error-as-data-node-type
  $ mononoke_walker --scuba-log-file scuba.json scrub -q --error-as-data-node-type AliasContentMapping -I deep -b master_bookmark 2>&1 | grep -vE "(Bytes|Raw|Walked)/s" | sed -re 's/(Could not step to).*/\1/' | uniq -c | sed 's/^ *//'
  1 [WARN] Error as data enabled, walk results may not be complete. Errors as data enabled for node types [AliasContentMapping] edge types []
  1 [INFO] Walking edge types * (glob)
  1 [INFO] Walking node types * (glob)
  1 [WARN] [walker scrub{repo=repo}] Could not step to
  1 [INFO] [walker scrub{repo=repo}] Seen,Loaded: 43,42

Check scuba data
  $ wc -l < scuba.json
  1
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .edge_type, .node_key, .node_type, .repo, .walk_type ] | @csv' < scuba.json | sort
  1,"missing","FileContentMetadataV2ToGitSha1Alias","alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c","AliasContentMapping","repo","scrub"

Check error-as-data-edge-type, should get an error on FileContentMetadataV2ToGitSha1Alias as have not converted its errors to data
  $ mononoke_walker scrub -q --error-as-data-node-type AliasContentMapping --error-as-data-edge-type FileContentMetadataV2ToSha1Alias -I deep -b master_bookmark 2>&1 | grep -vE "(Bytes|Walked)/s"
  Could not step to OutgoingEdge { label: FileContentMetadata*ToSha1Alias, target: AliasContentMapping(AliasKey(Sha1(Sha1(32096c2e0eff33d844ee6d675407ace18289357d)))), path: None }, due to Other(*), via Some(EmptyRoute), repo: repo (glob) (?)
  [WARN] Error as data enabled, walk results may not be complete. Errors as data enabled for node types [AliasContentMapping] edge types [FileContentMetadataV2ToSha1Alias]
  [INFO] Walking edge types * (glob)
  [INFO] Walking node types * (glob)
  [ERROR] Could not step to OutgoingEdge { label: FileContentMetadata*ToSha1Alias, target: AliasContentMapping(AliasKey(Sha1(Sha1(32096c2e0eff33d844ee6d675407ace18289357d)))), path: None }, due to Other(*), via Some(EmptyRoute) (?)
  [ERROR] Execution error: Could not step to OutgoingEdge { label: FileContentMetadataV2ToGitSha1Alias, target: AliasContentMapping(AliasKey(GitSha1(GitSha1(96d80cd6c4e7158dbebd0849f4fb7ce513e5828c)))), path: None } via Some(EmptyRoute) in repo repo
  
  Caused by:
      alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c is missing
  Error: Execution failed

Check error-as-data-edge-type, should get no errors as FileContentMetadataV2ToGitSha1Alias has its errors converted to data
  $ mononoke_walker scrub -q --error-as-data-node-type AliasContentMapping --error-as-data-edge-type FileContentMetadataV2ToGitSha1Alias -I deep -b master_bookmark 2>&1 | grep -vE "(Bytes|Raw|Walked)/s" | sed -re 's/(Could not step to).*/\1/' | uniq -c | sed 's/^ *//'
  1 [WARN] Error as data enabled, walk results may not be complete. Errors as data enabled for node types [AliasContentMapping] edge types [FileContentMetadataV2ToGitSha1Alias]
  1 [INFO] Walking edge types * (glob)
  1 [INFO] Walking node types * (glob)
  1 [WARN] [walker scrub{repo=repo}] Could not step to
  1 [INFO] [walker scrub{repo=repo}] Seen,Loaded: 43,42
