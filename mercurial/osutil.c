/*
 osutil.c - native operating system services

 Copyright 2007 Matt Mackall and others

 This software may be used and distributed according to the terms of
 the GNU General Public License, incorporated herein by reference.
*/

#define _ATFILE_SOURCE
#include <Python.h>
#include <dirent.h>
#include <fcntl.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

struct listdir_stat {
	PyObject_HEAD
	struct stat st;
};

#define listdir_slot(name) \
    static PyObject *listdir_stat_##name(PyObject *self, void *x) \
    { \
        return PyInt_FromLong(((struct listdir_stat *)self)->st.name); \
    }

listdir_slot(st_dev)
listdir_slot(st_mode)
listdir_slot(st_nlink)
listdir_slot(st_size)
listdir_slot(st_mtime)
listdir_slot(st_ctime)

static struct PyGetSetDef listdir_stat_getsets[] = {
	{"st_dev", listdir_stat_st_dev, 0, 0, 0},
	{"st_mode", listdir_stat_st_mode, 0, 0, 0},
	{"st_nlink", listdir_stat_st_nlink, 0, 0, 0},
	{"st_size", listdir_stat_st_size, 0, 0, 0},
	{"st_mtime", listdir_stat_st_mtime, 0, 0, 0},
	{"st_ctime", listdir_stat_st_ctime, 0, 0, 0},
	{0, 0, 0, 0, 0}
};

static PyObject *listdir_stat_new(PyTypeObject *t, PyObject *a, PyObject *k)
{
	return t->tp_alloc(t, 0);
}

static void listdir_stat_dealloc(PyObject *o)
{
	o->ob_type->tp_free(o);
}

static PyTypeObject listdir_stat_type = {
	PyObject_HEAD_INIT(NULL)
	0,                         /*ob_size*/
	"osutil.stat",             /*tp_name*/
	sizeof(struct listdir_stat), /*tp_basicsize*/
	0,                         /*tp_itemsize*/
	(destructor)listdir_stat_dealloc, /*tp_dealloc*/
	0,                         /*tp_print*/
	0,                         /*tp_getattr*/
	0,                         /*tp_setattr*/
	0,                         /*tp_compare*/
	0,                         /*tp_repr*/
	0,                         /*tp_as_number*/
	0,                         /*tp_as_sequence*/
	0,                         /*tp_as_mapping*/
	0,                         /*tp_hash */
	0,                         /*tp_call*/
	0,                         /*tp_str*/
	0,                         /*tp_getattro*/
	0,                         /*tp_setattro*/
	0,                         /*tp_as_buffer*/
	Py_TPFLAGS_DEFAULT | Py_TPFLAGS_BASETYPE, /*tp_flags*/
	"stat objects",            /* tp_doc */
	0,                         /* tp_traverse */
	0,                         /* tp_clear */
	0,                         /* tp_richcompare */
	0,                         /* tp_weaklistoffset */
	0,                         /* tp_iter */
	0,                         /* tp_iternext */
	0,                         /* tp_methods */
	0,                         /* tp_members */
	listdir_stat_getsets,      /* tp_getset */
	0,                         /* tp_base */
	0,                         /* tp_dict */
	0,                         /* tp_descr_get */
	0,                         /* tp_descr_set */
	0,                         /* tp_dictoffset */
	0,                         /* tp_init */
	0,                         /* tp_alloc */
	listdir_stat_new,          /* tp_new */
};

static PyObject *listfiles(PyObject *list, DIR *dir,
			   int keep_stat, int *need_stat)
{
	struct dirent *ent;
	PyObject *name, *py_kind, *val;

#ifdef DT_REG
	*need_stat = 0;
#else
	*need_stat = 1;
#endif

	for (ent = readdir(dir); ent; ent = readdir(dir)) {
		int kind = -1;

		if (!strcmp(ent->d_name, ".") || !strcmp(ent->d_name, ".."))
			continue;

#ifdef DT_REG
		if (!keep_stat)
			switch (ent->d_type) {
			case DT_REG: kind = S_IFREG; break;
			case DT_DIR: kind = S_IFDIR; break;
			case DT_LNK: kind = S_IFLNK; break;
			case DT_BLK: kind = S_IFBLK; break;
			case DT_CHR: kind = S_IFCHR; break;
			case DT_FIFO: kind = S_IFIFO; break;
			case DT_SOCK: kind = S_IFSOCK; break;
			default:
				*need_stat = 0;
				break;
			}
#endif

		if (kind != -1)
			py_kind = PyInt_FromLong(kind);
		else {
			py_kind = Py_None;
			Py_INCREF(Py_None);
		}

		val = PyTuple_New(keep_stat ? 3 : 2);
		name = PyString_FromString(ent->d_name);

		if (!name || !py_kind || !val) {
			Py_XDECREF(name);
			Py_XDECREF(py_kind);
			Py_XDECREF(val);
			return PyErr_NoMemory();
		}

		PyTuple_SET_ITEM(val, 0, name);
		PyTuple_SET_ITEM(val, 1, py_kind);
		if (keep_stat) {
			PyTuple_SET_ITEM(val, 2, Py_None);
			Py_INCREF(Py_None);
		}

		PyList_Append(list, val);
		Py_DECREF(val);
	}

	return 0;
}

