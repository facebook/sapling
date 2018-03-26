#!/bin/bash

set -e
set -o pipefail

function setup_test_config_repo {
  setup_testdelay_config_repo "$@"
}

source ./library.sh

common_setup

export COMMIT_NUM=1

source ./shared-test-push-commits.sh