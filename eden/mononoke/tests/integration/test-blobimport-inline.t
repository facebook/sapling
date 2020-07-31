# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# setup repo, usegeneraldelta flag = false for forcing inline flag for file
# forcing running algo for inline revlof parsing
  $ hg init repo-hg --config format.usegeneraldelta=false

# Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > EOF

  $ touch file
  $ max_line_length=20
  $ lines_cnt=20
  $ commits_cnt=20
  $ lines_per_commit=5

# Fill the file randomly
  $ for (( i=0; i < $lines_cnt; i++ ))
  > do
  >  LINE_LENGTH=$(random_int $max_line_length)
  >  echo $(head -c 10000 /dev/urandom | tr -dc 'a-zA-Z0-9' | fold -w $LINE_LENGTH 2>/dev/null | head -n 1) >> file
  > done

  $ hg ci -Aqm "commit"$c

# Perform commits, every commit changes random lines, simulation of making diffs
  $ for (( c=0; c < $commits_cnt; c++ ))
  > do
  >   for ((change=0; change<$lines_per_commit; change++))
  >   do
  >     LINE_LENGTH=$(random_int $max_line_length)
  >     LINE_NUMBER=$(random_int $lines_cnt)
  >     CONTENT=$(head -c 10000 /dev/urandom | tr -dc 'a-zA-Z0-9' | fold -w $LINE_LENGTH 2>/dev/null | head -n 1)
  >     sed -i "$LINE_NUMBER""s/.*/$CONTENT/" file
  >   done
  >   hg ci -Aqm "commit"$c
  > done

  $ cd $TESTTMP

  $ setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
