  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config

show with nothing enabled yet
  $ mononoke_admin derived-data enabled-types show -R repo
  No derived data types enabled for repo 0

set requires the safety flag
  $ mononoke_admin derived-data enabled-types set -R repo -T unodes
  Error: marking a derived data type enabled writes config-like state that gates derivation.
  If you still want to proceed, re-run the 'set' command with '--i-know-what-i-am-doing' flag to unblock.
  [1]

set a type enabled for the repo
  $ mononoke_admin derived-data enabled-types set -R repo -T unodes --i-know-what-i-am-doing

show now lists the enabled type
  $ mononoke_admin derived-data enabled-types show -R repo
  unodes

list --type shows the repo row (with the campaign id NULL for a manual poke)
  $ mononoke_admin derived-data enabled-types list -R repo -T unodes
  +---------+-------------------+-----------------+
  | Repo ID | Derived Data Type | Root Request ID |
  +---------+-------------------+-----------------+
  | 0       | unodes            | NULL            |
  +---------+-------------------+-----------------+

set the same type again is idempotent (no error)
  $ mononoke_admin derived-data enabled-types set -R repo -T unodes --i-know-what-i-am-doing

show still returns exactly one type
  $ mononoke_admin derived-data enabled-types show -R repo
  unodes

setting a second type with a campaign id records it and shows both
  $ mononoke_admin derived-data enabled-types set -R repo -T fsnodes --root-request-id 42 --i-know-what-i-am-doing
  $ mononoke_admin derived-data enabled-types show -R repo
  fsnodes
  unodes

list without a type filter shows both rows across the repo
  $ mononoke_admin derived-data enabled-types list -R repo
  +---------+-------------------+-----------------+
  | Repo ID | Derived Data Type | Root Request ID |
  +---------+-------------------+-----------------+
  | 0       | fsnodes           | 42              |
  +---------+-------------------+-----------------+
  | 0       | unodes            | NULL            |
  +---------+-------------------+-----------------+

unset requires the safety flag
  $ mononoke_admin derived-data enabled-types unset -R repo -T unodes
  Error: marking a derived data type disabled writes config-like state that gates derivation.
  If you still want to proceed, re-run the 'unset' command with '--i-know-what-i-am-doing' flag to unblock.
  [1]

unset a currently-enabled type
  $ mononoke_admin derived-data enabled-types unset -R repo -T unodes --i-know-what-i-am-doing

show confirms the type is gone but the other remains
  $ mononoke_admin derived-data enabled-types show -R repo
  fsnodes

unset the same type again is idempotent (no error)
  $ mononoke_admin derived-data enabled-types unset -R repo -T unodes --i-know-what-i-am-doing

list confirms only the remaining row is present
  $ mononoke_admin derived-data enabled-types list -R repo
  +---------+-------------------+-----------------+
  | Repo ID | Derived Data Type | Root Request ID |
  +---------+-------------------+-----------------+
  | 0       | fsnodes           | 42              |
  +---------+-------------------+-----------------+
