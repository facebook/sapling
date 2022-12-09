#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ enable morestatus
  $ setconfig morestatus.show=True ui.origbackuppath=.hg/origs

Python utility:

  @command
  def createstate(args):
      """Create an interrupted state resolving 'hg update --merge' conflicts"""
      def createrepo():
          $ newrepo
          $ drawdag << 'EOS'
          > B C
          > |/   # B/A=B\n
          > A
          > EOS
      if not args or args[0] == "update":
          createrepo()
          $ hg up -C $C -q
          $ echo C > A
          $ hg up --merge $B -q
          warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
          [1]
      elif args[0] == "backout":
          createrepo()
          $ drawdag << 'EOS'
          > D  # D/A=D\n
          > |
          > desc(B)
          > EOS
          $ hg up -C $D -q
          $ hg backout $B -q
          warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
          [1]


  $ createstate

# There is only one working parent (which is good):

  $ hg parents -T "{desc}\n"
  B

# 'morestatus' message:

  $ hg status
  M A
  
  # The repository is in an unfinished *update* state.
  # Unresolved merge conflicts:
  # 
  #     A
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  # To continue:                hg goto --continue
  # To abort:                   hg goto --clean .    (warning: this will discard uncommitted changes)

# Cannot --continue right now

  $ hg goto --continue
  abort: outstanding merge conflicts
  (use 'hg resolve --list' to list, 'hg resolve --mark FILE' to mark resolved)
  [255]

# 'morestatus' message after resolve
# BAD: The unfinished merge state is confusing and there is no clear way to get out.

  $ hg resolve -m A
  (no more unresolved files)
  continue: hg goto --continue
  $ hg status
  M A
  
  # The repository is in an unfinished *update* state.
  # No unresolved merge conflicts.
  # To continue:                hg goto --continue
  # To abort:                   hg goto --clean .    (warning: this will discard uncommitted changes)

# To get rid of the state

  $ hg goto --continue
  $ hg status
  M A

# Test abort flow

  $ createstate

  $ hg goto --clean . -q
  $ hg status

# Test 'hg continue'

  $ hg continue
  abort: nothing to continue
  [255]

  $ createstate

  $ hg continue
  abort: outstanding merge conflicts
  (use 'hg resolve --list' to list, 'hg resolve --mark FILE' to mark resolved)
  [255]

  $ hg resolve -m A
  (no more unresolved files)
  continue: hg goto --continue

  $ hg continue

# Test 'hg continue' in a context that does not implement --continue.
# Choose 'backout' for this test. The 'backout' command does not have
# --continue.

  $ createstate backout

  $ hg continue
  abort: outstanding merge conflicts
  (use 'hg resolve -l' to see a list of conflicted files, 'hg resolve -m' to mark files as resolved)
  [255]
  $ hg resolve --all -t :local
  (no more unresolved files)
  $ hg status
  R B
  
  # The repository is in an unfinished *merge* state.
  # No unresolved merge conflicts.

# The state is confusing, but 'hg continue' can resolve it.

  $ hg continue
  (exiting merge state)
  $ hg status
  R B
