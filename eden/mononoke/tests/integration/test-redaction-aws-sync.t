# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Test that the AWS sync path is triggered when creating redaction key lists,
and that it fails gracefully when AWS command-line tools are not available.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

  $ cd $TESTTMP

setup repo with testtool_drawdag
  $ testtool_drawdag -R repo --no-default-files --derive-all --print-hg-hashes <<EOF
  > C
  > |
  > A
  > # modify: A "a" "a"
  > # modify: C "secret" "sensitive data"
  > # bookmark: A master_bookmark
  > # bookmark: C other_bookmark
  > EOF
  A=* (glob)
  C=* (glob)

start mononoke
  $ start_and_wait_for_mononoke_server

Test-case: AWS sync discovery failure without `--skip-aws-sync`.
How/setup: Hide the AWS command-line tools from this invocation.
Expectation: The key list is saved locally and manual sync instructions are printed.

  $ mkdir "$TESTTMP/no-aws-tools"

  $ PATH="$TESTTMP/no-aws-tools" mononoke_admin redaction create-key-list -R repo -i $C secret --main-bookmark master_bookmark --output-file rs_0 2>&1
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: * (glob)
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator
  
  Checking if sync to AWS is required...
    * Warning: Failed to discover AWS pod (*) (glob)
    * To sync manually, run: (glob)
    *cloud eks update-kubeconfig mononoke-cloud us-west-2 mononoke-prod* (glob)
    *kubectl get pods* (glob)
    *kubectl exec*monad redaction create-key-list-from-ids -R repo_shadow* (glob)


Test-case: Local-only key list creation with `--skip-aws-sync`.
How/setup: Create the same key list with AWS sync disabled and the restricted tool path.
Expectation: The key list is saved without any AWS sync output.

  $ PATH="$TESTTMP/no-aws-tools" mononoke_admin redaction create-key-list -R repo -i $C secret --main-bookmark master_bookmark --skip-aws-sync --output-file rs_1 2>&1
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: * (glob)
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator
