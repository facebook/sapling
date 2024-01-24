# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ BUNDLE_OUTPUT="${TESTTMP}/repo.bundle"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# Create a few commits with changes
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -am "Add file1"
  [master (root-commit) 8ce3eae] Add file1
   1 file changed, 1 insertion(+)
   create mode 100644 file1
  $ git tag -a -m"new tag" first_tag
  $ mkdir src
  $ echo "fn main() -> Result<()>" > src/lib.rs
  $ git add .
  $ git commit -m "Added rust library"
  [master a612a21] Added rust library
   1 file changed, 1 insertion(+)
   create mode 100644 src/lib.rs
  $ git tag -a -m "tag for first release" release_v1.0
  $ mkdir src/test
  $ echo "fn test() -> Result<()>" > src/test/test.rs
  $ echo "mod test.rs" > src/mod.rs
  $ git add .
  $ git commit -m "Added rust tests"
  [master ca4b2b2] Added rust tests
   2 files changed, 2 insertions(+)
   create mode 100644 src/mod.rs
   create mode 100644 src/test/test.rs
  $ echo "This is new rust library. Use it on your own risk" > README.md
  $ git add .
  $ git commit -m "Added README.md"
  [master 7cb1854] Added README.md
   1 file changed, 1 insertion(+)
   create mode 100644 README.md
  $ git log --pretty=format:"%h %an %s %D"
  7cb1854 mononoke Added README.md HEAD -> master
  ca4b2b2 mononoke Added rust tests 
  a612a21 mononoke Added rust library tag: release_v1.0
  8ce3eae mononoke Add file1 tag: first_tag (no-eol)
# List all the known Git objects
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort | tee $TESTTMP/object_list
  018390a79208dbb5132fdfd6388b71e38b47750a blob src/test/test.rs
  10abb2a2c603aa94e5f6ad37ec13b2a11f01ec8d tree src/test
  1efe47d2a82e7ff4f45ef7dfe0e6475a3ab60b44 blob src/mod.rs
  2566e5da195021d7c25398a15dc9660d7e57295f blob src/lib.rs
  263607e3f9fb9e97d328a1e61a8960ab28102cac tree src
  29b45a08f5516275e22255b1bbdf74110bcdbbe8 tree src
  39cff93954c983af17b4b3b09c5c9c6084c29cdb blob README.md
  433eb172726bc7b6d60e8d68efb0f0ef4e67a667 blob file1
  5dd7faaa9ca449f1be40b141f6be940dfce29d13 tree 
  5f40977c5539a4764ad0c45cc6c438b1662ef2d2 tree 
  7cb1854dab21af54cce0a8ea4610bb6b3c4c1fd7 commit 
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 tag first_tag
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e commit 
  8ce5fb1a4ac630f015889504265021be5051f9d9 tree 
  a612a217c451a5401e52f03c0b3d336de0b778a0 commit 
  ca4b2b2194f4c925bff0a62d05687d5600987d2c commit 
  cb2ef838eb24e4667fee3a8b89c930234ae6e4bb tree 
  e84cb59a16997483e3fbfe17e8f41d1dff80d1a1 tag release_v1.0


# Create Git bundle out of this Git repo
  $ mononoke_newadmin git-bundle create from-path -o $BUNDLE_OUTPUT --git-repo-path $GIT_REPO_ORIGIN/.git
# Ensure that Git considers this a valid bundle
  $ git bundle verify $BUNDLE_OUTPUT
  $TESTTMP/repo.bundle is okay
  The bundle contains these 4 refs:
  * (glob)
  * (glob)
  * (glob)
  * (glob)
  The bundle records a complete history.
# Create a new empty repo
  $ mkdir $TESTTMP/git_client_repo
  $ cd $TESTTMP
# Clone using the bundle created above
  $ git clone $BUNDLE_OUTPUT git_client_repo
  Cloning into 'git_client_repo'...
  $ cd git_client_repo
# Get the repository log and verify if its the same as earlier
  $ git log --pretty=format:"%h %an %s %D"
  7cb1854 mononoke Added README.md HEAD -> master, origin/master, origin/HEAD
  ca4b2b2 mononoke Added rust tests 
  a612a21 mononoke Added rust library tag: release_v1.0
  8ce3eae mononoke Add file1 tag: first_tag (no-eol)
# Dump all the known Git objects into a file
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list

# Ensure that there are no differences between the set of objects by diffing both object list files
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
