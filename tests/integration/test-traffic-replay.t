  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > EOF

Setup helpers
  $ log() {
  >   hg log -G -T "{desc} [{phase};rev={rev};{node|short}] {remotenames}" "$@"
  > }

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

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

Clone the repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ hgmn pull -q
  devel-warn: applied empty changegroup at: * (glob)

We are going to put these args inside traffic replay request below.
For that we need to escape all double quotes. `sed` below does exactly that
  $ ARGS=$(sed 's/"/\\"/g' < "$TESTTMP/traffic-replay-blobstore/blobs/"*)
  $ cat >> traffic_replay_request <<EOF
  > {"int":{"duration":0}, "normal":{"command":"getbundle","args":"$ARGS", "reponame":"$REPONAME"}}
  > EOF
  $ ls -l $TESTTMP/traffic-replay-blobstore/blobs/ | grep blob | wc -l
  1
  $ traffic_replay traffic_replay_request &> /dev/null

Make sure one more was added to the ephemeral blobstore
  $ ls -l $TESTTMP/traffic-replay-blobstore/blobs/ | grep blob | wc -l
  2
