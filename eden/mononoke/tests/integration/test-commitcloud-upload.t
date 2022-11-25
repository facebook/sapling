# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration
  $ export READ_ONLY_REPO=1
  $ INFINITEPUSH_ALLOW_WRITES=true \
  >   setup_common_config
  $ cd $TESTTMP

  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "mutation_advertise_for_infinitepush": true,
  >     "mutation_accept_for_infinitepush": true,
  >     "mutation_generate_for_draft": true
  >   }
  > }
  > EOF

setup common configuration for these tests
mononoke  local commit cloud backend
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend =
  > commitcloud =
  > infinitepush =
  > rebase =
  > remotenames =
  > share =
  > [commitcloud]
  > hostname = testhost
  > servicetype = local
  > servicelocation = $TESTTMP
  > owner_team = The Test Team
  > usehttpupload = True
  > [visibility]
  > enabled = True
  > [mutation]
  > record = True
  > enabled = True
  > date = 0 0
  > [remotefilelog]
  > reponame=repo
  > EOF

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ mkcommit "base_commit"
  $ hg log -T '{short(node)}\n'
  8b2dca0c8a72

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup client1 and client2
  $ hgclone_treemanifest ssh://user@dummy/repo client1 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo client2 --noupdate

blobimport

  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server

  $ cd client1
  $ hgedenapi up master_bookmark -q
  $ hgedenapi cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'repo' repo
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ../client2
  $ hgedenapi up master_bookmark -q
  $ hgedenapi cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'repo' repo
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)


TEST CASES:

Make a commit in the first client and upload it
This test also checks file content deduplication. We upload 1 file content and 100 filenodes here.
  $ cd ../client1
  $ mkdir dir1 dir2

  $ for i in {0..99} ; do touch dir1/$i ; done
  $ hgedenapi addremove -q

  $ hgedenapi commit -m "New files Dir1"

  $ hgedenapi cloud check -r 536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9
  536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9 not uploaded

  $ hgedenapi cloud check -r 536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9 --config commitcloud.usehttpupload=False
  536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9 not backed up

  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud upload
   INFO edenapi::client: Requesting capabilities for repo repo
   INFO edenapi::client: Requesting lookup for 1 item(s)
  commitcloud: head '536d3fb3929e' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
   INFO edenapi::client: Requesting lookup for 3 item(s)
  edenapi: queue 100 files for upload
   INFO edenapi::client: Requesting lookup for 1 item(s)
   INFO edenapi::client: Received 0 token(s) from the lookup_batch request
   INFO edenapi::client: Requesting upload for */repo/upload/file/content_id/a6ef0ef0eb8935a67f26f91d4cd13c02d2f7e13c74325488d8b12fdda58b6a00?content_size=0 (glob)
   INFO edenapi::client: Received 1 new token(s) from upload requests
   INFO edenapi::client: Requesting hg filenodes upload for 100 item(s)
  edenapi: uploaded 100 files
  edenapi: queue 2 trees for upload
   INFO edenapi::client: Requesting trees upload for 2 item(s)
  edenapi: uploaded 2 trees
  edenapi: uploading commit '536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9'...
   INFO edenapi::client: Requesting changesets upload for 1 item(s)
  edenapi: uploaded 1 changeset

  $ hgedenapi cloud check -r 536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9
  536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9 uploaded

  $ hgedenapi cloud check -r 536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9 --config commitcloud.usehttpupload=False
  536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9 backed up


Make another commit in the first client and upload it
The files of the second commit are identical to the files of the first commit, so we don't expect any new content uploads
  $ hgedenapi prev -q
  [8b2dca] base_commit
  $ for i in {0..99} ; do touch dir2/$i ; done
  $ hgedenapi addremove -q
  $ hgedenapi commit -m "New files Dir2"

  $ hgedenapi cloud check -r 65289540f44d80cecffca8a3fd655c0ca6243cd9
  65289540f44d80cecffca8a3fd655c0ca6243cd9 not uploaded

  $ hgedenapi cloud check -r 65289540f44d80cecffca8a3fd655c0ca6243cd9 --config commitcloud.usehttpupload=False
  65289540f44d80cecffca8a3fd655c0ca6243cd9 not backed up

  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud upload
   INFO edenapi::client: Requesting capabilities for repo repo
   INFO edenapi::client: Requesting lookup for 1 item(s)
  commitcloud: head '65289540f44d' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
   INFO edenapi::client: Requesting lookup for 3 item(s)
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
   INFO edenapi::client: Requesting trees upload for 1 item(s)
  edenapi: uploaded 1 tree
  edenapi: uploading commit '65289540f44d80cecffca8a3fd655c0ca6243cd9'...
   INFO edenapi::client: Requesting changesets upload for 1 item(s)
  edenapi: uploaded 1 changeset

  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud upload
  commitcloud: nothing to upload

