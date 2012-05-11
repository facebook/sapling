/*
 parsers.c - efficient content parsing

 Copyright 2008 Matt Mackall <mpm@selenic.com> and others

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#include <Python.h>
#include <ctype.h>
#include <string.h>

#include "util.h"

static inline int hexdigit(const char *p, Py_ssize_t off)
{
	char c = p[off];

	if (c >= '0' && c <= '9')
		return c - '0';
	if (c >= 'a' && c <= 'f')
		return c - 'a' + 10;
	if (c >= 'A' && c <= 'F')
		return c - 'A' + 10;

	PyErr_SetString(PyExc_ValueError, "input contains non-hex character");
	return 0;
}

/*
 * Turn a hex-encoded string into binary.
 */
static PyObject *unhexlify(const char *str, int len)
{
	PyObject *ret;
	char *d;
	int i;

	ret = PyBytes_FromStringAndSize(NULL, len / 2);

	if (!ret)
		return NULL;

	d = PyBytes_AsString(ret);

	for (i = 0; i < len;) {
		int hi = hexdigit(str, i++);
		int lo = hexdigit(str, i++);
		*d++ = (hi << 4) | lo;
	}

	return ret;
}

/*
 * This code assumes that a manifest is stitched together with newline
 * ('\n') characters.
 */
static PyObject *parse_manifest(PyObject *self, PyObject *args)
{
	PyObject *mfdict, *fdict;
	char *str, *cur, *start, *zero;
	int len;

	if (!PyArg_ParseTuple(args, "O!O!s#:parse_manifest",
			      &PyDict_Type, &mfdict,
			      &PyDict_Type, &fdict,
			      &str, &len))
		goto quit;

	for (start = cur = str, zero = NULL; cur < str + len; cur++) {
		PyObject *file = NULL, *node = NULL;
		PyObject *flags = NULL;
		int nlen;

		if (!*cur) {
			zero = cur;
			continue;
		}
		else if (*cur != '\n')
			continue;

		if (!zero) {
			PyErr_SetString(PyExc_ValueError,
					"manifest entry has no separator");
			goto quit;
		}

		file = PyBytes_FromStringAndSize(start, zero - start);

		if (!file)
			goto bail;

		nlen = cur - zero - 1;

		node = unhexlify(zero + 1, nlen > 40 ? 40 : nlen);
		if (!node)
			goto bail;

		if (nlen > 40) {
			flags = PyBytes_FromStringAndSize(zero + 41,
							   nlen - 40);
			if (!flags)
				goto bail;

			if (PyDict_SetItem(fdict, file, flags) == -1)
				goto bail;
		}

		if (PyDict_SetItem(mfdict, file, node) == -1)
			goto bail;

		start = cur + 1;
		zero = NULL;

		Py_XDECREF(flags);
		Py_XDECREF(node);
		Py_XDECREF(file);
		continue;
	bail:
		Py_XDECREF(flags);
		Py_XDECREF(node);
		Py_XDECREF(file);
		goto quit;
	}

	if (len > 0 && *(cur - 1) != '\n') {
		PyErr_SetString(PyExc_ValueError,
				"manifest contains trailing garbage");
		goto quit;
	}

	Py_INCREF(Py_None);
	return Py_None;
quit:
	return NULL;
}

