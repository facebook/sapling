#chg-compatible

Test migration between narrow-heads and non-narrow-heads

  $ enable remotenames amend
  $ setconfig experimental.narrow-heads=true visibility.enabled=true mutation.record=true mutation.enabled=true mutation.date="0 0" experimental.evolution= remotenames.rename.default=remote

  $ newrepo
  $ drawdag << 'EOS'
  > B C
  > |/
  > A
  > EOS

Make 'B' public, and 'C' draft.

  $ hg debugremotebookmark master $B
  $ hg phase $B
  112478962961147124edd43549aedd1a335e44bf: public
  $ hg phase $C
  dc0947a82db884575bb76ea10ac97b08536bfa03: draft

Migrate down.

  $ setconfig experimental.narrow-heads=false
  $ hg phase $B
  migrating repo to old-style visibility and phases
  (this restores the behavior to a known good state; post in Source Control @ FB if you have issues)
  (added 1 draft roots)
  112478962961147124edd43549aedd1a335e44bf: public
  $ hg phase $C
  dc0947a82db884575bb76ea10ac97b08536bfa03: draft
  $ drawdag << 'EOS'
  > D
  > |
  > A
  > EOS
  $ hg phase $D
  b18e25de2cf5fc4699a029ed635882849e53ef73: draft

Migrate up.

  $ setconfig experimental.narrow-heads=true
  $ hg phase $B
  migrating repo to new-style visibility and phases
  (this does not affect most workflows; post in Source Control @ FB if you have issues)
  112478962961147124edd43549aedd1a335e44bf: public
  $ hg phase $C
  dc0947a82db884575bb76ea10ac97b08536bfa03: draft
  $ hg phase $D
  b18e25de2cf5fc4699a029ed635882849e53ef73: draft

Test (legacy) secret commit migration.

  $ newrepo
  $ setconfig experimental.narrow-heads=false

  $ drawdag << 'EOS'
  >   D
  >   |
  > M C
  > |/
  > | B
  > |/
  > A
  > EOS
  migrating repo to old-style visibility and phases
  (this restores the behavior to a known good state; post in Source Control @ FB if you have issues)
  $ hg debugremotebookmark master $M
  $ hg phase --force --public $M
  $ hg phase --force --draft $C
  $ hg phase --force --secret $D+$B
  $ hg hide $D -q

Migrate up.

  $ setconfig experimental.narrow-heads=true
  $ hg log -G -T '{desc} {phase}'
  migrating repo to new-style visibility and phases
  (this does not affect most workflows; post in Source Control @ FB if you have issues)
  o  M public
  |
  | o  C draft
  |/
  | o  B draft
  |/
  o  A public
  
Migrate down.

  $ rm .hg/store/phaseroots
  $ setconfig experimental.narrow-heads=false
  $ hg log -G -T '{desc} {phase}'
  migrating repo to old-style visibility and phases
  (this restores the behavior to a known good state; post in Source Control @ FB if you have issues)
  (added 2 draft roots)
  o  M public
  |
  | o  C draft
  |/
  | o  B draft
  |/
  o  A public
  
 (Check: D is invisible)
