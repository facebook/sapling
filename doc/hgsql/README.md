hgsql
=============

The hgsql extension allows multiple Mercurial servers to provide read and write access to a single repository at once. It does this by using a MySQL database to manage write locks and to propagate changes to the other servers.

This improves server scalability by allowing load to be distributed amongst multiple servers, and improves reliability by allowing individual servers to be taken down for repair without any downtime to the system as a whole.

Installing
==========

hgsql is part of this Mercurial distribution.  Enable it through configuration:

    :::ini
    [extensions]
    hgsql=

Configuring
-----------

**Server**

To set up a new hgsql repo, `hg init` an empty repository and add the appropriate hgsql configuration to the hgrc. Populate the database by pushing commits into this new repository. 

To set up other servers for an existing hgsql repo, `hg init` a new empty repository and give it the same configuration as the existing repo on the other machines. Run any read command (ex: hg log -l 1) to synchronize the repo with the database.

* `database` (required) - The name of the database to use.
* `enabled` (required) - Must be set to 'True'.
* `host` (required) - The host name of the database.
* `password` (required) - The password of the database to use. For testing only. DO NOT actually store your database password in plain text on your Mercurial servers. At Facebook we use an alternative mechanism for authenticating with the database. Users of hgsql are welcome to submit pull requests that enable other authentication mechanisms for their use cases.
* `port` (required) - The port of the database.
* `reponame` (required) - A unique name for this repository in the database. This is used to distinguish between multiple repositories being stored in the same database.
* `user` (required) - The name of the user for connecting to the database.
* `waittimeout` (optional) - The MySQL connection timeout to use. Useful when importing large repositories.  Defaults to 300 seconds.

An example server configuration:

    :::ini
    [hgsql]
    database = mydatabase
    enabled = True
    host = localhost
    password = aaa
    port = 12345
    reponame = myreponame
    user = mysqluser

**Database**

See `tests/hgsql/schema.sql` for required tables. You can create them using commands like:

    :::bash
    mysql -uroot -D hgsql < tests/hgsql/schema.sql

MySQL is recommended. Other implementations like MariaDB may have compatibility issues.

**Client**

Clients do not need hgsql installed, nor any special configuration to talk to hgsql based Mercurial servers.

Caveats & Troubleshooting
============

Because hgsql synchronizes when any request comes in (even read requests), all users who perform such requests must have write access to the repository.

Since hgsql synchronizes changes between servers, it's possible for servers to become out of sync if one server receives a write without the hgsql extension being enabled. If this happens, that server will refuse to receive any new data from the database and throw an exception. To fix it, strip the recent commits on the offending server using 'hg strip -r "badcommit:" --config extensions.hgsql=!' then try to resync with the db by running any read command (ex: hg log -l 1).

hgsql generally assumes that your repositories are write only and only provides rudimentary support for deleting commits. If you absolutely need to delete a commit, you can use `hg sqlstrip <rev>` to delete every commit newer than and including `<rev>`.  You will need to run this command on every hgsql server, since deletes are not propagated automatically.

Implementation Details
===========

hgsql works by keeping a table of all commit, manifest, and file revisions in the repository.

When a Mercurial server receives a request from a client, it first checks that it has the latest bits in the MySQL database. If there's new data, it downloads it before serving the request. Otherwise it serves the request from disk like normal. This means the majority of the read load is on the Mercurial server, and the database is just used for doing minimal synchronization.

When a client issues a write request to the Mercurial server (like a push), the Mercurial server obtains both the local Mercurial write lock, and a MySQL application level write lock that prevents all other servers from writing to that repo at the same time.

Contributing
============

Patches are welcome as pull requests, though they will be collapsed and rebased to maintain a linear history.

To run tests, copy `tests/hgsql/getdb.sh.example` to `tests/hgsql/getdb.sh`, and edit it to provide MySQL host, port, user and password. Then run the actual tests via:

    :::bash
    cd tests
    ./run-tests.py test-hgsql-*.t

We (Facebook) have to ask for a "Contributor License Agreement" from someone who sends in a patch or code that we want to include in the codebase. This is a legal requirement; a similar situation applies to Apache and other ASF projects.

If we ask you to fill out a CLA we'll direct you to our [online CLA page](https://developers.facebook.com/opensource/cla) where you can complete it easily. We use the same form as the Apache CLA so that friction is minimal.

License
=======

hgsql is made available under the terms of the GNU General Public License version 2, or any later version. See the COPYING file that accompanies this distribution for the full text of the license.
