#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > amend=
  > rebase=
  > [experimental]
  > evolution = obsolete
  > [mutation]
  > enabled=true
  > record=false
  > [visibility]
  > enabled=true
  > EOF

  $ hg init repo
  $ cd repo

# verify template options

  $ hg commit --config 'ui.allowemptycommit=True' --template '{desc}\n' -m 'some commit'
  some commit

  $ hg commit --config 'ui.allowemptycommit=True' --template '{node}\n' -m 'some commit'
  15312f872b9e54934cd96e0db83e24aaefc2356d

  $ hg commit --config 'ui.allowemptycommit=True' --template '{node|short} ({phase}): {desc}\n' -m 'some commit'
  e3bf63af66d6 (draft): some commit

  $ echo hello > hello.txt
  $ hg add hello.txt

  $ hg amend --template '{node|short} ({phase}): {desc}\n'
  4a5cb78b8fc9 (draft): some commit

  $ echo 'good luck' > hello.txt

  $ hg amend --template '{node|short} ({phase}): {desc}\n' --to 4a5cb78b8fc9
  abort: --to does not support --template
  [255]
  $ hg commit --amend --template '{node|short} ({phase}): {desc}\n'
  1d0c24f9beeb (draft): some commit
