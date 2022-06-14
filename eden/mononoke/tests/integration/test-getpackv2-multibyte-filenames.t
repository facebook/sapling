# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ DIR='dir-multibyte-€'
  $ NAME='file-multibyte-€'
  $ BOOKMARK='book'

  $ ENABLED_DERIVED_DATA='["git_trees", "filenodes", "hgchangesets"]' setup_common_config

# THis is a bit clowntown. There is a bug in our tests right now that prevents
# us from creating files using UTF-8 names on Mercurial Python 3, so we
# instead, we ...  create the file using Git and then Gitimport it.

  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ mkdir "$DIR"
  $ echo "foo" > "$NAME"
  $ echo "bar" > "$DIR/$NAME"
  $ git add "$NAME" "$DIR/$NAME"
  $ git commit -qm "Add test file"
  $ gitimport "$GIT_REPO" --derive-hg full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 1 - Oid:* => Bid:* (glob)
  * Hg: *: HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Ref: *: Some(ChangesetId(Blake2(0b82d99309fc23ae5ae39c8eb93aaee9178a746f6cd882afddc183e0d3217195))) (glob)

# Set test bookmark

  $ quiet mononoke_admin bookmarks set "$BOOKMARK" 0b82d99309fc23ae5ae39c8eb93aaee9178a746f6cd882afddc183e0d3217195

# Start Mononoke

  $ start_and_wait_for_mononoke_server
# Try to get the file from Mononoke. We can't do this by updating to the rev,
# because that breaks over utf-8 characters as well.

  $ cd "$TESTTMP"
  $ hgmn_clone mononoke://$(mononoke_address)/repo "$HG_REPO" --noupdate --config extensions.remotenames=
  $ cd "$HG_REPO"
  $ hgmn cat -r "$BOOKMARK" "$NAME"
  foo
  $ hgmn cat -r "$BOOKMARK" "$DIR/$NAME"
  bar
