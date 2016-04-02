# Helper script used for generating history and working copy files and content.
# The file's name corresponds to its history. The number of changesets can
# be specified on the command line. With 2 changesets, files with names like
# content1_content2_content1-untracked are generated. The first two filename
# segments describe the contents in the two changesets. The third segment
# ("content1-untracked") describes the state in the working copy, i.e.
# the file has content "content1" and is untracked (since it was previously
# tracked, it has been forgotten).
#
# This script generates the filenames and their content, but it's up to the
# caller to tell hg about the state.
#
# There are two subcommands:
#   filelist <numchangesets>
#   state <numchangesets> (<changeset>|wc)
#
# Typical usage:
#
# $ python $TESTDIR/generate-working-copy-states.py state 2 1
# $ hg addremove --similarity 0
# $ hg commit -m 'first'
#
# $ python $TESTDIR/generate-working-copy-states.py state 2 1
# $ hg addremove --similarity 0
# $ hg commit -m 'second'
#
# $ python $TESTDIR/generate-working-copy-states.py state 2 wc
# $ hg addremove --similarity 0
# $ hg forget *_*_*-untracked
# $ rm *_*_missing-*

from __future__ import absolute_import, print_function

import os
import sys

# Generates pairs of (filename, contents), where 'contents' is a list
# describing the file's content at each revision (or in the working copy).
# At each revision, it is either None or the file's actual content. When not
# None, it may be either new content or the same content as an earlier
# revisions, so all of (modified,clean,added,removed) can be tested.
def generatestates(maxchangesets, parentcontents):
    depth = len(parentcontents)
    if depth == maxchangesets + 1:
        for tracked in ('untracked', 'tracked'):
            filename = "_".join([(content is None and 'missing' or content) for
                                 content in parentcontents]) + "-" + tracked
            yield (filename, parentcontents)
    else:
        for content in (set([None, 'content' + str(depth + 1)]) |
                      set(parentcontents)):
            for combination in generatestates(maxchangesets,
                                              parentcontents + [content]):
                yield combination

# retrieve the command line arguments
target = sys.argv[1]
maxchangesets = int(sys.argv[2])
if target == 'state':
    depth = sys.argv[3]

# sort to make sure we have stable output
combinations = sorted(generatestates(maxchangesets, []))

# compute file content
content = []
for filename, states in combinations:
    if target == 'filelist':
        print(filename)
    elif target == 'state':
        if depth == 'wc':
            # Make sure there is content so the file gets written and can be
            # tracked. It will be deleted outside of this script.
            content.append((filename, states[maxchangesets] or 'TOBEDELETED'))
        else:
            content.append((filename, states[int(depth) - 1]))
    else:
        print("unknown target:", target, file=sys.stderr)
        sys.exit(1)

# write actual content
for filename, data in content:
    if data is not None:
        f = open(filename, 'wb')
        f.write(data + '\n')
        f.close()
    elif os.path.exists(filename):
        os.remove(filename)
