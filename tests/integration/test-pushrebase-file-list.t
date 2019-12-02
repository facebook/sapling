  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export PUSHREBASE_REWRITE_DATES=1
  $ setup_common_config "blob:files"
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
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

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke

Clone the repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

Push a directory
  $ hg up -q 0
  $ mkdir dir
  $ echo 1 > dir/1
  $ echo 2 > dir/2
  $ echo 3 > dir/3
  $ hg -q addremove
  $ hg ci -m 'create dir'
  $ hgmn push -r . --to master_bookmark -q
  $ hgmn up master_bookmark -q

Now replace directory with a file and push it. Make sure file lists before push
and after push match
  $ hg rm dir
  removing dir/1
  removing dir/2
  removing dir/3
  $ echo dir > dir
  $ hg addremove -q
  $ hg ci -m 'replace directory with a file'

List of files before the push
  $ hg log -r . -T '{files}'
  dir dir/1 dir/2 dir/3 (no-eol)

  $ hgmn push -r . --to master_bookmark -q
  $ hgmn up master_bookmark -q

List of files after the push.
  $ hg log -r . -T '{files}'
  dir dir/1 dir/2 dir/3 (no-eol)
