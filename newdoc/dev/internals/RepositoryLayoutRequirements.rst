*In http://selenic.com/pipermail/mercurial/2008-June/019561.html, mpm listed repository layout requirements (quote from that post):*

-------------------------

Here are the repo layout requirements:

a) does not randomize disk layout (ie hashing)
b) avoids case collisions
c) uses only ASCII
d) handles stupidly long filenames
e) avoids filesystem quirks like reserved words and characters
f) mostly human-readable (optional)
g) reversible (optional)


Point (a) is important for performance. Filesystems are likely to store sequentially created files in the same directory near each other on disk. Disks and operating systems are likely to cache such data. Thus, always reading and writing files in a consistent order gives the operating system and hardware its best opportunity to optimize layout. Ideally, the store layout should roughly mirror the working dir layout.

Point (g) is interesting. If we throw out cryptographically-strong hashing because of (a), we either have to expand names to meet (b) and (c) or throw out information and risk collisions. And we don't really want that, so (g) may effectively be implied. It's also worth considering (f): it makes understanding what's going on in the store a hell of a lot easier, especially when something goes wrong. Which again means: no hashing.

Which means that adding (d) is hard, because just about any solution to (b) and (c) will blow up path lengths, especially in the presence of interesting character sets. If our absolute paths are limited to a mere 255 characters and people want to have multi-line filenames, we've got a problem.

So we probably have to find a way to write longer filenames (NTFS's real limit is 32k).

-------------------------



*mpm on why hashing filenames doesn't solve the problem (quote from from* http://selenic.com/pipermail/mercurial/2008-June/019574.html*):*

-------------------------



Let's say you've cloned repo A, which gives you some set of files F that get stored in random order. To checkout that set of files F, we can either read them out of the repo in random (pessimal) order, or write them out to the working directory in random (pessimal) order. In both cases, we end up seeking like mad. The performance hit is around an order of magnitude: not something to sneeze at.

(And that's on an FS without decent metadata read-ahead. The difference will likely be more marked on something like cmason's btrfs.)

If we're clever, we can try reading every file out of the repo into memory in hash order, then writing them out in directory order. Which works just fine until you can't fit everything in memory. Then we discover we weren't so clever after all and we're back to seeking like mad, even if we're doing fairly large subsets. If our repo is 1G and we're batching chunks of 100M, we probably still end up seeking over the entire 1G of input space (or output space) to batch up each chunk.

Mercurial originally used a hashing scheme for its store 3 years ago and as soon as I moved my Linux test repo from one partition to another, performance suddenly went to hell because the sort order got pessimized by rsync alphabetizing things. I tried all sorts of variations on sorting, batching, etc., but none of them were even close in performance to the current scheme, namely mirroring the working directory layout and doing all operations in alphabetical order.

Further, simply cloning or cp'ing generally defragments things rather than making them worse.

-------------------------

Notes
~~~~~

According to http://selenic.com/pipermail/mercurial-devel/2008-June/006916.html, a reversible encoding of path names is currently (2008-06-28) required (see also http://selenic.com/pipermail/mercurial-devel/2008-June/006930.html).


