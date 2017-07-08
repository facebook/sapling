test sparse with --verbose and -T json

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > strip=
  > EOF

  $ echo a > show
  $ echo x > hide
  $ hg ci -Aqm 'initial'

  $ echo b > show
  $ echo y > hide
  $ echo aa > show2
  $ echo xx > hide2
  $ hg ci -Aqm 'two'

Verify basic --include and --reset

  $ hg up -q 0
  $ hg debugsparse --include 'hide' -Tjson
  [
   {
    "exclude_rules_added": 0,
    "files_added": 0,
    "files_conflicting": 0,
    "files_dropped": 1,
    "include_rules_added": 1,
    "profiles_added": 0
   }
  ]
  $ hg debugsparse --clear-rules
  $ hg debugsparse --include 'hide' --verbose
  removing show
  Profiles changed: 0
  Include rules changed: 1
  Exclude rules changed: 0

  $ hg debugsparse --reset -Tjson
  [
   {
    "exclude_rules_added": 0,
    "files_added": 1,
    "files_conflicting": 0,
    "files_dropped": 0,
    "include_rules_added": -1,
    "profiles_added": 0
   }
  ]
  $ hg debugsparse --include 'hide'
  $ hg debugsparse --reset --verbose
  getting show
  Profiles changed: 0
  Include rules changed: -1
  Exclude rules changed: 0

Verifying that problematic files still allow us to see the deltas when forcing:

  $ hg debugsparse --include 'show*'
  $ touch hide
  $ hg debugsparse --delete 'show*' --force -Tjson
  pending changes to 'hide'
  [
   {
    "exclude_rules_added": 0,
    "files_added": 0,
    "files_conflicting": 1,
    "files_dropped": 0,
    "include_rules_added": -1,
    "profiles_added": 0
   }
  ]
  $ hg debugsparse --include 'show*' --force
  pending changes to 'hide'
  $ hg debugsparse --delete 'show*' --force --verbose
  pending changes to 'hide'
  Profiles changed: 0
  Include rules changed: -1
  Exclude rules changed: 0