static PyObject *parse_dirstate(PyObject *self, PyObject *args)
{
	PyObject *dmap, *cmap, *parents = NULL, *ret = NULL;
	PyObject *fname = NULL, *cname = NULL, *entry = NULL;
	char *str, *cur, *end, *cpos;
	int state, mode, size, mtime;
	unsigned int flen;
	int len;

	if (!PyArg_ParseTuple(args, "O!O!s#:parse_dirstate",
			      &PyDict_Type, &dmap,
			      &PyDict_Type, &cmap,
			      &str, &len))
		goto quit;

	/* read parents */
	if (len < 40)
		goto quit;

	parents = Py_BuildValue("s#s#", str, 20, str + 20, 20);
	if (!parents)
		goto quit;

	/* read filenames */
	cur = str + 40;
	end = str + len;

	while (cur < end - 17) {
		/* unpack header */
		state = *cur;
		mode = getbe32(cur + 1);
		size = getbe32(cur + 5);
		mtime = getbe32(cur + 9);
		flen = getbe32(cur + 13);
		cur += 17;
		if (cur + flen > end || cur + flen < cur) {
			PyErr_SetString(PyExc_ValueError, "overflow in dirstate");
			goto quit;
		}

		entry = Py_BuildValue("ciii", state, mode, size, mtime);
		if (!entry)
			goto quit;
		PyObject_GC_UnTrack(entry); /* don't waste time with this */

		cpos = memchr(cur, 0, flen);
		if (cpos) {
			fname = PyBytes_FromStringAndSize(cur, cpos - cur);
			cname = PyBytes_FromStringAndSize(cpos + 1,
							   flen - (cpos - cur) - 1);
			if (!fname || !cname ||
			    PyDict_SetItem(cmap, fname, cname) == -1 ||
			    PyDict_SetItem(dmap, fname, entry) == -1)
				goto quit;
			Py_DECREF(cname);
		} else {
			fname = PyBytes_FromStringAndSize(cur, flen);
			if (!fname ||
			    PyDict_SetItem(dmap, fname, entry) == -1)
				goto quit;
		}
		cur += flen;
		Py_DECREF(fname);
		Py_DECREF(entry);
		fname = cname = entry = NULL;
	}

	ret = parents;
	Py_INCREF(ret);
quit:
	Py_XDECREF(fname);
	Py_XDECREF(cname);
	Py_XDECREF(entry);
	Py_XDECREF(parents);
	return ret;
}

/*
 * A base-16 trie for fast node->rev mapping.
 *
 * Positive value is index of the next node in the trie
 * Negative value is a leaf: -(rev + 1)
 * Zero is empty
 */
typedef struct {
	int children[16];
} nodetree;

/*
 * This class has two behaviours.
 *
 * When used in a list-like way (with integer keys), we decode an
 * entry in a RevlogNG index file on demand. Our last entry is a
 * sentinel, always a nullid.  We have limited support for
 * integer-keyed insert and delete, only at elements right before the
 * sentinel.
 *
 * With string keys, we lazily perform a reverse mapping from node to
 * rev, using a base-16 trie.
 */
typedef struct {
	PyObject_HEAD
	/* Type-specific fields go here. */
	PyObject *data;        /* raw bytes of index */
	PyObject **cache;      /* cached tuples */
	const char **offsets;  /* populated on demand */
	Py_ssize_t raw_length; /* original number of elements */
	Py_ssize_t length;     /* current number of elements */
	PyObject *added;       /* populated on demand */
	nodetree *nt;          /* base-16 trie */
	int ntlength;          /* # nodes in use */
	int ntcapacity;        /* # nodes allocated */
	int ntdepth;           /* maximum depth of tree */
	int ntsplits;          /* # splits performed */
	int ntrev;             /* last rev scanned */
	int ntlookups;         /* # lookups */
	int ntmisses;          /* # lookups that miss the cache */
	int inlined;
} indexObject;

static Py_ssize_t index_length(const indexObject *self)
{
	if (self->added == NULL)
		return self->length;
	return self->length + PyList_GET_SIZE(self->added);
}

static PyObject *nullentry;
static const char nullid[20];

static long inline_scan(indexObject *self, const char **offsets);

#if LONG_MAX == 0x7fffffffL
static char *tuple_format = "Kiiiiiis#";
#else
static char *tuple_format = "kiiiiiis#";
#endif

/*
 * Return a pointer to the beginning of a RevlogNG record.
 */
static const char *index_deref(indexObject *self, Py_ssize_t pos)
{
	if (self->inlined && pos > 0) {
		if (self->offsets == NULL) {
			self->offsets = malloc(self->raw_length *
					       sizeof(*self->offsets));
			if (self->offsets == NULL)
				return (const char *)PyErr_NoMemory();
			inline_scan(self, self->offsets);
		}
		return self->offsets[pos];
	}

	return PyString_AS_STRING(self->data) + pos * 64;
}

