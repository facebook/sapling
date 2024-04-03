# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration:
  $ setup_common_config

  $ testtool_drawdag -R repo <<EOF
  > A-B-C
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

check usage:
  $ mononoke_newadmin derived-data -R repo derive-bulk
  error: the following required arguments were not provided:
    --derived-data-types <DERIVED_DATA_TYPES>
    --all-types
    --start <START>
    --end <END>
  
  Usage: newadmin derived-data <--repo-id <REPO_ID>|--repo-name <REPO_NAME>> derive-bulk --derived-data-types <DERIVED_DATA_TYPES> --all-types --start <START> --end <END>
  
  For more information, try '--help'.
  [2]


  $ mononoke_newadmin derived-data -R repo derive-bulk --all-types
  error: the following required arguments were not provided:
    --derived-data-types <DERIVED_DATA_TYPES>
    --start <START>
    --end <END>
  
  Usage: newadmin derived-data <--repo-id <REPO_ID>|--repo-name <REPO_NAME>> derive-bulk --derived-data-types <DERIVED_DATA_TYPES> --all-types --start <START> --end <END>
  
  For more information, try '--help'.
  [2]


  $ mononoke_newadmin derived-data -R repo derive-bulk --all-types --start $A
  error: the following required arguments were not provided:
    --derived-data-types <DERIVED_DATA_TYPES>
    --end <END>
  
  Usage: newadmin derived-data <--repo-id <REPO_ID>|--repo-name <REPO_NAME>> derive-bulk --derived-data-types <DERIVED_DATA_TYPES> --all-types --start <START> --end <END>
  
  For more information, try '--help'.
  [2]


  $ mononoke_newadmin derived-data derive-bulk --all-types --start $A --end $C
  error: the following required arguments were not provided:
    <--repo-id <REPO_ID>|--repo-name <REPO_NAME>>
  
  Usage: newadmin <--config-path <CONFIG_PATH>|--config-tier <CONFIG_TIER>|--prod> derived-data <--repo-id <REPO_ID>|--repo-name <REPO_NAME>> <COMMAND>
  
  For more information, try '--help'.
  [2]


  $ mononoke_newadmin derived-data -R repo derive-bulk --all-types -T blame --start $A --end $C
  error: the argument '--all-types' cannot be used with '--derived-data-types <DERIVED_DATA_TYPES>'
  
  Usage: newadmin derived-data <--repo-id <REPO_ID>|--repo-name <REPO_NAME>> derive-bulk --derived-data-types <DERIVED_DATA_TYPES> --all-types --start <START> --end <END>
  
  For more information, try '--help'.
  [2]


