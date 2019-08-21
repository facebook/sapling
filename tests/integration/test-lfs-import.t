  $ . "${TEST_FIXTURES}/library.sh"
  $ LFS_HELPER="$(realpath "${TESTTMP}/lfs")"

# Setup Mononoke
  $ setup_mononoke_config

# Create a mock LFS helper
  $ cat > "$LFS_HELPER" <<EOF
  > #!/bin/bash
  > echo "lfs: \$*" >&2
  > yes 2>/dev/null | head -c 128
  > EOF
  $ chmod +x "$LFS_HELPER"

# Test importing blobs
  $ cd "$TESTTMP"

  $ cat > bad_hash << EOF
  > version https://git-lfs.github.com/spec/v1
  > oid sha256:d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38
  > size 128
  > EOF
  $ lfs_import "$LFS_HELPER" "$(cat bad_hash)"
  * INFO using repo "repo" repoid RepositoryId(0) (glob)
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  Error: InvalidSha256(InvalidHash { expected: Sha256(d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38), effective: Sha256(14217d6d598954662767fb151ff41cc10261f233d60d92aba9fdaa8534c2db33) })
  [1]

  $ cat > bad_size << EOF
  > version https://git-lfs.github.com/spec/v1
  > oid sha256:14217d6d598954662767fb151ff41cc10261f233d60d92aba9fdaa8534c2db33
  > size 128
  > EOF
  $ lfs_import "$LFS_HELPER" "$(cat bad_hash)"
  * INFO using repo "repo" repoid RepositoryId(0) (glob)
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  lfs: d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38 128
  Error: InvalidSha256(InvalidHash { expected: Sha256(d6c9160e8ac378413dd55fba213970bbf55afdddaf85999dc3cf8d941f08fb38), effective: Sha256(14217d6d598954662767fb151ff41cc10261f233d60d92aba9fdaa8534c2db33) })
  [1]

  $ cat > ok << EOF
  > version https://git-lfs.github.com/spec/v1
  > oid sha256:14217d6d598954662767fb151ff41cc10261f233d60d92aba9fdaa8534c2db33
  > size 128
  > EOF
  $ lfs_import "$LFS_HELPER" "$(cat ok)"
  * INFO using repo "repo" repoid RepositoryId(0) (glob)
  lfs: 14217d6d598954662767fb151ff41cc10261f233d60d92aba9fdaa8534c2db33 128
