fb_share_bookmarks
==================

An hg extension to share bookmarks peer to peer at Facebook.

Every time a client commits changes to a bookmark, this extension
pushes meta data with the bookmark name, unix user, project, project
path, and dev server to a mysql database. Users can then checkout
another user's bookmark using hg checkout. This is accomplished by
querying the database, downloading the necessary meta data, and
finally pulling the bookmark from the appropriate dev server.

The system is currently divided into two parts:
    * The hg extension
    * The helper utility

The hg extension is called by mercurial whenever a client commits
or checkouts a bookmark. In order to use the appropriate mysql
module at Facebook, the helper utility is built as a par and called
from the extension.

Naming Scheme
-------------

Currently, bookmark meta data is indexed by 'name', which has
the following format:

    project_name/user/bookmark_name

The project name is determined by reading the .projectid file.
If no .projectid file exists, it uses the name of the project's
root directory.

Users refer to a remote project via `hg checkout user/bookmark_name`.
The project name is implied via the current directory.

Local bookmarks which have a forward slash in their name take precedence
over remote bookmarks.

Caveats and Limitations
-----------------------

Currently, the extension relies on the helper utility to push and pull
information from the mysql database. This must be built using fbconfig
and fbmake, and therefore must be placed inside fbcode.

Bookmarks are not deleted from the database when they are deleted client side.

There are no mechanisms in place to restrict who can update bookmarks. Ideally,
users would only be allowed to update their own bookmarks.

The naming scheme does not allow for projects, unix users, or bookmark names
with forward slashes.
