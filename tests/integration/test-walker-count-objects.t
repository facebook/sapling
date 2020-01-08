  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ REPOTYPE="blob_files"
  $ setup_common_config "$REPOTYPE"
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark
  $ hg bookmark master_bookmark -r tip

blobimport, succeeding
  $ cd ..
  $ blobimport repo-hg/.hg repo

check blobstore numbers, walk will do some more steps for mappings
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ BONSAICOUNT=$(ls $BLOBPREFIX.changeset.* $BLOBPREFIX.content.* $BLOBPREFIX.content_metadata.* | wc -l)
  $ echo "$BONSAICOUNT"
  9
  $ HGCOUNT=$(ls $BLOBPREFIX.* | grep -E '.(filenode_lookup|hgchangeset|hgfilenode|hgmanifest).' | wc -l)
  $ echo "$HGCOUNT"
  12
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | grep -v .alias. | wc -l)
  $ echo "$BLOBCOUNT"
  21

count-objects, bonsai core data.  total nodes is BONSAICOUNT plus one for the root bookmark step.
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I bonsai 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BonsaiChangesetToBonsaiParent, BonsaiChangesetToFileContent, BookmarkToBonsaiChangeset]
  Walking node types [BonsaiChangeset, Bookmark, FileContent]
  Final count: (7, 7)
  * Type:Walked,Checks,Children BonsaiChangeset:3,* Bookmark:1,1,1 FileContent:3,3,0 (glob)
  Execution succeeded

count-objects, shallow, bonsai only.  No parents, expect just one of each node type
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I shallow -X hg -x BonsaiHgMapping 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiChangesetToFileContent, BookmarkToBonsaiChangeset, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata]
  Walking node types [AliasContentMapping, BonsaiChangeset, Bookmark, FileContent, FileContentMetadata]
  Final count: (7, 7)
  * Type:Walked,Checks,Children AliasContentMapping:3,3,0 BonsaiChangeset:1,1,2 Bookmark:1,1,1 FileContent:1,1,0 FileContentMetadata:1,0,3 (glob)
  Execution succeeded

count-objects, hg only. total nodes is HGCOUNT plus 1 for the root bookmark step, plus 1 for mapping from bookmark to hg. plus 3 for filenode (same blob as envelope)
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I hg 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BonsaiHgMappingToHgChangeset, BookmarkToBonsaiHgMapping, HgChangesetToHgManifest, HgChangesetToHgParent, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgLinkNodeToHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [BonsaiHgMapping, Bookmark, FileContent, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest]
  Final count: (17, 17)
  * Type:Walked,Checks,Children BonsaiHgMapping:1,1,1 Bookmark:1,1,1 FileContent:3,3,0 HgChangeset:3,*,5 HgFileEnvelope:3,*,3 HgFileNode:3,*,0 HgManifest:3,3,6 (glob)
  Execution succeeded

count-objects, default deep walk across bonsai and hg data.  BLOBCOUNT plus mappings and root.
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I deep 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiChangesetToBonsaiHgMapping, BonsaiChangesetToBonsaiParent, BonsaiChangesetToFileContent, BonsaiHgMappingToHgChangeset, BookmarkToBonsaiChangeset, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, HgBonsaiMappingToBonsaiChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgLinkNodeToHgBonsaiMapping, HgLinkNodeToHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiChangeset, BonsaiHgMapping, Bookmark, FileContent, FileContentMetadata, HgBonsaiMapping, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest]
  Final count: (37, 37)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiChangeset:3,3,11 BonsaiHgMapping:3,* Bookmark:1,1,1 FileContent:3,*,0 FileContentMetadata:3,0,9 HgBonsaiMapping:3,3,0 HgChangeset:3,* HgFileEnvelope:3,*,0 HgFileNode:3,6,3 HgManifest:3,3,6 (glob)
  Execution succeeded

count-objects, default shallow walk across bonsai and hg data
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I shallow 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiChangesetToBonsaiHgMapping, BonsaiChangesetToFileContent, BonsaiHgMappingToHgChangeset, BookmarkToBonsaiChangeset, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, HgChangesetToHgManifest, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiChangeset, BonsaiHgMapping, Bookmark, FileContent, FileContentMetadata, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest]
  Final count: (26, 26)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiChangeset:1,1,3 BonsaiHgMapping:1,1,1 Bookmark:1,1,1 FileContent:3,*,0 FileContentMetadata:3,0,9 HgChangeset:1,1,1 HgFileEnvelope:3,*,4 HgFileNode:3,3,0 HgManifest:1,1,6 (glob)
  Execution succeeded

count-objects, default shallow walk across bonsai and hg data, including mutable
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I shallow -I marker 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiChangesetToBonsaiHgMapping, BonsaiChangesetToBonsaiPhaseMapping, BonsaiChangesetToFileContent, BonsaiHgMappingToHgChangeset, BookmarkToBonsaiChangeset, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, HgChangesetToHgManifest, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiChangeset, BonsaiHgMapping, BonsaiPhaseMapping, Bookmark, FileContent, FileContentMetadata, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest]
  Final count: (27, 27)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiChangeset:1,1,4 BonsaiHgMapping:1,1,1 BonsaiPhaseMapping:1,1,0 Bookmark:1,1,1 FileContent:3,*,0 FileContentMetadata:3,0,9 HgChangeset:1,1,1 HgFileEnvelope:3,*,4 HgFileNode:3,3,0 HgManifest:1,1,6 (glob)
  Execution succeeded
