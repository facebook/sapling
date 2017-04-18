/**
* Copyright (c) 2016-present, Gregory Szorc
* All rights reserved.
*
* This software may be modified and distributed under the terms
* of the BSD license. See the LICENSE file for details.
*/

#include "python-zstandard.h"

extern PyObject* ZstdError;

ZstdCompressionDict* train_dictionary(PyObject* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"dict_size",
		"samples",
		"selectivity",
		"level",
		"notifications",
		"dict_id",
		NULL
	};
	size_t capacity;
	PyObject* samples;
	Py_ssize_t samplesLen;
	unsigned  selectivity = 0;
	int level = 0;
	unsigned notifications = 0;
	unsigned dictID = 0;
	ZDICT_params_t zparams;
	Py_ssize_t sampleIndex;
	Py_ssize_t sampleSize;
	PyObject* sampleItem;
	size_t zresult;
	void* sampleBuffer = NULL;
	void* sampleOffset;
	size_t samplesSize = 0;
	size_t* sampleSizes = NULL;
	void* dict = NULL;
	ZstdCompressionDict* result = NULL;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "nO!|IiII:train_dictionary",
		kwlist,
		&capacity,
		&PyList_Type, &samples,
		&selectivity, &level, &notifications, &dictID)) {
		return NULL;
	}

	memset(&zparams, 0, sizeof(zparams));

	zparams.selectivityLevel = selectivity;
	zparams.compressionLevel = level;
	zparams.notificationLevel = notifications;
	zparams.dictID = dictID;

	/* Figure out the size of the raw samples */
	samplesLen = PyList_Size(samples);
	for (sampleIndex = 0; sampleIndex < samplesLen; sampleIndex++) {
		sampleItem = PyList_GetItem(samples, sampleIndex);
		if (!PyBytes_Check(sampleItem)) {
			PyErr_SetString(PyExc_ValueError, "samples must be bytes");
			return NULL;
		}
		samplesSize += PyBytes_GET_SIZE(sampleItem);
	}

	/* Now that we know the total size of the raw simples, we can allocate
	a buffer for the raw data */
	sampleBuffer = PyMem_Malloc(samplesSize);
	if (!sampleBuffer) {
		PyErr_NoMemory();
		goto finally;
	}
	sampleSizes = PyMem_Malloc(samplesLen * sizeof(size_t));
	if (!sampleSizes) {
		PyErr_NoMemory();
		goto finally;
	}

	sampleOffset = sampleBuffer;
	/* Now iterate again and assemble the samples in the buffer */
	for (sampleIndex = 0; sampleIndex < samplesLen; sampleIndex++) {
		sampleItem = PyList_GetItem(samples, sampleIndex);
		sampleSize = PyBytes_GET_SIZE(sampleItem);
		sampleSizes[sampleIndex] = sampleSize;
		memcpy(sampleOffset, PyBytes_AS_STRING(sampleItem), sampleSize);
		sampleOffset = (char*)sampleOffset + sampleSize;
	}

	dict = PyMem_Malloc(capacity);
	if (!dict) {
		PyErr_NoMemory();
		goto finally;
	}

	/* TODO consider using dup2() to redirect zstd's stderr writing to a buffer */
	Py_BEGIN_ALLOW_THREADS
	zresult = ZDICT_trainFromBuffer_advanced(dict, capacity,
		sampleBuffer, sampleSizes, (unsigned int)samplesLen,
		zparams);
	Py_END_ALLOW_THREADS
	if (ZDICT_isError(zresult)) {
		PyErr_Format(ZstdError, "Cannot train dict: %s", ZDICT_getErrorName(zresult));
		PyMem_Free(dict);
		goto finally;
	}

	result = PyObject_New(ZstdCompressionDict, &ZstdCompressionDictType);
	if (!result) {
		goto finally;
	}

	result->dictData = dict;
	result->dictSize = zresult;
	result->d = 0;
	result->k = 0;

finally:
	PyMem_Free(sampleBuffer);
	PyMem_Free(sampleSizes);

	return result;
}

