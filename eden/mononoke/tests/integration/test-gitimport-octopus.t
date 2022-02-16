# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ENABLED_DERIVED_DATA='["git_trees", "filenodes", "hgchangesets"]' setup_common_config "blob_files"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"

# Setup git repsitory
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ git commit --allow-empty -m "root commit"
  [master (root-commit) d53a2ef] root commit
  $ git branch root

  $ echo "this is master" > master
  $ git add master
  $ git commit -qam "Add master"

  $ git checkout -q root
  $ git checkout -qb branch1
  $ echo "this is branch1" > branch1
  $ git add branch1
  $ git commit -qam "Add branch1"

  $ git checkout -q root
  $ git checkout -qb branch2
  $ echo "this is branch2" > branch2
  $ git add branch2
  $ git commit -qam "Add branch2"

  $ git checkout -q master
  $ git merge branch1 branch2
  Trying simple merge with branch1
  Trying simple merge with branch2
  Merge made by the 'octopus' strategy.
   branch1 | 1 +
   branch2 | 1 +
   2 files changed, 2 insertions(+)
   create mode 100644 branch1
   create mode 100644 branch2

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --derive-trees full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * GitRepo:*repo-git commit 1 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 2 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 3 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 4 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 5 of 5 - Oid:* => Bid:* (glob)
  * 5 tree(s) are valid! (glob)
  * Ref: Some("refs/heads/branch1"): Some(ChangesetId(Blake2(*))) (glob)
  * Ref: Some("refs/heads/branch2"): Some(ChangesetId(Blake2(*))) (glob)
  * Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(138f3627a8b764746a787d755cee5fb7134f631b63da40e97e515075f0a83dd1))) (glob)
  * Ref: Some("refs/heads/root"): Some(ChangesetId(Blake2(*))) (glob)

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master 138f3627a8b764746a787d755cee5fb7134f631b63da40e97e515075f0a83dd1
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: ChangesetId(Blake2(138f3627a8b764746a787d755cee5fb7134f631b63da40e97e515075f0a83dd1)) (glob)
  * Current position of BookmarkName { bookmark: "master" } is None (glob)

# Start Mononoke
  $ start_and_wait_for_mononoke_server
# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone mononoke://$(mononoke_address)/repo "$HG_REPO"
  $ cd "$HG_REPO"
  $ tail master branch1 branch2
  ==> master <==
  this is master
  * (glob)
  ==> branch1 <==
  this is branch1
  * (glob)
  ==> branch2 <==
  this is branch2

# Check the extras
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  stepparents=650b188a9428ac5492e597be0e14ff9df0ff56f8

# Now, check that various Mononoke verification binaries work properly on this commit
  $ hghash="$(hg log -r . -T '{node}')"
  $ RUST_BACKTRACE=1 bonsai_verify hg-manifest "$hghash" 1
  *Reloading redacted config from configerator* (glob)
  * 0ade4c953b0844653456906f5aaf8d3b1da94c37 total:1 bad:0 * (glob)

  $ bonsai_verify round-trip "$hghash"
  *Reloading redacted config from configerator* (glob)
  * 100.00% valid, summary: , total: *, valid: *, errors: 0, ignored: 0 (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT HEX(filenode), HEX(linknode) FROM filenodes ORDER BY filenode DESC;"
  DDAE7A95B6B0FB27DFACC4051C41AA9CFF30C1E2|389710B7D0BE8C40323D3D96F717219F1CCE9601
  DB9F6E90B4D31605949C7E5273E72FEADE04E6C4|650B188A9428AC5492E597BE0E14FF9DF0FF56F8
  D5E651FDE2FF4278E3172BF3BEDACCAE9F466C89|5D368F78915F9FDEBE4CB0801632B46D8B7CD74A
  B80DE5D138758541C5F05265AD144AB9FA86D1DB|3E41DBCA2B1CE9D31E73EAE592DAABC867C386FA
  B24D823C90409CA8CE2AC2BB22DAD5C6B9D17D4D|650B188A9428AC5492E597BE0E14FF9DF0FF56F8
  8D8AC2F4A8AF10BA885E164A5F33CB4F91F8A0F8|5D368F78915F9FDEBE4CB0801632B46D8B7CD74A
  290DD67AD15DE1444C88A016BE6EC55CDF056C10|389710B7D0BE8C40323D3D96F717219F1CCE9601
  1A4ECD744147A79966E5473A3B86B447533ABF9D|0ADE4C953B0844653456906F5AAF8D3B1DA94C37

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes; DELETE FROM fixedcopyinfo; DELETE FROM paths;"

  $ hg log -r 'all()' -T '{node}\n' > hashes
  $ regenerate_hg_filenodes --file 'hashes'
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * processed 5 (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT HEX(filenode), HEX(linknode) FROM filenodes ORDER BY filenode DESC;"
  DDAE7A95B6B0FB27DFACC4051C41AA9CFF30C1E2|389710B7D0BE8C40323D3D96F717219F1CCE9601
  DB9F6E90B4D31605949C7E5273E72FEADE04E6C4|650B188A9428AC5492E597BE0E14FF9DF0FF56F8
  D5E651FDE2FF4278E3172BF3BEDACCAE9F466C89|5D368F78915F9FDEBE4CB0801632B46D8B7CD74A
  B80DE5D138758541C5F05265AD144AB9FA86D1DB|3E41DBCA2B1CE9D31E73EAE592DAABC867C386FA
  B24D823C90409CA8CE2AC2BB22DAD5C6B9D17D4D|650B188A9428AC5492E597BE0E14FF9DF0FF56F8
  8D8AC2F4A8AF10BA885E164A5F33CB4F91F8A0F8|5D368F78915F9FDEBE4CB0801632B46D8B7CD74A
  290DD67AD15DE1444C88A016BE6EC55CDF056C10|389710B7D0BE8C40323D3D96F717219F1CCE9601
  1A4ECD744147A79966E5473A3B86B447533ABF9D|0ADE4C953B0844653456906F5AAF8D3B1DA94C37