/*
 * RevlogNG format (all in big endian, data may be inlined):
 *    6 bytes: offset
 *    2 bytes: flags
 *    4 bytes: compressed length
 *    4 bytes: uncompressed length
 *    4 bytes: base revision
 *    4 bytes: link revision
 *    4 bytes: parent 1 revision
 *    4 bytes: parent 2 revision
 *   32 bytes: nodeid (only 20 bytes used)
 */
static PyObject *index_get(indexObject *self, Py_ssize_t pos)
{
	uint64_t offset_flags;
	int comp_len, uncomp_len, base_rev, link_rev, parent_1, parent_2;
	const char *c_node_id;
	const char *data;
	Py_ssize_t length = index_length(self);
	PyObject *entry;

	if (pos < 0)
		pos += length;

	if (pos < 0 || pos >= length) {
		PyErr_SetString(PyExc_IndexError, "revlog index out of range");
		return NULL;
	}

	if (pos == length - 1) {
		Py_INCREF(nullentry);
		return nullentry;
	}

	if (pos >= self->length - 1) {
		PyObject *obj;
		obj = PyList_GET_ITEM(self->added, pos - self->length + 1);
		Py_INCREF(obj);
		return obj;
	}

	if (self->cache) {
		if (self->cache[pos]) {
			Py_INCREF(self->cache[pos]);
			return self->cache[pos];
		}
	} else {
		self->cache = calloc(self->raw_length, sizeof(PyObject *));
		if (self->cache == NULL)
			return PyErr_NoMemory();
	}

	data = index_deref(self, pos);
	if (data == NULL)
		return NULL;

	offset_flags = getbe32(data + 4);
	if (pos == 0) /* mask out version number for the first entry */
		offset_flags &= 0xFFFF;
	else {
		uint32_t offset_high = getbe32(data);
		offset_flags |= ((uint64_t)offset_high) << 32;
	}

	comp_len = getbe32(data + 8);
	uncomp_len = getbe32(data + 12);
	base_rev = getbe32(data + 16);
	link_rev = getbe32(data + 20);
	parent_1 = getbe32(data + 24);
	parent_2 = getbe32(data + 28);
	c_node_id = data + 32;

	entry = Py_BuildValue(tuple_format, offset_flags, comp_len,
			      uncomp_len, base_rev, link_rev,
			      parent_1, parent_2, c_node_id, 20);

	if (entry)
		PyObject_GC_UnTrack(entry);

	self->cache[pos] = entry;
	Py_INCREF(entry);

	return entry;
}

/*
 * Return the 20-byte SHA of the node corresponding to the given rev.
 */
static const char *index_node(indexObject *self, Py_ssize_t pos)
{
	Py_ssize_t length = index_length(self);
	const char *data;

	if (pos == length - 1)
		return nullid;

	if (pos >= length)
		return NULL;

	if (pos >= self->length - 1) {
		PyObject *tuple, *str;
		tuple = PyList_GET_ITEM(self->added, pos - self->length + 1);
		str = PyTuple_GetItem(tuple, 7);
		return str ? PyString_AS_STRING(str) : NULL;
	}

	data = index_deref(self, pos);
	return data ? data + 32 : NULL;
}

static int nt_insert(indexObject *self, const char *node, int rev);

static int node_check(PyObject *obj, char **node, Py_ssize_t *nodelen)
{
	if (PyString_AsStringAndSize(obj, node, nodelen) == -1)
		return -1;
	if (*nodelen == 20)
		return 0;
	PyErr_SetString(PyExc_ValueError, "20-byte hash required");
	return -1;
}

static PyObject *index_insert(indexObject *self, PyObject *args)
{
	PyObject *obj;
	char *node;
	long offset;
	Py_ssize_t len, nodelen;

	if (!PyArg_ParseTuple(args, "lO", &offset, &obj))
		return NULL;

	if (!PyTuple_Check(obj) || PyTuple_GET_SIZE(obj) != 8) {
		PyErr_SetString(PyExc_TypeError, "8-tuple required");
		return NULL;
	}

	if (node_check(PyTuple_GET_ITEM(obj, 7), &node, &nodelen) == -1)
		return NULL;

	len = index_length(self);

	if (offset < 0)
		offset += len;

	if (offset != len - 1) {
		PyErr_SetString(PyExc_IndexError,
				"insert only supported at index -1");
		return NULL;
	}

	if (offset > INT_MAX) {
		PyErr_SetString(PyExc_ValueError,
				"currently only 2**31 revs supported");
		return NULL;
	}

	if (self->added == NULL) {
		self->added = PyList_New(0);
		if (self->added == NULL)
			return NULL;
	}

	if (PyList_Append(self->added, obj) == -1)
		return NULL;

	if (self->nt)
		nt_insert(self, node, (int)offset);

	Py_RETURN_NONE;
}

