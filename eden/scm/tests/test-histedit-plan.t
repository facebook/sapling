
  $ eagerepo
  $ enable histedit

Rearrange a stack without an editor. With no ANCESTOR, histedit starts from the
oldest commit in the plan:
  $ newclientrepo
  $ drawdag <<'EOS'
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $E
  $ sl histedit --plan "pick $D" --plan "pick $C" --plan "roll $E"
  $ tglog
  @  c4ee4a827480 'C'
  │
  o  5729e65467b2 'D'
  │
  o  112478962961 'B'
  │
  o  426bada5c675 'A'
  $ sl log -r . -T '{files}\n'
  C E

Rule lines can include the summary that --show-plan prints, and verbs can be
abbreviated:
  $ newclientrepo
  $ drawdag <<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $C
  $ sl histedit --plan "d $B B" --plan "p $C C"
  dropping changeset 112478: B
  $ tglog
  @  088d21ab9b28 'C'
  │
  o  426bada5c675 'A'

A --plan value can contain several rules, one per line, and can be mixed with
other --plan values. Blank lines and lines starting with # are ignored:
  $ newclientrepo
  $ drawdag <<'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $D
  $ sl histedit --plan "# reorder
  > pick $C
  > 
  > drop $B" --plan "pick $D"
  dropping changeset 112478: B
  $ tglog
  @  8c5708ed4326 'D'
  │
  o  088d21ab9b28 'C'
  │
  o  426bada5c675 'A'

An explicit ANCESTOR is still honoured. Every commit from it to the working
copy parent needs a rule, even if histedit.dropmissing is set:
  $ newclientrepo
  $ drawdag <<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $C
  $ sl histedit $A --plan "pick $B" --plan "pick $C"
  sl: parse error: missing rules for changeset 426bada5c675
  (use "drop 426bada5c675" to discard, see also: 'sl help -e histedit.config')
  [255]
  $ sl histedit $A --plan "pick $B" --plan "pick $C" --config histedit.dropmissing=true
  sl: parse error: missing rules for changeset 426bada5c675
  (use "drop 426bada5c675" to discard, see also: 'sl help -e histedit.config')
  [255]
  $ sl histedit $B --plan "pick $C" --plan "pick $B"
  $ tglog
  @  508221a61cea 'B'
  │
  o  088d21ab9b28 'C'
  │
  o  426bada5c675 'A'

'roll' keeps only the message of the commit it is rolled into, and 'into'
keeps only the message of this commit, so each keeps exactly one of the
original messages. Neither opens an editor:
  $ newclientrepo
  $ drawdag <<'EOS'
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $E
  $ sl histedit --plan "pick $B" --plan "into $C" --plan "pick $D" --plan "roll $E"
  $ sl log -r 'all()' -T '{desc} {files}\n'
  A A
  C B C
  D D E

This also holds for a run of them:
  $ newclientrepo
  $ drawdag <<'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $D
  $ sl histedit --plan "pick $B" --plan "into $C" --plan "roll $D"
  $ sl log -r . -T '{desc} {files}\n'
  C B C D
  $ newclientrepo
  $ drawdag <<'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $D
  $ sl histedit --plan "pick $B" --plan "into $C" --plan "into $D"
  $ sl log -r . -T '{desc} {files}\n'
  D B C D

'fold' combines the messages, as it does without --plan:
  $ newclientrepo
  $ drawdag <<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $C
  $ sl histedit --plan "pick $B" --plan "fold $C"
  $ sl log -r . -T '{desc}\n'
  B
  ***
  C

'into' is an ordinary verb, so it also works in a --commands file:
  $ newclientrepo
  $ drawdag <<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $C
  $ sl histedit $B --commands - <<EOF
  > pick $B
  > i $C
  > EOF
  $ sl log -r . -T '{desc} {files}\n'
  C B C

and after stopping for a conflict. Not on EdenFS, where continuing after a
conflict in a fold, roll or into fails with "working copy has pending changes",
with or without --plan:
#if no-eden
  $ newclientrepo
  $ drawdag <<'EOS'
  > D  # D/f = 3\n
  > |
  > C  # C/f = 2\n
  > |
  > B  # B/f = 1\n
  > |
  > A
  > EOS
  $ sl goto -q $D
  $ sl histedit --plan "pick $B" --plan "pick $D" --plan "into $C"
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  Fix up the change (pick a255f7246f36)
  (sl histedit --continue to resume)
  [1]
  $ sl histedit --show-plan --config extensions.fbhistedit= --config extensions.rebase=
  histedit plan (call "histedit --continue/--retry" to resume it or "histedit --abort" to abort it):
      pick a255f7246f36 D
      into c9e6a60ee394 C
  $ sl resolve --tool internal:other --all
  (no more unresolved files)
  continue: sl histedit --continue
  $ sl histedit --continue
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  Fix up the change (into c9e6a60ee394)
  (sl histedit --continue to resume)
  [1]
  $ sl resolve --tool internal:other --all
  (no more unresolved files)
  continue: sl histedit --continue
  $ sl histedit --continue
  $ sl log -r 'all()' -T '{desc} {files}\n'
  A A
  B B f
  C C D f
  $ sl cat -r . f
  2
#endif

A drop can go anywhere in the plan, but fold, roll and into need a kept commit
before it, so it cannot follow only drops:
  $ newclientrepo
  $ drawdag <<'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ sl goto -q $D
  $ sl histedit --plan "drop $B" --plan "roll $C" --plan "pick $D"
  sl: parse error: first changeset cannot use verb "roll"
  (roll combines a commit with the kept commit before it)
  [255]
  $ sl histedit --plan "fold $B" --plan "pick $C" --plan "pick $D"
  sl: parse error: first changeset cannot use verb "fold"
  (fold combines a commit with the kept commit before it)
  [255]
  $ sl histedit --plan "pick $B" --plan "pick $D" --plan "drop $C"
  dropping changeset 26805a: C
  $ tglog
  @  5729e65467b2 'D'
  │
  o  112478962961 'B'
  │
  o  426bada5c675 'A'

Invalid plans are rejected before anything is changed:
  $ sl histedit --plan "pick $B" --commands -
  abort: cannot use both --plan and --commands
  [255]
  $ sl histedit --plan "pick 123456789abc"
  abort: unknown changeset 123456789abc listed
  [255]
  $ sl histedit --plan "frob $B"
  sl: parse error: unknown action "frob"
  [255]
  $ sl histedit --continue --plan "pick $B"
  abort: --plan can only be used to start a new histedit
  [255]

  $ newclientrepo
  $ drawdag <<'EOS'
  > B C
  > |/
  > A
  > EOS
  $ sl histedit --plan "pick $B" --plan "pick $C"
  abort: the commits in the plan must have exactly one common root
  (pass the commit to start from as ANCESTOR)
  [255]
