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
  >  return ctx.file.len() <= 10
  > end
  > CONFIG
  $ register_hook common/hooks/file_size_hook.lua PerAddedOrModifiedFile "bypass_commit_string=\"@allow_large_files\""

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
  $ register_hook common/hooks/no_owners_file_deletes.lua PerChangeset "bypass_commit_string=\"@allow_delete_owners\""

  $ cat > common/hooks/no_owners2_file_deletes_pushvars.lua <<CONFIG
  > hook = function (ctx)
  >   for _, f in ipairs(ctx.files) do
  >     if f.is_deleted() and string.match(f.path, ".*OWNERS2$") then
  >       return false, "Deletion of OWNERS files is not allowed"
  >     end
  >   end
  >   return true
  > end
  > CONFIG
  $ register_hook common/hooks/no_owners2_file_deletes_pushvars.lua PerChangeset "bypass_pushvar=\"ALLOW_DELETE_OWNERS=true\""

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

Bypass owners check
  $ cat >> .hg/hgrc <<CONFIG
  > [extensions]
  > fbamend=
  > CONFIG
  $ hg amend -m 'remove OWNERS\n@allow_delete_owners'
  saved backup bundle to $TESTTMP/repo2/.hg/strip-backup/2d1a0bcf73ee-65b66be0-amend.hg (glob)
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 67730b0d6122 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

Add OWNERS2 file. This time bypass it with pushvars
  $ touch OWNERS2 && hg ci -Aqm 'add OWNERS2'
  $ hgmn push -r . --to master_bookmark -q
  server ignored bookmark master_bookmark update
  $ hg rm OWNERS2
  $ hg ci -m 'remove OWNERS2'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 55334cb4e1e4 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hookrunner failed Failures(([(ChangesetHookExecutionID { cs_id: HgChangesetId(HgNodeHash(Sha1(55334cb4e1e487f6de665629326eb1aaddccde53))), hook_name: "no_owners2_file_deletes_pushvars" }, Rejected(HookRejectionInfo { description: "Deletion of OWNERS files is not allowed", long_description: "" }))], [])), root_cause: ErrorMessage { (glob)
  remote:     msg: "hookrunner failed Failures(([(ChangesetHookExecutionID { cs_id: HgChangesetId(HgNodeHash(Sha1(55334cb4e1e487f6de665629326eb1aaddccde53))), hook_name: \"no_owners2_file_deletes_pushvars\" }, Rejected(HookRejectionInfo { description: \"Deletion of OWNERS files is not allowed\", long_description: \"\" }))], []))"
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
  $ hgmn push -r . --to master_bookmark --pushvars "ALLOW_DELETE_OWNERS=true"
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 55334cb4e1e4 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

Send large file
  $ hg up -q 0
  $ echo 'aaaaaaaaaaa' > largefile
  $ hg ci -Aqm 'largefile'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 3e0db158edcc to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hookrunner failed Failures(([], [(FileHookExecutionID { cs_id: HgChangesetId(HgNodeHash(Sha1(3e0db158edcc82d93b971f44c13ac74836db5714))), hook_name: "file_size_hook", file: HookFile path: largefile, changeset_id: 3e0db158edcc82d93b971f44c13ac74836db5714 }, Rejected(HookRejectionInfo { description: "", long_description: "" }))])), root_cause: ErrorMessage { (glob)
  remote:     msg: "hookrunner failed Failures(([], [(FileHookExecutionID { cs_id: HgChangesetId(HgNodeHash(Sha1(3e0db158edcc82d93b971f44c13ac74836db5714))), hook_name: \"file_size_hook\", file: HookFile path: largefile, changeset_id: 3e0db158edcc82d93b971f44c13ac74836db5714 }, Rejected(HookRejectionInfo { description: \"\", long_description: \"\" }))]))"
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Bypass large file hook
  $ hg amend -m '@allow_large_files'
  saved backup bundle to $TESTTMP/repo2/.hg/strip-backup/3e0db158edcc-6025a9b3-amend.hg (glob)
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 51fea0e7527d to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update