static void _index_clearcaches(indexObject *self)
{
	if (self->cache) {
		Py_ssize_t i;

		for (i = 0; i < self->raw_length; i++) {
			if (self->cache[i]) {
				Py_DECREF(self->cache[i]);
				self->cache[i] = NULL;
			}
		}
		free(self->cache);
		self->cache = NULL;
	}
	if (self->offsets) {
		free(self->offsets);
		self->offsets = NULL;
	}
	if (self->nt) {
		free(self->nt);
		self->nt = NULL;
	}
}

static PyObject *index_clearcaches(indexObject *self)
{
	_index_clearcaches(self);
	self->ntlength = self->ntcapacity = 0;
	self->ntdepth = self->ntsplits = 0;
	self->ntrev = -1;
	self->ntlookups = self->ntmisses = 0;
	Py_RETURN_NONE;
}

static PyObject *index_stats(indexObject *self)
{
	PyObject *obj = PyDict_New();

	if (obj == NULL)
		return NULL;

#define istat(__n, __d) \
	if (PyDict_SetItemString(obj, __d, PyInt_FromLong(self->__n)) == -1) \
		goto bail;

	if (self->added) {
		Py_ssize_t len = PyList_GET_SIZE(self->added);
		if (PyDict_SetItemString(obj, "index entries added",
					 PyInt_FromLong(len)) == -1)
			goto bail;
	}

	if (self->raw_length != self->length - 1)
		istat(raw_length, "revs on disk");
	istat(length, "revs in memory");
	istat(ntcapacity, "node trie capacity");
	istat(ntdepth, "node trie depth");
	istat(ntlength, "node trie count");
	istat(ntlookups, "node trie lookups");
	istat(ntmisses, "node trie misses");
	istat(ntrev, "node trie last rev scanned");
	istat(ntsplits, "node trie splits");

#undef istat

	return obj;

bail:
	Py_XDECREF(obj);
	return NULL;
}

static inline int nt_level(const char *node, Py_ssize_t level)
{
	int v = node[level>>1];
	if (!(level & 1))
		v >>= 4;
	return v & 0xf;
}

/*
 * Return values:
 *
 *   -4: match is ambiguous (multiple candidates)
 *   -2: not found
 * rest: valid rev
 */
static int nt_find(indexObject *self, const char *node, Py_ssize_t nodelen)
{
	int level, off;

	if (nodelen == 20 && node[0] == '\0' && memcmp(node, nullid, 20) == 0)
		return -1;

	if (self->nt == NULL)
		return -2;

	for (level = off = 0; level < nodelen; level++) {
		int k = nt_level(node, level);
		nodetree *n = &self->nt[off];
		int v = n->children[k];

		if (v < 0) {
			const char *n;
			v = -v - 1;
			n = index_node(self, v);
			if (n == NULL)
				return -2;
			return memcmp(node, n, nodelen > 20 ? 20 : nodelen)
				? -2 : v;
		}
		if (v == 0)
			return -2;
		off = v;
	}
	/* multiple matches against an ambiguous prefix */
	return -4;
}

static int nt_new(indexObject *self)
{
	if (self->ntlength == self->ntcapacity) {
		self->ntcapacity *= 2;
		self->nt = realloc(self->nt,
				   self->ntcapacity * sizeof(nodetree));
		if (self->nt == NULL) {
			PyErr_SetString(PyExc_MemoryError, "out of memory");
			return -1;
		}
		memset(&self->nt[self->ntlength], 0,
		       sizeof(nodetree) * (self->ntcapacity - self->ntlength));
	}
	return self->ntlength++;
}

