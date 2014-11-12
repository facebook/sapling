====================
Apache Docker Server
====================

This directory contains code for running a Mercurial hgweb server via
mod_wsgi with the Apache HTTP Server inside a Docker container.

.. important::

   This container is intended for testing purposes only: it is
   **not** meant to be suitable for production use.

Building Image
==============

The first step is to build a Docker image containing Apache and mod_wsgi::

  $ docker build -t hg-apache .

.. important::

   You should rebuild the image whenever the content of this directory
   changes. Rebuilding after pulling or when you haven't run the container
   in a while is typically a good idea.

Running the Server
==================

To run the container, you'll execute something like::

  $ docker run --rm -it -v `pwd`/../../..:/var/hg/source -p 8000:80 hg-apache

If you aren't a Docker expert:

* ``--rm`` will remove the container when it stops (so it doesn't clutter
  your system)
* ``-i`` will launch the container in interactive mode so stdin is attached
* ``-t`` will allocate a pseudo TTY
* ``-v src:dst`` will mount the host filesystem at ``src`` into ``dst``
  in the container. In our example, we assume you are running from this
  directory and use the source code a few directories up.
* ``-p 8000:80`` will publish port ``80`` on the container to port ``8000``
  on the host, allowing you to access the HTTP server on the host interface.
* ``hg-apache`` is the container image to run. This should correspond to what
  we build with ``docker build``.

.. important::

   The container **requires** that ``/var/hg/source`` contain the Mercurial
   source code.

   Upon start, the container will attempt an install of the source in that
   directory. If the architecture of the host machine doesn't match that of
   the Docker host (e.g. when running Boot2Docker under OS X), Mercurial's
   Python C extensions will fail to run. Be sure to ``make clean`` your
   host's source tree before mounting it in the container to avoid this.

When starting the container, you should see some start-up actions (including
a Mercurial install) and some output saying Apache has started::

Now if you load ``http://localhost:8000/`` (or whatever interface Docker
is using), you should see hgweb running!

For your convenience, we've created an empty repository available at
``/repo``. Feel free to populate it with ``hg push``.

Customizing the Server
======================

By default, the Docker container installs a basic hgweb config and an
empty dummy repository. It also uses some reasonable defaults for
mod_wsgi.

Customizing the WSGI Dispatcher And Mercurial Config
----------------------------------------------------

By default, the Docker environment installs a custom ``hgweb.wsgi``
file (based on the example in ``contrib/hgweb.wsgi``). The file
is installed into ``/var/hg/htdocs/hgweb.wsgi``.

A default hgweb configuration file is also installed. The ``hgwebconfig``
file from this directory is installed into ``/var/hg/htdocs/config``.

You have a few options for customizing these files.

The simplest is to hack up ``hgwebconfig`` and ``entrypoint.sh`` in
this directory and to rebuild the Docker image. This has the downside
that the Mercurial working copy is modified and you may accidentally
commit unwanted changes.

The next simplest is to copy this directory somewhere, make your changes,
then rebuild the image. No working copy changes involved.

The preferred solution is to mount a host file into the container and
overwrite the built-in defaults.

For example, say we create a custom hgweb config file in ``~/hgweb``. We
can start the container like so to install our custom config file::

  $ docker run -v ~/hgweb:/var/hg/htdocs/config ...

You can do something similar to install a custom WSGI dispatcher::

  $ docker run -v ~/hgweb.wsgi:/var/hg/htdocs/hgweb.wsgi ...

Managing Repositories
---------------------

Repositories are served from ``/var/hg/repos`` by default. This directory
is configured as a Docker volume. This means you can mount an existing
data volume container in the container so repository data is persisted
across container invocations. See
https://docs.docker.com/userguide/dockervolumes/ for more.

Alternatively, if you just want to perform lightweight repository
manipulation, open a shell in the container::

  $ docker exec -it <container> /bin/bash

Then run ``hg init``, etc to manipulate the repositories in ``/var/hg/repos``.

mod_wsgi Configuration Settings
-------------------------------

mod_wsgi settings can be controlled with the following environment
variables.

WSGI_PROCESSES
   Number of WSGI processes to run.
WSGI_THREADS
   Number of threads to run in each WSGI process
WSGI_MAX_REQUESTS
   Maximum number of requests each WSGI process may serve before it is
   reaped.

See https://code.google.com/p/modwsgi/wiki/ConfigurationDirectives#WSGIDaemonProcess
for more on these settings.

.. note::

   The default is to use 1 thread per process. The reason is that Mercurial
   doesn't perform well in multi-threaded mode due to the GIL. Most people
   run a single thread per process in production for this reason, so that's
   what we default to.
