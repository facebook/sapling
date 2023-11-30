# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ mononoke_testtool drawdag -R repo <<'EOF'
  > A-B-C
  >    \
  >     D
  > # bookmark: C main
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be


exactly one of `-T` or `--all-flags` are provided:
  $ mononoke_newadmin derived-data -R repo derive -B main
  error: the following required arguments were not provided:
    --derived-data-types <DERIVED_DATA_TYPES>
    --all-types
  
  Usage: newadmin derived-data <--repo-id <REPO_ID>|--repo-name <REPO_NAME>> derive --derived-data-types <DERIVED_DATA_TYPES> --all-types <--changeset-id <CHANGESET_ID>|--hg-id <HG_ID>|--bookmark <BOOKMARK>|--all-bookmarks>
  
  For more information, try '--help'.
  [2]
  $ mononoke_newadmin derived-data -R repo derive --all-types -T unodes -B main
  error: the argument '--all-types' cannot be used with '--derived-data-types <DERIVED_DATA_TYPES>'
  
  Usage: newadmin derived-data <--repo-id <REPO_ID>|--repo-name <REPO_NAME>> derive --derived-data-types <DERIVED_DATA_TYPES> --all-types <--changeset-id <CHANGESET_ID>|--hg-id <HG_ID>|--bookmark <BOOKMARK>|--all-bookmarks>
  
  For more information, try '--help'.
  [2]


derive all types:
  $ mononoke_newadmin derived-data -R repo derive --all-types -B main
  $ mononoke_newadmin derived-data -R repo derive --all-types -B main


confirm everything was derived:
  $ mononoke_newadmin derived-data -R repo exists -B main -T blame
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -B main -T changeset_info
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -B main -T deleted_manifest
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -B main -T fastlog
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -B main -T filenodes
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -B main -T fsnodes
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -B main -T unodes
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -B main -T hgchangesets
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -B main -T skeleton_manifests
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -B main -T bssm
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