static int nt_insert(indexObject *self, const char *node, int rev)
{
	int level = 0;
	int off = 0;

	while (level < 20) {
		int k = nt_level(node, level);
		nodetree *n;
		int v;

		n = &self->nt[off];
		v = n->children[k];

		if (v == 0) {
			n->children[k] = -rev - 1;
			return 0;
		}
		if (v < 0) {
			const char *oldnode = index_node(self, -v - 1);
			int noff;

			if (!oldnode || !memcmp(oldnode, node, 20)) {
				n->children[k] = -rev - 1;
				return 0;
			}
			noff = nt_new(self);
			if (noff == -1)
				return -1;
			/* self->nt may have been changed by realloc */
			self->nt[off].children[k] = noff;
			off = noff;
			n = &self->nt[off];
			n->children[nt_level(oldnode, ++level)] = v;
			if (level > self->ntdepth)
				self->ntdepth = level;
			self->ntsplits += 1;
		} else {
			level += 1;
			off = v;
		}
	}

	return -1;
}

static int nt_init(indexObject *self)
{
	if (self->nt == NULL) {
		self->ntcapacity = self->raw_length < 4
			? 4 : self->raw_length / 2;
		self->nt = calloc(self->ntcapacity, sizeof(nodetree));
		if (self->nt == NULL) {
			PyErr_NoMemory();
			return -1;
		}
		self->ntlength = 1;
		self->ntrev = (int)index_length(self) - 1;
		self->ntlookups = 1;
		self->ntmisses = 0;
	}
	return 0;
}

/*
 * Return values:
 *
 *   -3: error (exception set)
 *   -2: not found (no exception set)
 * rest: valid rev
 */
static int index_find_node(indexObject *self,
			   const char *node, Py_ssize_t nodelen)
{
	int rev;

	self->ntlookups++;
	rev = nt_find(self, node, nodelen);
	if (rev >= -1)
		return rev;

	if (nt_init(self) == -1)
		return -3;

	/*
	 * For the first handful of lookups, we scan the entire index,
	 * and cache only the matching nodes. This optimizes for cases
	 * like "hg tip", where only a few nodes are accessed.
	 *
	 * After that, we cache every node we visit, using a single
	 * scan amortized over multiple lookups.  This gives the best
	 * bulk performance, e.g. for "hg log".
	 */
	if (self->ntmisses++ < 4) {
		for (rev = self->ntrev - 1; rev >= 0; rev--) {
			const char *n = index_node(self, rev);
			if (n == NULL)
				return -2;
			if (memcmp(node, n, nodelen > 20 ? 20 : nodelen) == 0) {
				if (nt_insert(self, n, rev) == -1)
					return -3;
				break;
			}
		}
	} else {
		for (rev = self->ntrev - 1; rev >= 0; rev--) {
			const char *n = index_node(self, rev);
			if (n == NULL) {
				self->ntrev = rev + 1;
				return -2;
			}
			if (nt_insert(self, n, rev) == -1) {
				self->ntrev = rev + 1;
				return -3;
			}
			if (memcmp(node, n, nodelen > 20 ? 20 : nodelen) == 0) {
				break;
			}
		}
		self->ntrev = rev;
	}

	if (rev >= 0)
		return rev;
	return -2;
}

static PyObject *raise_revlog_error(void)
{
	static PyObject *errclass;
	PyObject *mod = NULL, *errobj;

	if (errclass == NULL) {
		PyObject *dict;

		mod = PyImport_ImportModule("mercurial.error");
		if (mod == NULL)
			goto classfail;

		dict = PyModule_GetDict(mod);
		if (dict == NULL)
			goto classfail;

		errclass = PyDict_GetItemString(dict, "RevlogError");
		if (errclass == NULL) {
			PyErr_SetString(PyExc_SystemError,
					"could not find RevlogError");
			goto classfail;
		}
		Py_INCREF(errclass);
	}

	errobj = PyObject_CallFunction(errclass, NULL);
	if (errobj == NULL)
		return NULL;
	PyErr_SetObject(errclass, errobj);
	return errobj;

classfail:
	Py_XDECREF(mod);
	return NULL;
}