static PyObject *statfiles(PyObject *list, PyObject *ctor_args, int keep,
			   char *path, int len, int dfd)
{
	struct stat buf;
	struct stat *stp = &buf;
	int kind;
	int ret;
	ssize_t i;
	ssize_t size = PyList_Size(list);

	for (i = 0; i < size; i++) {
		PyObject *elt = PyList_GetItem(list, i);
		char *name = PyString_AsString(PyTuple_GET_ITEM(elt, 0));
		PyObject *py_st = NULL;
		PyObject *py_kind = PyTuple_GET_ITEM(elt, 1);

		kind = py_kind == Py_None ? -1 : PyInt_AsLong(py_kind);
		if (kind != -1 && !keep)
			continue;

		strncpy(path + len + 1, name, PATH_MAX - len);
		path[PATH_MAX] = 0;

		if (keep) {
			py_st = PyObject_CallObject(
				(PyObject *)&listdir_stat_type, ctor_args);
			if (!py_st)
				return PyErr_NoMemory();
			stp = &((struct listdir_stat *)py_st)->st;
			PyTuple_SET_ITEM(elt, 2, py_st);
		}

#ifdef AT_SYMLINK_NOFOLLOW
		ret = fstatat(dfd, name, stp, AT_SYMLINK_NOFOLLOW);
#else
		ret = lstat(path, stp);
#endif
		if (ret == -1)
			return PyErr_SetFromErrnoWithFilename(PyExc_OSError,
							      path);

		if (kind == -1) {
			if (S_ISREG(stp->st_mode))
				kind = S_IFREG;
			else if (S_ISDIR(stp->st_mode))
				kind = S_IFDIR;
			else if (S_ISLNK(stp->st_mode))
				kind = S_IFLNK;
			else if (S_ISBLK(stp->st_mode))
				kind = S_IFBLK;
			else if (S_ISCHR(stp->st_mode))
				kind = S_IFCHR;
			else if (S_ISFIFO(stp->st_mode))
				kind = S_IFIFO;
			else if (S_ISSOCK(stp->st_mode))
				kind = S_IFSOCK;
			else
				kind = stp->st_mode;
		}

		if (py_kind == Py_None && kind != -1) {
			py_kind = PyInt_FromLong(kind);
			if (!py_kind)
				return PyErr_NoMemory();
			Py_XDECREF(Py_None);
			PyTuple_SET_ITEM(elt, 1, py_kind);
		}
	}

	return 0;
}

static PyObject *listdir(PyObject *self, PyObject *args, PyObject *kwargs)
{
	static char *kwlist[] = { "path", "stat", NULL };
	DIR *dir = NULL;
	PyObject *statobj = NULL;
	PyObject *list = NULL;
	PyObject *err = NULL;
	PyObject *ctor_args = NULL;
	char *path;
	char full_path[PATH_MAX + 10];
	int path_len;
	int need_stat, keep_stat;
	int dfd;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "s#|O:listdir", kwlist,
					 &path, &path_len, &statobj))
		goto bail;

	keep_stat = statobj && PyObject_IsTrue(statobj);

#ifdef AT_SYMLINK_NOFOLLOW
	dfd = open(path, O_RDONLY);
	dir = fdopendir(dfd);
#else
	dir = opendir(path);
	dfd = -1;
#endif
	if (!dir) {
		err = PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
		goto bail;
	}

	list = PyList_New(0);
	ctor_args = PyTuple_New(0);
	if (!list || !ctor_args)
		goto bail;

	strncpy(full_path, path, PATH_MAX);
	full_path[path_len] = '/';

	err = listfiles(list, dir, keep_stat, &need_stat);
	if (err)
		goto bail;

	PyList_Sort(list);

	if (!keep_stat && !need_stat)
		goto done;

	err = statfiles(list, ctor_args, keep_stat, full_path, path_len, dfd);
	if (!err)
		goto done;

 bail:
	Py_XDECREF(list);

 done:
	Py_XDECREF(ctor_args);
	if (dir)
		closedir(dir);
	return err ? err : list;
}


static char osutil_doc[] = "Native operating system services.";

static PyMethodDef methods[] = {
	{"listdir", (PyCFunction)listdir, METH_VARARGS | METH_KEYWORDS,
	 "list a directory\n"},
	{NULL, NULL}
};

PyMODINIT_FUNC initosutil(void)
{
	if (PyType_Ready(&listdir_stat_type) == -1)
		return;

	Py_InitModule3("osutil", methods, osutil_doc);
}
