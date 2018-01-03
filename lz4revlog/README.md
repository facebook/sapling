lz4revlog
=========

Uses the fast
[lz4](http://en.wikipedia.org/wiki/LZ4_%28compression_algorithm%29) compression
algorithm to compress data stored by Mercurial.

On large real-world repositories, this can improve Mercurial's performance
significantly, though at the cost of 20-40% extra disk space used.

Installing
==========

First, install the [python-lz4](https://github.com/steeve/python-lz4) bindings
and make them available in your Python environment.

Then, run

    :::sh
    hg clone https://bitbucket.org/facebook/lz4revlog

In your user `.hgrc`, add the following lines:

    :::ini
    [extensions]
    lz4revlog = path/to/this/directory/lz4revlog.py

Using
=====

Mercurial decides what features to use at clone time, so to use lz4revlog you
will need to make fresh clones. As long as the extension is enabled, any fresh
clones you make will use lz4 compression.

Testing
=======

lz4revlog includes some basic tests, which can be run by cloning the Mercurial
repository to a separate directory:

    :::sh
    hg clone http://selenic.com/hg

and then running the tests with:

    :::sh
    cd path/to/this/directory/tests
    python path/to/hg/tests/run-tests.py


Contributing
============

Patches are welcome as pull requests, though they will be collapsed and rebased
to maintain a linear history. We may also set up a Phabricator project on
https://reviews.facebook.net/ soon.

We (Facebook) have to ask for a "Contributor License Agreement" from someone who
sends in a patch or code that we want to include in the codebase. This is a
legal requirement; a similar situation applies to Apache and other ASF projects.

If we ask you to fill out a CLA we'll direct you to our
[online CLA page](https://developers.facebook.com/opensource/cla) where you can
complete it easily. We use the same form as the Apache CLA so that friction is
minimal.

License
=======

lz4revlog is made available under the terms of the GNU General Public License
version 2, or any later version. See the COPYING file that accompanies this
distribution for the full text of the license.