static PyObject *index_getitem(indexObject *self, PyObject *value)
{
	char *node;
	Py_ssize_t nodelen;
	int rev;

	if (PyInt_Check(value))
		return index_get(self, PyInt_AS_LONG(value));

	if (PyString_AsStringAndSize(value, &node, &nodelen) == -1)
		return NULL;
	rev = index_find_node(self, node, nodelen);
	if (rev >= -1)
		return PyInt_FromLong(rev);
	if (rev == -2)
		raise_revlog_error();
	return NULL;
}

static PyObject *index_m_get(indexObject *self, PyObject *args)
{
	char *node;
	int nodelen, rev;

	if (!PyArg_ParseTuple(args, "s#", &node, &nodelen))
		return NULL;

	rev = index_find_node(self, node, nodelen);
	if (rev ==  -3)
		return NULL;
	if (rev == -2)
		Py_RETURN_NONE;
	return PyInt_FromLong(rev);
}

static int index_contains(indexObject *self, PyObject *value)
{
	char *node;
	Py_ssize_t nodelen;

	if (PyInt_Check(value)) {
		long rev = PyInt_AS_LONG(value);
		return rev >= -1 && rev < index_length(self);
	}

	if (!PyString_Check(value))
		return 0;

	node = PyString_AS_STRING(value);
	nodelen = PyString_GET_SIZE(value);

	switch (index_find_node(self, node, nodelen)) {
	case -3:
		return -1;
	case -2:
		return 0;
	default:
		return 1;
	}
}

/*
 * Invalidate any trie entries introduced by added revs.
 */
static void nt_invalidate_added(indexObject *self, Py_ssize_t start)
{
	Py_ssize_t i, len = PyList_GET_SIZE(self->added);

	for (i = start; i < len; i++) {
		PyObject *tuple = PyList_GET_ITEM(self->added, i);
		PyObject *node = PyTuple_GET_ITEM(tuple, 7);

		nt_insert(self, PyString_AS_STRING(node), -1);
	}

	if (start == 0) {
		Py_DECREF(self->added);
		self->added = NULL;
	}
}

/*
 * Delete a numeric range of revs, which must be at the end of the
 * range, but exclude the sentinel nullid entry.
 */
static int index_slice_del(indexObject *self, PyObject *item)
{
	Py_ssize_t start, stop, step, slicelength;
	Py_ssize_t length = index_length(self);

	if (PySlice_GetIndicesEx((PySliceObject*)item, length,
				 &start, &stop, &step, &slicelength) < 0)
		return -1;

	if (slicelength <= 0)
		return 0;

	if ((step < 0 && start < stop) || (step > 0 && start > stop))
		stop = start;

	if (step < 0) {
		stop = start + 1;
		start = stop + step*(slicelength - 1) - 1;
		step = -step;
	}

	if (step != 1) {
		PyErr_SetString(PyExc_ValueError,
				"revlog index delete requires step size of 1");
		return -1;
	}

	if (stop != length - 1) {
		PyErr_SetString(PyExc_IndexError,
				"revlog index deletion indices are invalid");
		return -1;
	}

	if (start < self->length - 1) {
		if (self->nt) {
			Py_ssize_t i;

			for (i = start + 1; i < self->length - 1; i++) {
				const char *node = index_node(self, i);

				if (node)
					nt_insert(self, node, -1);
			}
			if (self->added)
				nt_invalidate_added(self, 0);
			if (self->ntrev > start)
				self->ntrev = (int)start;
		}
		self->length = start + 1;
		return 0;
	}

	if (self->nt) {
		nt_invalidate_added(self, start - self->length + 1);
		if (self->ntrev > start)
			self->ntrev = (int)start;
	}
	return self->added
		? PyList_SetSlice(self->added, start - self->length + 1,
				  PyList_GET_SIZE(self->added), NULL)
		: 0;
}

/*
 * Supported ops:
 *
 * slice deletion
 * string assignment (extend node->rev mapping)
 * string deletion (shrink node->rev mapping)
 */
static int index_assign_subscript(indexObject *self, PyObject *item,
				  PyObject *value)
{
	char *node;
	Py_ssize_t nodelen;
	long rev;

	if (PySlice_Check(item) && value == NULL)
		return index_slice_del(self, item);

	if (node_check(item, &node, &nodelen) == -1)
		return -1;

	if (value == NULL)
		return self->nt ? nt_insert(self, node, -1) : 0;
	rev = PyInt_AsLong(value);
	if (rev > INT_MAX || rev < 0) {
		if (!PyErr_Occurred())
			PyErr_SetString(PyExc_ValueError, "rev out of range");
		return -1;
	}
	return nt_insert(self, node, (int)rev);
}

