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
  $ gitimport "$GIT_REPO" full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 2 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 3 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 4 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 5 of 5 - Oid:* => Bid:* (glob)
  * Ref: "refs/heads/branch1": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/heads/branch2": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(79c017aa5cc494f127d5d12c6362c79dd802153d1b776b540328b551b8d22d63))) (glob)
  * Ref: "refs/heads/root": Some(ChangesetId(Blake2(*))) (glob)

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master 79c017aa5cc494f127d5d12c6362c79dd802153d1b776b540328b551b8d22d63
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(79c017aa5cc494f127d5d12c6362c79dd802153d1b776b540328b551b8d22d63)) (glob)
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
  convert_revision=6283891fdea5a1a4560451f09366220a585e07b2
  stepparents=7b25971f8ceef2c9754602cacdc8e7bdf96a313e

# Now, check that various Mononoke verification binaries work properly on this commit
  $ hghash="$(hg log -r . -T '{node}')"
  $ RUST_BACKTRACE=1 bonsai_verify hg-manifest "$hghash" 1
  * e9ee3b8706741d6da3c0b92516a1b62da0f99c1e total:1 bad:0 * (glob)

  $ bonsai_verify round-trip "$hghash"
  * 100.00% valid, summary: , total: *, valid: *, errors: 0, ignored: 0 (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT HEX(filenode), HEX(linknode) FROM filenodes ORDER BY filenode DESC;"
  DDAE7A95B6B0FB27DFACC4051C41AA9CFF30C1E2|6C8FBD0084A89D90E50B94EC1CDCC291D958AD52
  DB9F6E90B4D31605949C7E5273E72FEADE04E6C4|7B25971F8CEEF2C9754602CACDC8E7BDF96A313E
  D5E651FDE2FF4278E3172BF3BEDACCAE9F466C89|112A678D16E82350F73EE53901AC99147A931B51
  B80DE5D138758541C5F05265AD144AB9FA86D1DB|F467A740A49E20A2CA15F1B55283EE2C1417E5FC
  B24D823C90409CA8CE2AC2BB22DAD5C6B9D17D4D|7B25971F8CEEF2C9754602CACDC8E7BDF96A313E
  8D8AC2F4A8AF10BA885E164A5F33CB4F91F8A0F8|112A678D16E82350F73EE53901AC99147A931B51
  290DD67AD15DE1444C88A016BE6EC55CDF056C10|6C8FBD0084A89D90E50B94EC1CDCC291D958AD52
  1A4ECD744147A79966E5473A3B86B447533ABF9D|E9EE3B8706741D6DA3C0B92516A1B62DA0F99C1E

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes; DELETE FROM fixedcopyinfo; DELETE FROM paths;"

  $ hg log -r 'all()' -T '{node}\n' > hashes
  $ regenerate_hg_filenodes --file 'hashes'
  * using repo "repo" repoid RepositoryId(0) (glob)
  * processed 5 (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT HEX(filenode), HEX(linknode) FROM filenodes ORDER BY filenode DESC;"
  DDAE7A95B6B0FB27DFACC4051C41AA9CFF30C1E2|6C8FBD0084A89D90E50B94EC1CDCC291D958AD52
  DB9F6E90B4D31605949C7E5273E72FEADE04E6C4|7B25971F8CEEF2C9754602CACDC8E7BDF96A313E
  D5E651FDE2FF4278E3172BF3BEDACCAE9F466C89|112A678D16E82350F73EE53901AC99147A931B51
  B80DE5D138758541C5F05265AD144AB9FA86D1DB|F467A740A49E20A2CA15F1B55283EE2C1417E5FC
  B24D823C90409CA8CE2AC2BB22DAD5C6B9D17D4D|7B25971F8CEEF2C9754602CACDC8E7BDF96A313E
  8D8AC2F4A8AF10BA885E164A5F33CB4F91F8A0F8|112A678D16E82350F73EE53901AC99147A931B51
  290DD67AD15DE1444C88A016BE6EC55CDF056C10|6C8FBD0084A89D90E50B94EC1CDCC291D958AD52
  1A4ECD744147A79966E5473A3B86B447533ABF9D|E9EE3B8706741D6DA3C0B92516A1B62DA0F99C1E
