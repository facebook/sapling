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
  $ hg sparse --include 'hide' -Tjson
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
  $ hg sparse --clear-rules
  $ hg sparse --include 'hide' --verbose
  removing show
  Profile # change: 0
  Include rule # change: 1
  Exclude rule # change: 0

  $ hg sparse --reset -Tjson
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
  $ hg sparse --include 'hide'
  $ hg sparse --reset --verbose
  getting show
  Profile # change: 0
  Include rule # change: -1
  Exclude rule # change: 0

Verifying that problematic files still allow us to see the deltas when forcing:

  $ hg sparse --include 'show*'
  $ touch hide
  $ hg sparse --delete 'show*' --force -Tjson
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
  $ hg sparse --include 'show*' --force
  pending changes to 'hide'
  $ hg sparse --delete 'show*' --force --verbose
  pending changes to 'hide'
  Profile # change: 0
  Include rule # change: -1
  Exclude rule # change: 0
