  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config "blob_files"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"

# Setup git repsitory
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init
  Initialized empty Git repository in $TESTTMP/repo-git/.git/
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
  $ gitimport "$GIT_REPO" --derive-trees
  * using repo "repo" repoid RepositoryId(0) (glob)
  Created d53a2ef2bbadbe26f8c28598b408e03c0b01027c => ChangesetId(Blake2(6527b56ce8ba165584a33318aa71b7442750e39466bf2691160a6158825f2193))
  Created 161a8cb720352af550786d4e73eeb36d5b958ddd => ChangesetId(Blake2(e0bd5a3d151b8b0cbcc0b07f131275a89237643ea14662b9a18a789619674ae5))
  Created bf946c828dea5fe0a0228dc7d556aa4a524df2d1 => ChangesetId(Blake2(dd8e619f36b5c91a908e9eade9c8407ca3bd06f637cef84282f04921675e9ebd))
  Created 933c6d8556a071c2105b8b2fd1dabff709d87929 => ChangesetId(Blake2(8d189eed3e25c42d197f50d11b92831cc43cd32002e9c5f9ab2c1b1a1af61c94))
  Created 6283891fdea5a1a4560451f09366220a585e07b2 => ChangesetId(Blake2(2c1b9f21f25524196376709ca6b4850e17170cbee48714802601e4706cfc1f28))
  Ref: Some("refs/heads/branch1"): Some(ChangesetId(Blake2(dd8e619f36b5c91a908e9eade9c8407ca3bd06f637cef84282f04921675e9ebd)))
  Ref: Some("refs/heads/branch2"): Some(ChangesetId(Blake2(8d189eed3e25c42d197f50d11b92831cc43cd32002e9c5f9ab2c1b1a1af61c94)))
  Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(2c1b9f21f25524196376709ca6b4850e17170cbee48714802601e4706cfc1f28)))
  Ref: Some("refs/heads/root"): Some(ChangesetId(Blake2(6527b56ce8ba165584a33318aa71b7442750e39466bf2691160a6158825f2193)))
  5 tree(s) are valid!

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master 2c1b9f21f25524196376709ca6b4850e17170cbee48714802601e4706cfc1f28
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(2c1b9f21f25524196376709ca6b4850e17170cbee48714802601e4706cfc1f28)) (glob)

# Start Mononoke
  $ mononoke
  $ wait_for_mononoke

# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone 'ssh://user@dummy/repo' "$HG_REPO"
  $ cd "$HG_REPO"
  $ tail master branch1 branch2
  ==> master <==
  this is master
  
  ==> branch1 <==
  this is branch1
  
  ==> branch2 <==
  this is branch2

# Check the extras
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  stepparents=0f6d8ee2c499636aa76108fb788093f12294ea87

# Now, check that various Mononoke verification binaries work properly on this commit
  $ hghash="$(hg log -r . -T '{node}')"
  $ RUST_BACKTRACE=1 bonsai_verify hg-manifest "$hghash" 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * d099b4dbaac9d3ca98d6c98090029ee79dff9e98 total:1 bad:0 * (glob)

  $ bonsai_verify round-trip "$hghash"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * 100.00% valid, summary: , total: 5, valid: 5, errors: 0, ignored: 0 (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT HEX(filenode), HEX(linknode) FROM filenodes ORDER BY filenode DESC;"
  DDAE7A95B6B0FB27DFACC4051C41AA9CFF30C1E2|C5FD128DF314BDAF4217A731D1FEA3FD190B72D3
  DB9F6E90B4D31605949C7E5273E72FEADE04E6C4|0F6D8EE2C499636AA76108FB788093F12294EA87
  D5E651FDE2FF4278E3172BF3BEDACCAE9F466C89|7D590605FFCC46B04347C33CD85AB72A32A0EE62
  B80DE5D138758541C5F05265AD144AB9FA86D1DB|E90D3F198A5C22785DC5227854F46272524B6195
  B24D823C90409CA8CE2AC2BB22DAD5C6B9D17D4D|0F6D8EE2C499636AA76108FB788093F12294EA87
  8D8AC2F4A8AF10BA885E164A5F33CB4F91F8A0F8|7D590605FFCC46B04347C33CD85AB72A32A0EE62
  290DD67AD15DE1444C88A016BE6EC55CDF056C10|C5FD128DF314BDAF4217A731D1FEA3FD190B72D3
  1A4ECD744147A79966E5473A3B86B447533ABF9D|D099B4DBAAC9D3CA98D6C98090029EE79DFF9E98

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes; DELETE FROM fixedcopyinfo; DELETE FROM paths;"

  $ hg log -r 'all()' -T '{node}\n' > hashes
  $ regenerate_hg_filenodes --file 'hashes'
  * using repo "repo" repoid RepositoryId(0) (glob)
  processed 100

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT HEX(filenode), HEX(linknode) FROM filenodes ORDER BY filenode DESC;"
  DDAE7A95B6B0FB27DFACC4051C41AA9CFF30C1E2|C5FD128DF314BDAF4217A731D1FEA3FD190B72D3
  DB9F6E90B4D31605949C7E5273E72FEADE04E6C4|0F6D8EE2C499636AA76108FB788093F12294EA87
  D5E651FDE2FF4278E3172BF3BEDACCAE9F466C89|7D590605FFCC46B04347C33CD85AB72A32A0EE62
  B80DE5D138758541C5F05265AD144AB9FA86D1DB|E90D3F198A5C22785DC5227854F46272524B6195
  B24D823C90409CA8CE2AC2BB22DAD5C6B9D17D4D|0F6D8EE2C499636AA76108FB788093F12294EA87
  8D8AC2F4A8AF10BA885E164A5F33CB4F91F8A0F8|7D590605FFCC46B04347C33CD85AB72A32A0EE62
  290DD67AD15DE1444C88A016BE6EC55CDF056C10|C5FD128DF314BDAF4217A731D1FEA3FD190B72D3
  1A4ECD744147A79966E5473A3B86B447533ABF9D|D099B4DBAAC9D3CA98D6C98090029EE79DFF9E98
