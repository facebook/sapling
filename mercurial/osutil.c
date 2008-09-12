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

#ifdef AT_SYMLINK_NOFOLLOW
#define USEFDOPEN 1
#else
#define USEFDOPEN 0
#endif

int entkind(struct dirent *ent)
{
#ifdef DT_REG
	switch (ent->d_type) {
	case DT_REG: return S_IFREG;
	case DT_DIR: return S_IFDIR;
	case DT_LNK: return S_IFLNK;
	case DT_BLK: return S_IFBLK;
	case DT_CHR: return S_IFCHR;
	case DT_FIFO: return S_IFIFO;
	case DT_SOCK: return S_IFSOCK;
	}
#endif
}

static PyObject *listdir(PyObject *self, PyObject *args, PyObject *kwargs)
{
	static char *kwlist[] = { "path", "stat", NULL };
	PyObject *statflag = NULL, *list, *elem, *stat, *ret = NULL;
	char fullpath[PATH_MAX + 10], *path;
	int pathlen, keepstat, kind, dfd, err;
	struct stat st;
	struct dirent *ent;
	DIR *dir;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "s#|O:listdir", kwlist,
					 &path, &pathlen, &statflag))
		goto error_parse;

	strncpy(fullpath, path, PATH_MAX);
	fullpath[pathlen] = '/';
	keepstat = statflag && PyObject_IsTrue(statflag);

	if (USEFDOPEN) {
		dfd = open(path, O_RDONLY);
		if (dfd == -1) {
			PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
			goto error_parse;
		}
		dir = fdopendir(dfd);
	} else
		dir = opendir(path);
	if (!dir) {
		PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
		goto error_dir;
 	}

	list = PyList_New(0);
	if (!list)
		goto error_list;

	while ((ent = readdir(dir))) {
		if (!strcmp(ent->d_name, ".") || !strcmp(ent->d_name, ".."))
			continue;

		kind = entkind(ent);
		if (kind == -1 || keepstat) {
			if (USEFDOPEN)
				err = fstatat(dfd, ent->d_name, &st,
					      AT_SYMLINK_NOFOLLOW);
			else {
				strncpy(fullpath + pathlen + 1, ent->d_name,
					PATH_MAX - pathlen);
				fullpath[PATH_MAX] = 0;
				err = lstat(fullpath, &st);
			}
			if (err == -1) {
				strncpy(fullpath + pathlen + 1, ent->d_name,
					PATH_MAX - pathlen);
				fullpath[PATH_MAX] = 0;
				PyErr_SetFromErrnoWithFilename(PyExc_OSError,
							       fullpath);
				goto error;
			}
			kind = st.st_mode & S_IFMT;
		}

		if (keepstat) {
			stat = PyObject_CallObject((PyObject *)&listdir_stat_type, NULL);
			if (!stat)
				goto error;
			memcpy(&((struct listdir_stat *)stat)->st, &st, sizeof(st));
			elem = Py_BuildValue("siN", ent->d_name, kind, stat);
		} else
			elem = Py_BuildValue("si", ent->d_name, kind);
		if (!elem)
			goto error;

		PyList_Append(list, elem);
		Py_DECREF(elem);
	}

	PyList_Sort(list);
	ret = list;
	Py_INCREF(ret);

error:
	Py_DECREF(list);
error_list:
	closedir(dir);
error_dir:
	if (USEFDOPEN)
 		close(dfd);
error_parse:
	return ret;
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
