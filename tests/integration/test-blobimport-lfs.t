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

# Setup Mononoke
  $ setup_mononoke_config

# Check that blobimport fails if given a helper that does not exist
  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo --lfs-helper "$LFS_HELPER"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * lfs_upload: importing blob Sha256(cc216c8df3beca4da80c551d178260b2cb844e04f7f7aa943d8c665162abca14) (glob)
  * lfs_upload: importing blob Sha256(cc216c8df3beca4da80c551d178260b2cb844e04f7f7aa943d8c665162abca14) (glob)
  * lfs_upload: importing blob Sha256(cc216c8df3beca4da80c551d178260b2cb844e04f7f7aa943d8c665162abca14) (glob)
  * lfs_upload: importing blob Sha256(cc216c8df3beca4da80c551d178260b2cb844e04f7f7aa943d8c665162abca14) (glob)
  * lfs_upload: importing blob Sha256(cc216c8df3beca4da80c551d178260b2cb844e04f7f7aa943d8c665162abca14) (glob)
  * lfs_upload: importing blob Sha256(cc216c8df3beca4da80c551d178260b2cb844e04f7f7aa943d8c665162abca14) (glob)
  * lfs_upload: importing blob Sha256(cc216c8df3beca4da80c551d178260b2cb844e04f7f7aa943d8c665162abca14) (glob)
  * failed to blobimport: While uploading changeset: 527169d71e0eac8abd0a25d18520cb3b8371edb5 (glob)
  * cause: While creating Changeset Some(HgNodeHash(Sha1(527169d71e0eac8abd0a25d18520cb3b8371edb5))), uuid: * (glob)
  * cause: While processing entries (glob)
  * cause: While uploading child entries (glob)
  * cause: While starting lfs_helper: "$TESTTMP/lfs" (glob)
  * cause: No such file or directory (os error 2) (glob)
  * root cause: Os { code: 2, kind: NotFound, message: "No such file or directory" } (glob)
  * error while blobimporting, Root cause: "failed to blobimport: While uploading changeset: 527169d71e0eac8abd0a25d18520cb3b8371edb5" (glob)
  * Error: failed to blobimport: While uploading changeset: 527169d71e0eac8abd0a25d18520cb3b8371edb5 (glob)
  Error: blobimport exited with a failure
  
  Stack backtrace:
      Run with RUST_LIB_BACKTRACE=1 env variable to display a backtrace
  
  [1]

# Create the blobimport LFS helper
  $ cat > "$LFS_HELPER" <<EOF
  > #!/bin/bash
  > echo "lfs: \$*" >&2
  > exec hg --config extensions.lfs= debuglfsreceive "\$@" "$LFS_URL"
  > EOF
  $ chmod +x "$LFS_HELPER"

# Run blobimport
  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo --no-create --lfs-helper "$LFS_HELPER"