/*
 * Find all RevlogNG entries in an index that has inline data. Update
 * the optional "offsets" table with those entries.
 */
static long inline_scan(indexObject *self, const char **offsets)
{
	const char *data = PyString_AS_STRING(self->data);
	const char *end = data + PyString_GET_SIZE(self->data);
	const long hdrsize = 64;
	long incr = hdrsize;
	Py_ssize_t len = 0;

	while (data + hdrsize <= end) {
		uint32_t comp_len;
		const char *old_data;
		/* 3rd element of header is length of compressed inline data */
		comp_len = getbe32(data + 8);
		incr = hdrsize + comp_len;
		if (incr < hdrsize)
			break;
		if (offsets)
			offsets[len] = data;
		len++;
		old_data = data;
		data += incr;
		if (data <= old_data)
			break;
	}

	if (data != end && data + hdrsize != end) {
		if (!PyErr_Occurred())
			PyErr_SetString(PyExc_ValueError, "corrupt index file");
		return -1;
	}

	return len;
}

static int index_init(indexObject *self, PyObject *args)
{
	PyObject *data_obj, *inlined_obj;
	Py_ssize_t size;

	if (!PyArg_ParseTuple(args, "OO", &data_obj, &inlined_obj))
		return -1;
	if (!PyString_Check(data_obj)) {
		PyErr_SetString(PyExc_TypeError, "data is not a string");
		return -1;
	}
	size = PyString_GET_SIZE(data_obj);

	self->inlined = inlined_obj && PyObject_IsTrue(inlined_obj);
	self->data = data_obj;
	self->cache = NULL;

	self->added = NULL;
	self->offsets = NULL;
	self->nt = NULL;
	self->ntlength = self->ntcapacity = 0;
	self->ntdepth = self->ntsplits = 0;
	self->ntlookups = self->ntmisses = 0;
	self->ntrev = -1;
	Py_INCREF(self->data);

	if (self->inlined) {
		long len = inline_scan(self, NULL);
		if (len == -1)
			goto bail;
		self->raw_length = len;
		self->length = len + 1;
	} else {
		if (size % 64) {
			PyErr_SetString(PyExc_ValueError, "corrupt index file");
			goto bail;
		}
		self->raw_length = size / 64;
		self->length = self->raw_length + 1;
	}

	return 0;
bail:
	return -1;
}

static PyObject *index_nodemap(indexObject *self)
{
	Py_INCREF(self);
	return (PyObject *)self;
}

static void index_dealloc(indexObject *self)
{
	_index_clearcaches(self);
	Py_DECREF(self->data);
	Py_XDECREF(self->added);
	PyObject_Del(self);
}

static PySequenceMethods index_sequence_methods = {
	(lenfunc)index_length,   /* sq_length */
	0,                       /* sq_concat */
	0,                       /* sq_repeat */
	(ssizeargfunc)index_get, /* sq_item */
	0,                       /* sq_slice */
	0,                       /* sq_ass_item */
	0,                       /* sq_ass_slice */
	(objobjproc)index_contains, /* sq_contains */
};

static PyMappingMethods index_mapping_methods = {
	(lenfunc)index_length,                 /* mp_length */
	(binaryfunc)index_getitem,             /* mp_subscript */
	(objobjargproc)index_assign_subscript, /* mp_ass_subscript */
};

static PyMethodDef index_methods[] = {
	{"clearcaches", (PyCFunction)index_clearcaches, METH_NOARGS,
	 "clear the index caches"},
	{"get", (PyCFunction)index_m_get, METH_VARARGS,
	 "get an index entry"},
	{"insert", (PyCFunction)index_insert, METH_VARARGS,
	 "insert an index entry"},
	{"stats", (PyCFunction)index_stats, METH_NOARGS,
	 "stats for the index"},
	{NULL} /* Sentinel */
};

static PyGetSetDef index_getset[] = {
	{"nodemap", (getter)index_nodemap, NULL, "nodemap", NULL},
	{NULL} /* Sentinel */
};

