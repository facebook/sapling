# generate proper file state to test working copy behavior
import sys
import os

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

# sort to make sure we have stable output
combinations = sorted(generatestates(2, []))

# retrieve the state we must generate
target = sys.argv[1]

# compute file content
content = []
for filename, [base, parent, wcc] in combinations:
    if target == 'filelist':
        print filename
    elif target == 'base':
        content.append((filename, base))
    elif target == 'parent':
        content.append((filename, parent))
    elif target == 'wc':
        # Make sure there is content so the file gets written and can be
        # tracked. It will be deleted outside of this script.
        content.append((filename, wcc or 'TOBEDELETED'))
    else:
        print >> sys.stderr, "unknown target:", target
        sys.exit(1)

# write actual content
for filename, data in content:
    if data is not None:
        f = open(filename, 'w')
        f.write(data + '\n')
        f.close()
    elif os.path.exists(filename):
        os.remove(filename)
