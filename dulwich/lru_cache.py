# Copyright (C) 2006, 2008 Canonical Ltd
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA

"""A simple least-recently-used (LRU) cache."""

from collections import deque


class LRUCache(object):
    """A class which manages a cache of entries, removing unused ones."""

    def __init__(self, max_cache=100, after_cleanup_count=None):
        self._cache = {}
        self._cleanup = {}
        self._queue = deque() # Track when things are accessed
        self._refcount = {} # number of entries in self._queue for each key
        self._update_max_cache(max_cache, after_cleanup_count)

    def __contains__(self, key):
        return key in self._cache

    def __getitem__(self, key):
        val = self._cache[key]
        self._record_access(key)
        return val

    def __len__(self):
        return len(self._cache)

    def add(self, key, value, cleanup=None):
        """Add a new value to the cache.

        Also, if the entry is ever removed from the queue, call cleanup.
        Passing it the key and value being removed.

        :param key: The key to store it under
        :param value: The object to store
        :param cleanup: None or a function taking (key, value) to indicate
                        'value' sohuld be cleaned up.
        """
        if key in self._cache:
            self._remove(key)
        self._cache[key] = value
        if cleanup is not None:
            self._cleanup[key] = cleanup
        self._record_access(key)

        if len(self._cache) > self._max_cache:
            # Trigger the cleanup
            self.cleanup()

    def get(self, key, default=None):
        if key in self._cache:
            return self[key]
        return default

    def keys(self):
        """Get the list of keys currently cached.

        Note that values returned here may not be available by the time you
        request them later. This is simply meant as a peak into the current
        state.

        :return: An unordered list of keys that are currently cached.
        """
        return self._cache.keys()

    def cleanup(self):
        """Clear the cache until it shrinks to the requested size.

        This does not completely wipe the cache, just makes sure it is under
        the after_cleanup_count.
        """
        # Make sure the cache is shrunk to the correct size
        while len(self._cache) > self._after_cleanup_count:
            self._remove_lru()
        # No need to compact the queue at this point, because the code that
        # calls this would have already triggered it based on queue length

    def __setitem__(self, key, value):
        """Add a value to the cache, there will be no cleanup function."""
        self.add(key, value, cleanup=None)

    def _record_access(self, key):
        """Record that key was accessed."""
        self._queue.append(key)
        # Can't use setdefault because you can't += 1 the result
        self._refcount[key] = self._refcount.get(key, 0) + 1

        # If our access queue is too large, clean it up too
        if len(self._queue) > self._compact_queue_length:
            self._compact_queue()

    def _compact_queue(self):
        """Compact the queue, leaving things in sorted last appended order."""
        new_queue = deque()
        for item in self._queue:
            if self._refcount[item] == 1:
                new_queue.append(item)
            else:
                self._refcount[item] -= 1
        self._queue = new_queue
        # All entries should be of the same size. There should be one entry in
        # queue for each entry in cache, and all refcounts should == 1
        if not (len(self._queue) == len(self._cache) ==
                len(self._refcount) == sum(self._refcount.itervalues())):
            raise AssertionError()

    def _remove(self, key):
        """Remove an entry, making sure to maintain the invariants."""
        cleanup = self._cleanup.pop(key, None)
        val = self._cache.pop(key)
        if cleanup is not None:
            cleanup(key, val)
        return val

    def _remove_lru(self):
        """Remove one entry from the lru, and handle consequences.

        If there are no more references to the lru, then this entry should be
        removed from the cache.
        """
        key = self._queue.popleft()
        self._refcount[key] -= 1
        if not self._refcount[key]:
            del self._refcount[key]
            self._remove(key)

    def clear(self):
        """Clear out all of the cache."""
        # Clean up in LRU order
        while self._cache:
            self._remove_lru()

    def resize(self, max_cache, after_cleanup_count=None):
        """Change the number of entries that will be cached."""
        self._update_max_cache(max_cache,
                               after_cleanup_count=after_cleanup_count)

    def _update_max_cache(self, max_cache, after_cleanup_count=None):
        self._max_cache = max_cache
        if after_cleanup_count is None:
            self._after_cleanup_count = self._max_cache * 8 / 10
        else:
            self._after_cleanup_count = min(after_cleanup_count, self._max_cache)

        self._compact_queue_length = 4*self._max_cache
        if len(self._queue) > self._compact_queue_length:
            self._compact_queue()
        self.cleanup()


class LRUSizeCache(LRUCache):
    """An LRUCache that removes things based on the size of the values.

    This differs in that it doesn't care how many actual items there are,
    it just restricts the cache to be cleaned up after so much data is stored.

    The values that are added must support len(value).
    """

    def __init__(self, max_size=1024*1024, after_cleanup_size=None,
                 compute_size=None):
        """Create a new LRUSizeCache.

        :param max_size: The max number of bytes to store before we start
            clearing out entries.
        :param after_cleanup_size: After cleaning up, shrink everything to this
            size.
        :param compute_size: A function to compute the size of the values. We
            use a function here, so that you can pass 'len' if you are just
            using simple strings, or a more complex function if you are using
            something like a list of strings, or even a custom object.
            The function should take the form "compute_size(value) => integer".
            If not supplied, it defaults to 'len()'
        """
        self._value_size = 0
        self._compute_size = compute_size
        if compute_size is None:
            self._compute_size = len
        # This approximates that texts are > 0.5k in size. It only really
        # effects when we clean up the queue, so we don't want it to be too
        # large.
        self._update_max_size(max_size, after_cleanup_size=after_cleanup_size)
        LRUCache.__init__(self, max_cache=max(int(max_size/512), 1))

    def add(self, key, value, cleanup=None):
        """Add a new value to the cache.

        Also, if the entry is ever removed from the queue, call cleanup.
        Passing it the key and value being removed.

        :param key: The key to store it under
        :param value: The object to store
        :param cleanup: None or a function taking (key, value) to indicate
                        'value' sohuld be cleaned up.
        """
        if key in self._cache:
            self._remove(key)
        value_len = self._compute_size(value)
        if value_len >= self._after_cleanup_size:
            return
        self._value_size += value_len
        self._cache[key] = value
        if cleanup is not None:
            self._cleanup[key] = cleanup
        self._record_access(key)

        if self._value_size > self._max_size:
            # Time to cleanup
            self.cleanup()

    def cleanup(self):
        """Clear the cache until it shrinks to the requested size.

        This does not completely wipe the cache, just makes sure it is under
        the after_cleanup_size.
        """
        # Make sure the cache is shrunk to the correct size
        while self._value_size > self._after_cleanup_size:
            self._remove_lru()

    def _remove(self, key):
        """Remove an entry, making sure to maintain the invariants."""
        val = LRUCache._remove(self, key)
        self._value_size -= self._compute_size(val)

    def resize(self, max_size, after_cleanup_size=None):
        """Change the number of bytes that will be cached."""
        self._update_max_size(max_size, after_cleanup_size=after_cleanup_size)
        max_cache = max(int(max_size/512), 1)
        self._update_max_cache(max_cache)

    def _update_max_size(self, max_size, after_cleanup_size=None):
        self._max_size = max_size
        if after_cleanup_size is None:
            self._after_cleanup_size = self._max_size * 8 / 10
        else:
            self._after_cleanup_size = min(after_cleanup_size, self._max_size)