static PyTypeObject indexType = {
	PyObject_HEAD_INIT(NULL)
	0,                         /* ob_size */
	"parsers.index",           /* tp_name */
	sizeof(indexObject),       /* tp_basicsize */
	0,                         /* tp_itemsize */
	(destructor)index_dealloc, /* tp_dealloc */
	0,                         /* tp_print */
	0,                         /* tp_getattr */
	0,                         /* tp_setattr */
	0,                         /* tp_compare */
	0,                         /* tp_repr */
	0,                         /* tp_as_number */
	&index_sequence_methods,   /* tp_as_sequence */
	&index_mapping_methods,    /* tp_as_mapping */
	0,                         /* tp_hash */
	0,                         /* tp_call */
	0,                         /* tp_str */
	0,                         /* tp_getattro */
	0,                         /* tp_setattro */
	0,                         /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT,        /* tp_flags */
	"revlog index",            /* tp_doc */
	0,                         /* tp_traverse */
	0,                         /* tp_clear */
	0,                         /* tp_richcompare */
	0,                         /* tp_weaklistoffset */
	0,                         /* tp_iter */
	0,                         /* tp_iternext */
	index_methods,             /* tp_methods */
	0,                         /* tp_members */
	index_getset,              /* tp_getset */
	0,                         /* tp_base */
	0,                         /* tp_dict */
	0,                         /* tp_descr_get */
	0,                         /* tp_descr_set */
	0,                         /* tp_dictoffset */
	(initproc)index_init,      /* tp_init */
	0,                         /* tp_alloc */
};

/*
 * returns a tuple of the form (index, index, cache) with elements as
 * follows:
 *
 * index: an index object that lazily parses RevlogNG records
 * cache: if data is inlined, a tuple (index_file_content, 0), else None
 *
 * added complications are for backwards compatibility
 */
static PyObject *parse_index2(PyObject *self, PyObject *args)
{
	PyObject *tuple = NULL, *cache = NULL;
	indexObject *idx;
	int ret;

	idx = PyObject_New(indexObject, &indexType);
	if (idx == NULL)
		goto bail;

	ret = index_init(idx, args);
	if (ret == -1)
		goto bail;

	if (idx->inlined) {
		cache = Py_BuildValue("iO", 0, idx->data);
		if (cache == NULL)
			goto bail;
	} else {
		cache = Py_None;
		Py_INCREF(cache);
	}

	tuple = Py_BuildValue("NN", idx, cache);
	if (!tuple)
		goto bail;
	return tuple;

bail:
	Py_XDECREF(idx);
	Py_XDECREF(cache);
	Py_XDECREF(tuple);
	return NULL;
}

static char parsers_doc[] = "Efficient content parsing.";

static PyMethodDef methods[] = {
	{"parse_manifest", parse_manifest, METH_VARARGS, "parse a manifest\n"},
	{"parse_dirstate", parse_dirstate, METH_VARARGS, "parse a dirstate\n"},
	{"parse_index2", parse_index2, METH_VARARGS, "parse a revlog index\n"},
	{NULL, NULL}
};

static void module_init(PyObject *mod)
{
	indexType.tp_new = PyType_GenericNew;
	if (PyType_Ready(&indexType) < 0)
		return;
	Py_INCREF(&indexType);

	PyModule_AddObject(mod, "index", (PyObject *)&indexType);

	nullentry = Py_BuildValue("iiiiiiis#", 0, 0, 0,
				  -1, -1, -1, -1, nullid, 20);
	if (nullentry)
		PyObject_GC_UnTrack(nullentry);
}

#ifdef IS_PY3K
static struct PyModuleDef parsers_module = {
	PyModuleDef_HEAD_INIT,
	"parsers",
	parsers_doc,
	-1,
	methods
};

PyMODINIT_FUNC PyInit_parsers(void)
{
	PyObject *mod = PyModule_Create(&parsers_module);
	module_init(mod);
	return mod;
}
#else
PyMODINIT_FUNC initparsers(void)
{
	PyObject *mod = Py_InitModule3("parsers", methods, parsers_doc);
	module_init(mod);
}
#endif
