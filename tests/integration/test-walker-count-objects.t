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
  $ BONSAICOUNT=$(ls $BLOBPREFIX.changeset.* $BLOBPREFIX.content.* | wc -l)
  $ echo "$BONSAICOUNT"
  6
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | grep -v .alias. | wc -l)
  $ echo "$BLOBCOUNT"
  21

count-objects, bonsai core data
  $ PLUSNONBLOBSTORENODES=
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore count-objects -q --bookmark master_bookmark -t Bookmark -t BonsaiChangeset -t BonsaiParents -t FileContent 2>&1
  * Excluding types * (glob)
  * Walking roots * (glob)
  * Walking node types [BonsaiChangeset, BonsaiParents, Bookmark, FileContent]* (glob)
  * Final count: (10, 10) (glob)
  * Type:Walked,Checks,Children BonsaiChangeset:3,3,6 BonsaiParents:3,3,2 Bookmark:1,0,1 FileContent:3,3,0  (glob)
  * Exiting... (glob)

count-objects, default types, which is bonsai and hg
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore count-objects -q --bookmark master_bookmark -x HgFileEnvelope 2>&1
  * Excluding types * (glob)
  * Walking roots * (glob)
  * Walking node types [BonsaiChangeset, BonsaiChangesetFromHgChangeset, BonsaiParents, Bookmark, FileContent, FileContentMetadata, HgChangeset, HgChangesetFromBonsaiChangeset, HgFileNode, HgManifest] (glob)
  * Final count: (28, 28) (glob)
  * Type:Walked,Checks,Children BonsaiChangeset:3,3,12 BonsaiChangesetFromHgChangeset:3,3,0 BonsaiParents:3,3,2 Bookmark:1,0,1 FileContent:3,3,0 FileContentMetadata:3,0,0 HgChangeset:3,3,3 HgChangesetFromBonsaiChangeset:3,3,3 HgFileNode:3,6,3 HgManifest:3,3,3  (glob)
  * Exiting... (glob)
