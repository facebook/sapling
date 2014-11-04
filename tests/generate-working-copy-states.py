# generate proper file state to test working copy behavior
import sys
import os

# build the combination of possible states
combination = []
for base in [None, 'content1']:
    for parent in set([None, 'content2']) | set([base]):
        for wcc in set([None, 'content3']) | set([base, parent]):
            for tracked in (False, True):
                def statestring(content):
                    return content is None and 'missing' or content
                trackedstring = tracked and 'tracked' or 'untracked'
                filename = "%s_%s_%s-%s" % (statestring(base),
                                            statestring(parent),
                                            statestring(wcc),
                                            trackedstring)
                combination.append((filename, base, parent, wcc))

# make sure we have stable output
combination.sort()

# retrieve the state we must generate
target = sys.argv[1]

# compute file content
content = []
for filename, base, parent, wcc in combination:
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