Expect remote lookup, edenapi based version of the `hg cloud check` command doesn't check the local cache yet.
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud check -r 65289540f44d80cecffca8a3fd655c0ca6243cd9
   INFO edenapi::client: Requesting lookup for 1 item(s)
  65289540f44d80cecffca8a3fd655c0ca6243cd9 uploaded

The legacy version performs remote lookup with the `--remote` option only
  $ hgedenapi cloud check -r 65289540f44d80cecffca8a3fd655c0ca6243cd9 --config commitcloud.usehttpupload=False --debug
  65289540f44d80cecffca8a3fd655c0ca6243cd9 backed up

  $ hgedenapi cloud check -r 65289540f44d80cecffca8a3fd655c0ca6243cd9 --config commitcloud.usehttpupload=False --debug --remote
  sending hello command
  sending clienttelemetry command
  sending knownnodes command
  65289540f44d80cecffca8a3fd655c0ca6243cd9 backed up

  $ cd ..

Try pull an uploaded commit from another client
  $ cd client2
  $ hgedenapi pull -r 65289540f44d80cecffca8a3fd655c0ca6243cd9
  pulling from mononoke://$LOCALIP:*/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes

  $ tglogm
  o  65289540f44d 'New files Dir2'
  │
  @  8b2dca0c8a72 'base_commit'
  
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud check -r 65289540f44d --config commitcloud.usehttpupload=False  # pull doesn't update backup state
  65289540f44d80cecffca8a3fd655c0ca6243cd9 not backed up

  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud upload
   INFO edenapi::client: Requesting capabilities for repo repo
   INFO edenapi::client: Requesting lookup for 1 item(s)
  commitcloud: nothing to upload

  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud upload # upload does, no remote calls for the second call
  commitcloud: nothing to upload

  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud check -r 65289540f44d  --config commitcloud.usehttpupload=False --debug # upload does, no remote calls
  65289540f44d80cecffca8a3fd655c0ca6243cd9 backed up

  $ cd ..

Rebase a commit and pull it again in the client2. Check for correct mutation markers.
Also, check that upload will not reupload file contents again.

  $ cd client1
  $ hgedenapi rebase -s 65289540f44d80cecffca8a3fd655c0ca6243cd9 -d 536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc
  rebasing 65289540f44d "New files Dir2"
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: head 'a8c7c28d0391' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploading commit 'a8c7c28d0391c5948f0a40f43e8b16d7172289cf'...
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

  $ cd client2
  $ hgedenapi pull -r a8c7c28d0391c5948f0a40f43e8b16d7172289cf
  pulling from mononoke://$LOCALIP:*/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes

  $ tglogm --hidden
  o  a8c7c28d0391 'New files Dir2'
  │
  o  536d3fb3929e 'New files Dir1'
  │
  │ x  65289540f44d 'New files Dir2'  (Rewritten using rebase into a8c7c28d0391)
  ├─╯
  @  8b2dca0c8a72 'base_commit'
  

Try `cloud sync` now. Expected that nothing new is either uploaded or pulled.
Remote lookup is expected because `hg pull` command doesn't update backup state.
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
   INFO edenapi::client: Requesting capabilities for repo repo
   INFO edenapi::client: Requesting lookup for 1 item(s)
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)


Check that the second run doesn't perform remote lookup because the previous command should update local backed up state.
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

