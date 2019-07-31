  $ . "${TEST_FIXTURES}/library.sh"
  $ LFS_BLOBS="$(realpath "${TESTTMP}/blobs")"
  $ LFS_HELPER="$(realpath "${TESTTMP}/lfs")"
  $ LFS_URL="file://${LFS_BLOBS}"

# setup repo
  $ hg init repo-hg

# Init treemanifest and remotefilelog, and LFS storage
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > lfs=
  > [treemanifest]
  > server=True
  > [lfs]
  > url=$LFS_URL
  > threshold=10B
  > EOF

# Create a commit
  $ yes 2>/dev/null | head -c 100 > large
  $ hg commit -Aqm "large file"

# Push LFS blobs to the LFS "backend". That is how they get found later.
  $ mkdir "$LFS_BLOBS"
  $ hg log --template '{node}\n' | xargs -n 1 -- hg debuglfsupload -r

# Create the blobimport LFS helper
  $ cat > "$LFS_HELPER" <<EOF
  > #!/bin/bash
  > echo "lfs: $*" >&2
  > exec hg --config extensions.lfs= debuglfsreceive "\$@" "$LFS_URL"
  > EOF
  $ chmod +x "$LFS_HELPER"

# Run blobimport
  $ setup_mononoke_config
  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo --lfs-helper "$LFS_HELPER"
