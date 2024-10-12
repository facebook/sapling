# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Setup Mononoke
  $ . "${TEST_FIXTURES}/library.sh"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="repo"
  $ setup_common_config

# Setup git repo without LFS
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ mkdir dir
  $ echo "this is file2" > dir/file2
  $ git add dir/file2
  $ git commit -am "Add files"
  [master_bookmark (root-commit) c141531] Add files
   2 files changed, 2 insertions(+)
   create mode 100644 dir/file2
   create mode 100644 file1

# Setup a matching hg repo
  $ setup_common_config
  $ testtool_drawdag -R "${HG_REPO}" --no-default-files --derive-all << EOF
  > A
  > # message: A "Add files"
  > # modify: A "file1" "this is file1\n"
  > # modify: A "dir/file2" "this is file2\n"
  > EOF
  A=eb6e4b1ec39dbab1ffc2cf960272d20de189e919b1a2b15026b95603d7499999

# Validate with and without LFS, see that it's the same both ways round.
  $ check_git_wc --csid $A --git-repo-path "${GIT_REPO}/.git" --git-commit c141531763860520767a348d160d1c1c02339218 --git-lfs --scheduled-max 2
  $ check_git_wc --csid $A --git-repo-path "${GIT_REPO}/.git" --git-commit c141531763860520767a348d160d1c1c02339218 --scheduled-max 2

# Add an LFS pointer
  $ cd "$GIT_REPO"
  $ cat > lfs-file <<EOF
  > version https://git-lfs.github.com/spec/v1
  > oid sha256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03
  > size 6
  > EOF
  $ git add lfs-file
  $ git commit -am "LFS pointer"
  [master_bookmark 71ba3fb] LFS pointer
   1 file changed, 3 insertions(+)
   create mode 100644 lfs-file

# Update the hg repo, and confirm WC is correct
  $ testtool_drawdag -R "${HG_REPO}" --no-default-files --derive-all << EOF
  > B
  > |
  > A
  > # message: A "Add files"
  > # modify: A "file1" "this is file1\n"
  > # modify: A "dir/file2" "this is file2\n"
  > # modify: B "lfs-file" "hello\n"
  > # message: B "LFS pointer"
  > EOF
  A=eb6e4b1ec39dbab1ffc2cf960272d20de189e919b1a2b15026b95603d7499999
  B=a6abeefbc4d1f50887fa6e53c7227ebdbaa044f840445207481a73ea55789378

# This time, LFS works, non-LFS fails because the git sha256 is of the pointer, not the content
  $ check_git_wc --csid $B --git-repo-path "${GIT_REPO}/.git" --git-commit 71ba3fb41d4d75215d50edc4c2061ff3f21225b8 --git-lfs --scheduled-max 2
  $ check_git_wc --csid $B --git-repo-path "${GIT_REPO}/.git" --git-commit 71ba3fb41d4d75215d50edc4c2061ff3f21225b8 --scheduled-max 2
  *] Execution error: file 'lfs-file' has hash 94cb9a4fb124ed218aeeaefa7927680d5a261652f400f9d4f6a4e729c995d088 in git and 5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03 in Mononoke (glob)
  Error: Execution failed
  [1]

# With two commits present, validate the older git commit against newer Mononoke and vice-versa
  $ check_git_wc --csid $A --git-repo-path "${GIT_REPO}/.git" --git-commit 71ba3fb41d4d75215d50edc4c2061ff3f21225b8 --git-lfs --scheduled-max 2
  *] Execution error: file 'lfs-file' in git but not Bonsai (glob)
  Error: Execution failed
  [1]
  $ check_git_wc --csid $B --git-repo-path "${GIT_REPO}/.git" --git-commit c141531763860520767a348d160d1c1c02339218 --git-lfs --scheduled-max 2
  *] Execution error: File (root path)/lfs-file in Bonsai but not git (glob)
  Error: Execution failed
  [1]