Try moving a directory and uploaded a resulting commit.
Expected that the 'lookup' returns tokens for file contents and it won't be reuploaded again.
Also, dedup for file contents is expected to work (see queue 100 files but only 1 lookup).
  $ hgedenapi checkout a8c7c28d0391 -q
  $ hgedenapi mv dir2 dir3 -q
  $ hgedenapi commit -m "New files Dir3 moved from Dir2" -q
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
   INFO edenapi::client: Requesting capabilities for repo repo
   INFO edenapi::client: Requesting lookup for 1 item(s)
  commitcloud: head '32551ca74417' hasn't been uploaded yet
   INFO edenapi::client: Requesting lookup for 3 item(s)
  edenapi: queue 1 commit for upload
   INFO edenapi::client: Requesting lookup for 102 item(s)
  edenapi: queue 100 files for upload
   INFO edenapi::client: Requesting lookup for 1 item(s)
   INFO edenapi::client: Received 1 token(s) from the lookup_batch request
   INFO edenapi::client: Received 0 new token(s) from upload requests
   INFO edenapi::client: Requesting hg filenodes upload for 100 item(s)
  edenapi: uploaded 100 files
  edenapi: queue 2 trees for upload
   INFO edenapi::client: Requesting trees upload for 2 item(s)
  edenapi: uploaded 2 trees
  edenapi: uploading commit '32551ca744171ab6eedf48245d4fab816292ae5f'...
   INFO edenapi::client: Requesting changesets upload for 1 item(s)
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Back to client1 and sync.
  $ cd client1
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: nothing to upload
  pulling 32551ca74417 from mononoke://$LOCALIP:*/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglogm
  o  32551ca74417 'New files Dir3 moved from Dir2'
  │
  @  a8c7c28d0391 'New files Dir2'
  │
  │ x  65289540f44d 'New files Dir2'  (Rewritten using rebase into a8c7c28d0391)
  │ │
  o │  536d3fb3929e 'New files Dir1'
  ├─╯
  o  8b2dca0c8a72 'base_commit'
  
  $ cd ..

Check how upload behaves if only commit metadata has been changed.
No trees or filenodes are expected to be reuploaded.
  $ cd client2
  $ hgedenapi commit --amend -m "Edited: New files Dir3 moved from Dir2" -q
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: head 'c8b3ca487837' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploading commit 'c8b3ca4878376f03b729cc867113280dc38baf23'...
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Sync also client1 at the end for further tests...
  $ cd client1
  $ hgedenapi cloud sync -q

Check that Copy From information has been uploaded correctly.
The file dir3/0 has been moved from the file dir2/0 on the client2 previously.
So, this information is expected to be preserved on the client1.
  $ hgedenapi checkout c8b3ca487837 -q
  $ hgedenapi log -f dir3/0
  commit:      c8b3ca487837
  user:        test
  date:        * (glob)
  summary:     Edited: New files Dir3 moved from Dir2
  
  commit:      a8c7c28d0391
  user:        test
  date:        * (glob)
  summary:     New files Dir2
  


Check both ways to specify a commit to back up work - even though we're going through a compat alias
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud backup c8b3ca487837
  commitcloud: nothing to upload
 
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud backup -r c8b3ca487837
  commitcloud: nothing to upload

Check the force flag for backup. Local cache checks must be ignoree
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud backup -r c8b3ca487837 --force
   INFO edenapi::client: Requesting capabilities for repo repo
  commitcloud: head 'c8b3ca487837' hasn't been uploaded yet
  edenapi: queue 3 commits for upload
  edenapi: queue 300 files for upload
   INFO edenapi::client: Requesting lookup for 1 item(s)
   INFO edenapi::client: Received 1 token(s) from the lookup_batch request
   INFO edenapi::client: Received 0 new token(s) from upload requests
   INFO edenapi::client: Requesting hg filenodes upload for 300 item(s)
  edenapi: uploaded 300 files
  edenapi: queue 6 trees for upload
   INFO edenapi::client: Requesting trees upload for 6 item(s)
  edenapi: uploaded 6 trees
  edenapi: uploading commit '536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9'...
  edenapi: uploading commit 'a8c7c28d0391c5948f0a40f43e8b16d7172289cf'...
  edenapi: uploading commit 'c8b3ca4878376f03b729cc867113280dc38baf23'...
   INFO edenapi::client: Requesting changesets upload for 3 item(s)
  edenapi: uploaded 3 changesets

Remove the local cache, check that the sync operation will restore the cache and that remote checks will be performed
  $ rm -rf .hg/commitcloud/backedupheads*

  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
   INFO edenapi::client: Requesting lookup for 4 item(s)
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ ls .hg/commitcloud/backedupheads*
  .hg/commitcloud/backedupheads* (glob)

Remove the local cache, check that the upload operation will restore the cache and that remote checks will be performed
  $ rm -rf .hg/commitcloud/backedupheads*

  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud upload
   INFO edenapi::client: Requesting lookup for 4 item(s)
  commitcloud: nothing to upload

  $ ls .hg/commitcloud/backedupheads*
  .hg/commitcloud/backedupheads* (glob)


Check that `hg cloud sync` command can self recover from corrupted local backed up state
  $ echo "trash" > .hg/commitcloud/backedupheads*
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud sync --debug
  commitcloud: synchronizing 'repo' with 'user/test/default'
  unrecognised backedupheads version 'trash', ignoring
   INFO edenapi::client: Requesting lookup for 4 item(s)
  commitcloud: nothing to upload
  commitcloud local service: get_references for current version 5
  commitcloud: commits synchronized
  finished in * (glob)
