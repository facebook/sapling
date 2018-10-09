  $ . $TESTDIR/library.sh

setup configuration
  $ setup_hg_config_repo
  $ cd "$TESTTMP/mononoke-config"

  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="master_bookmark"
  > CONFIG

  $ mkdir -p common/hooks
  $ cat > common/hooks/file_size_hook.lua <<CONFIG
  > hook = function (ctx)
  >  return ctx.file.len() <= 100000
  > end
  > CONFIG
  $ register_hook common/hooks/file_size_hook.lua PerAddedOrModifiedFile

  $ cat > common/hooks/no_owners_file_deletes.lua <<CONFIG
  > hook = function (ctx)
  >   for _, f in ipairs(ctx.files) do
  >     if f.is_deleted() and string.match(f.path, ".*OWNERS$") then
  >       return false, "Deletion of OWNERS files is not allowed"
  >     end
  >   end
  >   return true
  > end
  > CONFIG
  $ register_hook common/hooks/no_owners_file_deletes.lua PerChangeset

  $ commit_and_blobimport_config_repo
  $ setup_common_hg_configs
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

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport rocksdb repo-hg/.hg repo

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

  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hgmn push -r . --to master_bookmark -q
  server ignored bookmark master_bookmark update

Delete a file, make sure that file_size_hook is not called on deleted files
  $ hgmn up -q tip
  $ hg rm 1
  $ hg ci -m 'delete a file'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 8ecfb5e6aa64 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

Add OWNERS file, then delete it. Make sure deletion is not allowed
  $ touch OWNERS && hg add OWNERS && hg ci -m 'add OWNERS'
  $ hgmn push -r . --to master_bookmark -q
  server ignored bookmark master_bookmark update
  $ hg rm OWNERS
  $ hg ci -m 'remove OWNERS'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 2d1a0bcf73ee to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hookrunner failed Failures(([(ChangesetHookExecutionID { cs_id: HgChangesetId(HgNodeHash(Sha1(2d1a0bcf73ee48cde9073fd52b6bbb71e4459c9b))), hook_name: "no_owners_file_deletes" }, Rejected(HookRejectionInfo { description: "Deletion of OWNERS files is not allowed", long_description: "" }))], [])), root_cause: ErrorMessage { (glob)
  remote:     msg: "hookrunner failed Failures(([(ChangesetHookExecutionID { cs_id: HgChangesetId(HgNodeHash(Sha1(2d1a0bcf73ee48cde9073fd52b6bbb71e4459c9b))), hook_name: \"no_owners_file_deletes\" }, Rejected(HookRejectionInfo { description: \"Deletion of OWNERS files is not allowed\", long_description: \"\" }))], []))"
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
