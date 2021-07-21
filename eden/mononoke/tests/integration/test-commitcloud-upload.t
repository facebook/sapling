# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration
  $ export READ_ONLY_REPO=1
  $ INFINITEPUSH_ALLOW_WRITES=true \
  >   ENABLE_PRESERVE_BUNDLE2=true \
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
  > token_enforced = False
  > owner_team = The Test Team
  > usehttpupload = True
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

  $ mononoke
  $ wait_for_mononoke

start edenapi
  $ setup_configerator_configs
  $ start_edenapi_server_no_tls

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
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud upload
    INFO edenapi::client: Requesting lookup for 1 item(s)
  commitcloud: head '536d3fb3929e' hasn't been uploaded yet
    INFO edenapi::client: Requesting lookup for 1 item(s)
  commitcloud: queue 1 commit for upload
    INFO edenapi::client: Requesting lookup for 100 item(s)
  commitcloud: queue 100 files for upload
    INFO edenapi::client: Requesting lookup for 1 item(s)
    INFO edenapi::client: Received 0 token(s) from the lookup_batch request
    INFO edenapi::client: Requesting upload for */repo/upload/file/content_id/a6ef0ef0eb8935a67f26f91d4cd13c02d2f7e13c74325488d8b12fdda58b6a00 (glob)
    INFO edenapi::client: Received 1 new token(s) from upload requests
    INFO edenapi::client: Requesting hg filenodes upload for 100 item(s)
  commitcloud: uploaded 100 files
    INFO edenapi::client: Requesting lookup for 2 item(s)
  commitcloud: queue 2 trees for upload
    INFO edenapi::client: Requesting trees upload for 2 item(s)
  commitcloud: uploaded 2 trees
  commitcloud: uploading commit '536d3fb3929eab4b01e63ab7fc9b25a5c8a08bc9'...
    INFO edenapi::client: Requesting changesets upload for 1 item(s)
  commitcloud: uploaded 1 changeset
 
 
Make another commit in the first client and upload it
The files of the second commit are identical to the files of the first commit, so we don't expect any new content uploads
  $ hgedenapi prev -q
  [8b2dca] base_commit
  $ for i in {0..99} ; do touch dir2/$i ; done
  $ hgedenapi addremove -q
  $ hgedenapi commit -m "New files Dir2"
  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud upload
    INFO edenapi::client: Requesting lookup for 2 item(s)
  commitcloud: head '65289540f44d' hasn't been uploaded yet
    INFO edenapi::client: Requesting lookup for 1 item(s)
  commitcloud: queue 1 commit for upload
    INFO edenapi::client: Requesting lookup for 100 item(s)
  commitcloud: queue 0 files for upload
    INFO edenapi::client: Requesting lookup for 2 item(s)
  commitcloud: queue 1 tree for upload
    INFO edenapi::client: Requesting trees upload for 1 item(s)
  commitcloud: uploaded 1 tree
  commitcloud: uploading commit '65289540f44d80cecffca8a3fd655c0ca6243cd9'...
    INFO edenapi::client: Requesting changesets upload for 1 item(s)
  commitcloud: uploaded 1 changeset

  $ EDENSCM_LOG="edenapi::client=info" hgedenapi cloud upload
    INFO edenapi::client: Requesting lookup for 2 item(s)
  commitcloud: nothing to upload
