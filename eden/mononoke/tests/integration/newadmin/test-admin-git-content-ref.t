# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ export GIT_CONTENT_REFS_SCRIBE_CATEGORY=mononoke_git_content_ref
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ . "${TEST_FIXTURES}/library.sh"

Enable logging of git content ref updates
  $ mkdir -p $TESTTMP/scribe_logs
  $ touch $TESTTMP/scribe_logs/$GIT_CONTENT_REFS_SCRIBE_CATEGORY

setup configuration
  $ setup_common_config "blob_files"
  $ mononoke_testtool drawdag -R repo --derive-all <<'EOF'
  > A-B-C
  >    \
  >     D
  > # bookmark: C main
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be

# Create a tree to use in our tests
  $ mkdir -p testtree
  $ echo "test tree content" > testtree/file1
  $ git init -q
  $ cd testtree
  $ git add file1
  $ git commit -qm "test commit"
  $ TREE_HASH=$(git write-tree)
  $ echo "Tree hash: $TREE_HASH"
  Tree hash: * (glob)
# Create a blob to use in our tests
  $ echo "test content" > testfile
  $ BLOB_HASH=$(git hash-object -w testfile)
  $ echo "Blob hash: $BLOB_HASH"
  Blob hash: d670460b4b4aece5915caf5c68d12f560a9fe3e4
  $ cd ..

# Test creating a content ref pointing to a blob
  $ mononoke_admin git-content-ref -R repo create --ref-name test-blob-ref --git-hash $BLOB_HASH
  Content ref test-blob-ref pointing to d670460b4b4aece5915caf5c68d12f560a9fe3e4 (is_tree: false) has been added

# Test getting a content ref
  $ mononoke_admin git-content-ref -R repo get --ref-name test-blob-ref
  The content ref test-blob-ref points to d670460b4b4aece5915caf5c68d12f560a9fe3e4 (is_tree: false)

# Test creating a content ref pointing to a tree
  $ mononoke_admin git-content-ref -R repo create --ref-name test-tree-ref --git-hash $TREE_HASH --is-tree
  Content ref test-tree-ref pointing to 4acf132349d3e2de89cdf59c08f4c489f66491c8 (is_tree: true) has been added

# Test getting a tree content ref
  $ mononoke_admin git-content-ref -R repo get --ref-name test-tree-ref
  The content ref test-tree-ref points to 4acf132349d3e2de89cdf59c08f4c489f66491c8 (is_tree: true)

# Test updating a content ref
  $ mononoke_admin git-content-ref -R repo update --ref-name test-blob-ref --git-hash $TREE_HASH --is-tree
  Content ref test-blob-ref pointing to 4acf132349d3e2de89cdf59c08f4c489f66491c8 (is_tree: true) has been updated

# Test getting the updated content ref
  $ mononoke_admin git-content-ref -R repo get --ref-name test-blob-ref
  The content ref test-blob-ref points to 4acf132349d3e2de89cdf59c08f4c489f66491c8 (is_tree: true)

# Test getting a non-existent content ref
  $ mononoke_admin git-content-ref -R repo get --ref-name non-existent-ref
  Content ref non-existent-ref not found

# Test creating a content ref that already exists (should fail)
  $ mononoke_admin git-content-ref -R repo create --ref-name test-tree-ref --git-hash $BLOB_HASH
  Error: The content ref test-tree-ref already exists and it points to 4acf132349d3e2de89cdf59c08f4c489f66491c8 (is_tree: true)
  [1]

# Test deleting a content ref
  $ mononoke_admin git-content-ref -R repo delete --ref-names test-blob-ref
  Successfully deleted content refs ["test-blob-ref"]

# Test getting a deleted content ref
  $ mononoke_admin git-content-ref -R repo get --ref-name test-blob-ref
  Content ref test-blob-ref not found

# Test deleting multiple content refs
  $ mononoke_admin git-content-ref -R repo create --ref-name test-blob-ref2 --git-hash $BLOB_HASH
  Content ref test-blob-ref2 pointing to d670460b4b4aece5915caf5c68d12f560a9fe3e4 (is_tree: false) has been added
  $ mononoke_admin git-content-ref -R repo delete --ref-names test-tree-ref,test-blob-ref2
  Successfully deleted content refs ["test-tree-ref", "test-blob-ref2"]

# Test getting deleted content refs
  $ mononoke_admin git-content-ref -R repo get --ref-name test-tree-ref
  Content ref test-tree-ref not found
  $ mononoke_admin git-content-ref -R repo get --ref-name test-blob-ref2
  Content ref test-blob-ref2 not found

# Validate that the content ref updates are logged to scribe
  $ cat "$TESTTMP/scribe_logs/$GIT_CONTENT_REFS_SCRIBE_CATEGORY" | sort | jq '{repo_name,ref_name,git_hash,object_type}'
  {
    "repo_name": "repo",
    "ref_name": "test-blob-ref",
    "git_hash": "0000000000000000000000000000000000000000",
    "object_type": "NA"
  }
  {
    "repo_name": "repo",
    "ref_name": "test-blob-ref",
    "git_hash": "4acf132349d3e2de89cdf59c08f4c489f66491c8",
    "object_type": "tree"
  }
  {
    "repo_name": "repo",
    "ref_name": "test-blob-ref",
    "git_hash": "d670460b4b4aece5915caf5c68d12f560a9fe3e4",
    "object_type": "blob"
  }
  {
    "repo_name": "repo",
    "ref_name": "test-blob-ref2",
    "git_hash": "0000000000000000000000000000000000000000",
    "object_type": "NA"
  }
  {
    "repo_name": "repo",
    "ref_name": "test-blob-ref2",
    "git_hash": "d670460b4b4aece5915caf5c68d12f560a9fe3e4",
    "object_type": "blob"
  }
  {
    "repo_name": "repo",
    "ref_name": "test-tree-ref",
    "git_hash": "0000000000000000000000000000000000000000",
    "object_type": "NA"
  }
  {
    "repo_name": "repo",
    "ref_name": "test-tree-ref",
    "git_hash": "4acf132349d3e2de89cdf59c08f4c489f66491c8",
    "object_type": "tree"
  }