ZstdCompressionDict* train_cover_dictionary(PyObject* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"dict_size",
		"samples",
		"k",
		"d",
		"notifications",
		"dict_id",
		"level",
		"optimize",
		"steps",
		"threads",
		NULL
	};

	size_t capacity;
	PyObject* samples;
	unsigned k = 0;
	unsigned d = 0;
	unsigned notifications = 0;
	unsigned dictID = 0;
	int level = 0;
	PyObject* optimize = NULL;
	unsigned steps = 0;
	int threads = 0;
	COVER_params_t params;
	Py_ssize_t samplesLen;
	Py_ssize_t i;
	size_t samplesSize = 0;
	void* sampleBuffer = NULL;
	size_t* sampleSizes = NULL;
	void* sampleOffset;
	Py_ssize_t sampleSize;
	void* dict = NULL;
	size_t zresult;
	ZstdCompressionDict* result = NULL;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "nO!|IIIIiOIi:train_cover_dictionary",
		kwlist, &capacity, &PyList_Type, &samples,
		&k, &d, &notifications, &dictID, &level, &optimize, &steps, &threads)) {
		return NULL;
	}

	if (threads < 0) {
		threads = cpu_count();
	}

	memset(&params, 0, sizeof(params));
	params.k = k;
	params.d = d;
	params.steps = steps;
	params.nbThreads = threads;
	params.notificationLevel = notifications;
	params.dictID = dictID;
	params.compressionLevel = level;

	/* Figure out total size of input samples. */
	samplesLen = PyList_Size(samples);
	for (i = 0; i < samplesLen; i++) {
		PyObject* sampleItem = PyList_GET_ITEM(samples, i);

		if (!PyBytes_Check(sampleItem)) {
			PyErr_SetString(PyExc_ValueError, "samples must be bytes");
			return NULL;
		}
		samplesSize += PyBytes_GET_SIZE(sampleItem);
	}

	sampleBuffer = PyMem_Malloc(samplesSize);
	if (!sampleBuffer) {
		PyErr_NoMemory();
		goto finally;
	}

	sampleSizes = PyMem_Malloc(samplesLen * sizeof(size_t));
	if (!sampleSizes) {
		PyErr_NoMemory();
		goto finally;
	}

	sampleOffset = sampleBuffer;
	for (i = 0; i < samplesLen; i++) {
		PyObject* sampleItem = PyList_GET_ITEM(samples, i);
		sampleSize = PyBytes_GET_SIZE(sampleItem);
		sampleSizes[i] = sampleSize;
		memcpy(sampleOffset, PyBytes_AS_STRING(sampleItem), sampleSize);
		sampleOffset = (char*)sampleOffset + sampleSize;
	}

	dict = PyMem_Malloc(capacity);
	if (!dict) {
		PyErr_NoMemory();
		goto finally;
	}

	Py_BEGIN_ALLOW_THREADS
	if (optimize && PyObject_IsTrue(optimize)) {
		zresult = COVER_optimizeTrainFromBuffer(dict, capacity,
			sampleBuffer, sampleSizes, (unsigned)samplesLen, &params);
	}
	else {
		zresult = COVER_trainFromBuffer(dict, capacity,
			sampleBuffer, sampleSizes, (unsigned)samplesLen, params);
	}
	Py_END_ALLOW_THREADS

	if (ZDICT_isError(zresult)) {
		PyMem_Free(dict);
		PyErr_Format(ZstdError, "cannot train dict: %s", ZDICT_getErrorName(zresult));
		goto finally;
	}

	result = PyObject_New(ZstdCompressionDict, &ZstdCompressionDictType);
	if (!result) {
		PyMem_Free(dict);
		goto finally;
	}

	result->dictData = dict;
	result->dictSize = zresult;
	result->d = params.d;
	result->k = params.k;

finally:
	PyMem_Free(sampleBuffer);
	PyMem_Free(sampleSizes);

	return result;
}

PyDoc_STRVAR(ZstdCompressionDict__doc__,
"ZstdCompressionDict(data) - Represents a computed compression dictionary\n"
"\n"
"This type holds the results of a computed Zstandard compression dictionary.\n"
"Instances are obtained by calling ``train_dictionary()`` or by passing bytes\n"
"obtained from another source into the constructor.\n"
);

