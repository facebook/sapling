#!/bin/bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# This is a script that should be used instead of default ui.ssh when performing
# pull/push/clone on Mononoke deployed in tupperware.
#
# This script will select a host using `srselect mononoke` and `tw ssh` to
# connect to it

set -e

usage() {
  echo '                                     _           '
  echo '                                    | |          '
  echo ' ____   ___  ____   ___  ____   ___ | |  _ _____ '
  echo '|    \ / _ \|  _ \ / _ \|  _ \ / _ \| |_/ ) ___ |'
  echo '| | | | |_| | | | | |_| | | | | |_| |  _ (| ____|'
  echo '|_|_|_|\___/|_| |_|\___/|_| |_|\___/|_| \_)_____)'
  echo '           _                                     '
  echo '          | |                                    '
  echo '  ___  ___| |__                                  '
  echo ' /___)/___)  _ \                                 '
  echo '|___ |___ | | | |                                '
  echo '(___/(___/|_| |_| '
  echo ''
  echo '       /\_/'\\
  echo '  ____/ o o '\\
  echo ' /~____  =ø= /'
  echo '(______)__m_m)'
  echo ''
  echo '  ____ ____ ____ ____ ____ '
  echo ' ||c |||l |||o |||n |||e ||'
  echo ' ||__|||__|||__|||__|||__||'
  echo ' |/__\|/__\|/__\|/__\|/__\|'
  echo 'For cloning from mononoke tupperware deployment use:'
  echo 'hg clone '\\
  echo '  --config ui.ssh="~/fbcode/scm/mononoke/hgcli/mononoke_ssh.sh" '\\
  echo '  --config ui.remotecmd="/packages/mononoke.hgcli/hgcli" '\\
  echo '  "ssh://mononoke//home/mononoke/<REPONAME>"'
  echo ''
  echo '  ____ ____ ____ ____ _________ ____ ____ ____ ____ '
  echo ' ||p |||u |||l |||l |||       |||p |||u |||s |||h ||'
  echo ' ||__|||__|||__|||__|||_______|||__|||__|||__|||__||'
  echo ' |/__\|/__\|/__\|/__\|/_______\|/__\|/__\|/__\|/__\|'
  echo 'For pull/push it is recommended to add following lines to .hg/hgrc'
  echo 'or use the same command args as for hg clone:'
  echo ''
  echo 'echo "'
  echo '[paths]'
  echo 'default = ssh://mononoke//home/mononoke/mononoke-config'
  echo ''
  echo '[ui]'
  echo 'ssh = ~/fbcode/scm/mononoke/hgcli/mononoke_ssh.sh'
  echo 'remotecmd = /packages/mononoke.hgcli/hgcli'
  echo '" >> <REPOPATH>/.hg/hgrc'
}

main() {
  if [ "$1" != "mononoke" ] || [[ "$2" != */home/mononoke/* ]]; then
    usage >&2
    exit 1
  fi
  shift

  local twhost
  twhost=$(srselect mononoke --svc-select-count 1 | cut -d' ' -f3)

  tw ssh "$twhost" -- "$@"
}

main "$@"
