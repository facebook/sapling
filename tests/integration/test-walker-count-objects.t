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
  $ mononoke_walker --storage-id=blobstore --readonly-storage count-objects -q --bookmark master_bookmark -I bonsai 2>&1
  * Walking roots * (glob)
  * Walking edge types [BonsaiChangesetToBonsaiParent, BonsaiChangesetToFileContent, BookmarkToBonsaiChangeset, FileContentToFileContentMetadata] (glob)
  * Walking node types [BonsaiChangeset, Bookmark, FileContent, FileContentMetadata] (glob)
  * Final count: (10, 10) (glob)
  * Type:Walked,Checks,Children BonsaiChangeset:3,3,8 Bookmark:1,1,1 FileContent:3,3,0 FileContentMetadata:3,0,0  (glob)
  * Exiting... (glob)

count-objects, shallow, bonsai only.  No parents, expect just one of each node type
  $ mononoke_walker --storage-id=blobstore --readonly-storage count-objects -q --bookmark master_bookmark -I shallow -X hg -x BonsaiHgMapping 2>&1
  * Walking roots * (glob)
  * Walking edge types [BonsaiChangesetToFileContent, BookmarkToBonsaiChangeset, FileContentToFileContentMetadata] (glob)
  * Walking node types [BonsaiChangeset, Bookmark, FileContent, FileContentMetadata] (glob)
  * Final count: (4, 4) (glob)
  * Type:Walked,Checks,Children BonsaiChangeset:1,1,2 Bookmark:1,1,1 FileContent:1,1,0 FileContentMetadata:1,0,0  (glob)
  * Exiting... (glob)

count-objects, hg only. total nodes is HGCOUNT plus 1 for the root bookmark step, plus 1 for mapping from bookmark to hg. plus 3 for filenode (same blob as envelope)
  $ mononoke_walker --storage-id=blobstore --readonly-storage count-objects -q --bookmark master_bookmark -I hg 2>&1
  * Walking roots * (glob)
  * Walking edge types [BonsaiHgMappingToHgChangeset, BookmarkToBonsaiHgMapping, HgChangesetToHgManifest, HgChangesetToHgParent, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgLinkNodeToHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode] (glob)
  * Walking node types [BonsaiHgMapping, Bookmark, FileContent, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest] (glob)
  * Final count: (17, 17) (glob)
  * Type:Walked,Checks,Children BonsaiHgMapping:1,1,1 Bookmark:1,1,1 FileContent:3,3,0 HgChangeset:3,3,5 HgFileEnvelope:3,*,3 HgFileNode:3,*,0 HgManifest:3,3,6  (glob)
  * Exiting... (glob)

count-objects, default deep walk across bonsai and hg data.  BLOBCOUNT plus mappings and root.
  $ mononoke_walker --storage-id=blobstore --readonly-storage count-objects -q --bookmark master_bookmark -I deep 2>&1
  * Walking roots * (glob)
  * Walking edge types [BonsaiChangesetToBonsaiHgMapping, BonsaiChangesetToBonsaiParent, BonsaiChangesetToFileContent, BonsaiHgMappingToHgChangeset, BookmarkToBonsaiChangeset, FileContentToFileContentMetadata, HgBonsaiMappingToBonsaiChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgLinkNodeToHgBonsaiMapping, HgLinkNodeToHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode] (glob)
  * Walking node types [BonsaiChangeset, BonsaiHgMapping, Bookmark, FileContent, FileContentMetadata, HgBonsaiMapping, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest] (glob)
  * Final count: (28, 28) (glob)
  * Type:Walked,Checks,Children BonsaiChangeset:3,3,11 BonsaiHgMapping:3,* Bookmark:1,1,1 FileContent:3,3,0 FileContentMetadata:3,0,0 HgBonsaiMapping:3,3,0 HgChangeset:3,* HgFileEnvelope:3,*,0 HgFileNode:3,6,3 HgManifest:3,3,6  (glob)
  * Exiting... (glob)

count-objects, default shallow walk across bonsai and hg data
  $ mononoke_walker --storage-id=blobstore --readonly-storage count-objects -q --bookmark master_bookmark -I shallow 2>&1
  * Walking roots * (glob)
  * Walking edge types [BonsaiChangesetToBonsaiHgMapping, BonsaiChangesetToFileContent, BonsaiHgMappingToHgChangeset, BookmarkToBonsaiChangeset, FileContentToFileContentMetadata, HgChangesetToHgManifest, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode] (glob)
  * Walking node types [BonsaiChangeset, BonsaiHgMapping, Bookmark, FileContent, FileContentMetadata, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest] (glob)
  * Final count: (17, 17) (glob)
  * Type:Walked,Checks,Children BonsaiChangeset:1,1,3 BonsaiHgMapping:1,1,1 Bookmark:1,1,1 FileContent:3,4,0 FileContentMetadata:3,0,0 HgChangeset:1,1,1 HgFileEnvelope:3,*,4 HgFileNode:3,3,0 HgManifest:1,1,6  (glob)
  * Exiting... (glob)