static int ZstdCompressionDict_init(ZstdCompressionDict* self, PyObject* args) {
	const char* source;
	Py_ssize_t sourceSize;

	self->dictData = NULL;
	self->dictSize = 0;

#if PY_MAJOR_VERSION >= 3
	if (!PyArg_ParseTuple(args, "y#:ZstdCompressionDict",
#else
	if (!PyArg_ParseTuple(args, "s#:ZstdCompressionDict",
#endif
		&source, &sourceSize)) {
		return -1;
	}

	self->dictData = PyMem_Malloc(sourceSize);
	if (!self->dictData) {
		PyErr_NoMemory();
		return -1;
	}

	memcpy(self->dictData, source, sourceSize);
	self->dictSize = sourceSize;

	return 0;
	}

static void ZstdCompressionDict_dealloc(ZstdCompressionDict* self) {
	if (self->dictData) {
		PyMem_Free(self->dictData);
		self->dictData = NULL;
	}

	PyObject_Del(self);
}

static PyObject* ZstdCompressionDict_dict_id(ZstdCompressionDict* self) {
	unsigned dictID = ZDICT_getDictID(self->dictData, self->dictSize);

	return PyLong_FromLong(dictID);
}

static PyObject* ZstdCompressionDict_as_bytes(ZstdCompressionDict* self) {
	return PyBytes_FromStringAndSize(self->dictData, self->dictSize);
}

static PyMethodDef ZstdCompressionDict_methods[] = {
	{ "dict_id", (PyCFunction)ZstdCompressionDict_dict_id, METH_NOARGS,
	PyDoc_STR("dict_id() -- obtain the numeric dictionary ID") },
	{ "as_bytes", (PyCFunction)ZstdCompressionDict_as_bytes, METH_NOARGS,
	PyDoc_STR("as_bytes() -- obtain the raw bytes constituting the dictionary data") },
	{ NULL, NULL }
};

static PyMemberDef ZstdCompressionDict_members[] = {
	{ "k", T_UINT, offsetof(ZstdCompressionDict, k), READONLY,
	  "segment size" },
	{ "d", T_UINT, offsetof(ZstdCompressionDict, d), READONLY,
	  "dmer size" },
	{ NULL }
};

static Py_ssize_t ZstdCompressionDict_length(ZstdCompressionDict* self) {
	return self->dictSize;
}

static PySequenceMethods ZstdCompressionDict_sq = {
	(lenfunc)ZstdCompressionDict_length, /* sq_length */
	0,                                   /* sq_concat */
	0,                                   /* sq_repeat */
	0,                                   /* sq_item */
	0,                                   /* sq_ass_item */
	0,                                   /* sq_contains */
	0,                                   /* sq_inplace_concat */
	0                                    /* sq_inplace_repeat */
};

PyTypeObject ZstdCompressionDictType = {
	PyVarObject_HEAD_INIT(NULL, 0)
	"zstd.ZstdCompressionDict",     /* tp_name */
	sizeof(ZstdCompressionDict),    /* tp_basicsize */
	0,                              /* tp_itemsize */
	(destructor)ZstdCompressionDict_dealloc, /* tp_dealloc */
	0,                              /* tp_print */
	0,                              /* tp_getattr */
	0,                              /* tp_setattr */
	0,                              /* tp_compare */
	0,                              /* tp_repr */
	0,                              /* tp_as_number */
	&ZstdCompressionDict_sq,        /* tp_as_sequence */
	0,                              /* tp_as_mapping */
	0,                              /* tp_hash */
	0,                              /* tp_call */
	0,                              /* tp_str */
	0,                              /* tp_getattro */
	0,                              /* tp_setattro */
	0,                              /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT | Py_TPFLAGS_BASETYPE, /* tp_flags */
	ZstdCompressionDict__doc__,     /* tp_doc */
	0,                              /* tp_traverse */
	0,                              /* tp_clear */
	0,                              /* tp_richcompare */
	0,                              /* tp_weaklistoffset */
	0,                              /* tp_iter */
	0,                              /* tp_iternext */
	ZstdCompressionDict_methods,    /* tp_methods */
	ZstdCompressionDict_members,    /* tp_members */
	0,                              /* tp_getset */
	0,                              /* tp_base */
	0,                              /* tp_dict */
	0,                              /* tp_descr_get */
	0,                              /* tp_descr_set */
	0,                              /* tp_dictoffset */
	(initproc)ZstdCompressionDict_init, /* tp_init */
	0,                              /* tp_alloc */
	PyType_GenericNew,              /* tp_new */
};

void compressiondict_module_init(PyObject* mod) {
	Py_TYPE(&ZstdCompressionDictType) = &PyType_Type;
	if (PyType_Ready(&ZstdCompressionDictType) < 0) {
		return;
	}

	Py_INCREF((PyObject*)&ZstdCompressionDictType);
	PyModule_AddObject(mod, "ZstdCompressionDict",
		(PyObject*)&ZstdCompressionDictType);
}
