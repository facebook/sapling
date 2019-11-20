  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

# setup config repo

  $ REPOTYPE="blob:files"
  $ setup_common_config $REPOTYPE
  $ cd $TESTTMP

# 1. Setup nolfs hg repo, create several commit to it
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

# Commit small file
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"
  $ hg bookmark master_bookmark -r tip
  $ cd ..

  $ blobimport repo-hg/.hg repo

# 2. Setup Mononoke.
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

# 3. Clone hg server repo to hg client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-hg-client --noupdate --config extensions.remotenames=
  $ cd repo-hg-client
  $ setup_hg_client

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ hgmn pull -q
  devel-warn: applied empty changegroup at* (glob)
  $ hgmn update -r master_bookmark -q

# 4. Make a commit with corrupted file node, Change file node text
  $ echo "hello_world" > file
  $ hg commit -Aqm "commit"

# remotefilelog is True, so reference to filenodes are by hashes (SHA1)
  $ PACK_TO_CORRUPT=".hg/store/packs/dee3d9750ad87ede865d69e20330c34e51ec83d5.datapack"
# change access to file, as it is readonly
  $ chmod 666 "$PACK_TO_CORRUPT"
  $ sed -i s/hello_world/aaaaaaaaaaa/ "$PACK_TO_CORRUPT"

Do a push, but disable cache verification on the client side, otherwise
filenode won't be send at all
  $ hgmn push -r . --to master_bookmark -v --config remotefilelog.validatecachehashes=False
  pushing rev cb67355f2348 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       182 (changelog)
       140  file
  remote: Command failed
  remote:   Error:
  remote:     Error while uploading data for changesets, hashes: [HgChangesetId(HgNodeHash(Sha1(cb67355f234869bb9bf94787d5a69e21e23a8c9b)))]
  remote:   Root cause:
  remote:     InconsistentEntryHash(
  remote:         FilePath(
  remote:             MPath("file"),
  remote:         ),
  remote:         HgNodeHash(
  remote:             Sha1(979d39e9dea4d1f3f1fea701fd4d3bae43eef76b),
  remote:         ),
  remote:         HgNodeHash(
  remote:             Sha1(d159b93d975921924ad128d6a46ef8b1b8f28ba5),
  remote:         ),
  remote:     )
  remote:   Caused by:
  remote:     While creating Changeset Some(HgNodeHash(Sha1(cb67355f234869bb9bf94787d5a69e21e23a8c9b))), uuid: * (glob)
  remote:   Caused by:
  remote:     While processing entries
  remote:   Caused by:
  remote:     While uploading child entries
  remote:   Caused by:
  remote:     While walking dependencies of Root Manifest with id HgManifestId(HgNodeHash(Sha1(314550e1ace48fe6245515c137b38ea8aeb04c7d)))
  remote:   Caused by:
  remote:     Inconsistent node hash for entry: path file 'file', provided: 979d39e9dea4d1f3f1fea701fd4d3bae43eef76b, computed: d159b93d975921924ad128d6a46ef8b1b8f28ba5
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
