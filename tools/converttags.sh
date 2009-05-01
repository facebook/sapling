#!/bin/bash
# This shell script exists to convert hgsubversion tags to real hg tags.
# This will go away once hgsubversion's tags handling uses .hgtags directly.
hg tags | sed -E 's/([a-zA-Z0-9./_-]*) [^:]*:([a-f0-9]*)/\2 \1/' | grep -v ' tip$' > .hgtags
cat .hgtags | sed "$(
for x in `cat .hgtags| cut -f 1 -d ' '` ;do
    echo -n "s/$x/" ; hg log --template '{node}' -r $x ; echo -n '/g; '
done)" > .hgtags.new
mv .hgtags.new .hgtags
