/**
* Copyright (c) 2016-present, Gregory Szorc
* All rights reserved.
*
* This software may be modified and distributed under the terms
* of the BSD license. See the LICENSE file for details.
*/

#include "python-zstandard.h"

extern PyObject* ZstdError;

PyDoc_STRVAR(ZstdDecompressionWriter__doc,
"""A context manager used for writing decompressed output.\n"
);

static void ZstdDecompressionWriter_dealloc(ZstdDecompressionWriter* self) {
	Py_XDECREF(self->decompressor);
	Py_XDECREF(self->writer);

	PyObject_Del(self);
}

static PyObject* ZstdDecompressionWriter_enter(ZstdDecompressionWriter* self) {
	if (self->entered) {
		PyErr_SetString(ZstdError, "cannot __enter__ multiple times");
		return NULL;
	}

	if (0 != init_dstream(self->decompressor)) {
		return NULL;
	}

	self->entered = 1;

	Py_INCREF(self);
	return (PyObject*)self;
}

static PyObject* ZstdDecompressionWriter_exit(ZstdDecompressionWriter* self, PyObject* args) {
	self->entered = 0;

	Py_RETURN_FALSE;
}

static PyObject* ZstdDecompressionWriter_memory_size(ZstdDecompressionWriter* self) {
	if (!self->decompressor->dstream) {
		PyErr_SetString(ZstdError, "cannot determine size of inactive decompressor; "
			"call when context manager is active");
		return NULL;
	}

	return PyLong_FromSize_t(ZSTD_sizeof_DStream(self->decompressor->dstream));
}

static PyObject* ZstdDecompressionWriter_write(ZstdDecompressionWriter* self, PyObject* args) {
	const char* source;
	Py_ssize_t sourceSize;
	size_t zresult = 0;
	ZSTD_inBuffer input;
	ZSTD_outBuffer output;
	PyObject* res;
	Py_ssize_t totalWrite = 0;

#if PY_MAJOR_VERSION >= 3
	if (!PyArg_ParseTuple(args, "y#:write", &source, &sourceSize)) {
#else
	if (!PyArg_ParseTuple(args, "s#:write", &source, &sourceSize)) {
#endif
		return NULL;
	}

	if (!self->entered) {
		PyErr_SetString(ZstdError, "write must be called from an active context manager");
		return NULL;
	}

	assert(self->decompressor->dstream);

	output.dst = PyMem_Malloc(self->outSize);
	if (!output.dst) {
		return PyErr_NoMemory();
	}
	output.size = self->outSize;
	output.pos = 0;

	input.src = source;
	input.size = sourceSize;
	input.pos = 0;

	while ((ssize_t)input.pos < sourceSize) {
		Py_BEGIN_ALLOW_THREADS
		zresult = ZSTD_decompressStream(self->decompressor->dstream, &output, &input);
		Py_END_ALLOW_THREADS

		if (ZSTD_isError(zresult)) {
			PyMem_Free(output.dst);
			PyErr_Format(ZstdError, "zstd decompress error: %s",
				ZSTD_getErrorName(zresult));
			return NULL;
		}

		if (output.pos) {
#if PY_MAJOR_VERSION >= 3
			res = PyObject_CallMethod(self->writer, "write", "y#",
#else
			res = PyObject_CallMethod(self->writer, "write", "s#",
#endif
				output.dst, output.pos);
			Py_XDECREF(res);
			totalWrite += output.pos;
			output.pos = 0;
		}
	}

	PyMem_Free(output.dst);

	return PyLong_FromSsize_t(totalWrite);
}

static PyMethodDef ZstdDecompressionWriter_methods[] = {
	{ "__enter__", (PyCFunction)ZstdDecompressionWriter_enter, METH_NOARGS,
	PyDoc_STR("Enter a decompression context.") },
	{ "__exit__", (PyCFunction)ZstdDecompressionWriter_exit, METH_VARARGS,
	PyDoc_STR("Exit a decompression context.") },
	{ "memory_size", (PyCFunction)ZstdDecompressionWriter_memory_size, METH_NOARGS,
	PyDoc_STR("Obtain the memory size in bytes of the underlying decompressor.") },
	{ "write", (PyCFunction)ZstdDecompressionWriter_write, METH_VARARGS,
	PyDoc_STR("Compress data") },
	{ NULL, NULL }
};

PyTypeObject ZstdDecompressionWriterType = {
	PyVarObject_HEAD_INIT(NULL, 0)
	"zstd.ZstdDecompressionWriter", /* tp_name */
	sizeof(ZstdDecompressionWriter),/* tp_basicsize */
	0,                              /* tp_itemsize */
	(destructor)ZstdDecompressionWriter_dealloc, /* tp_dealloc */
	0,                              /* tp_print */
	0,                              /* tp_getattr */
	0,                              /* tp_setattr */
	0,                              /* tp_compare */
	0,                              /* tp_repr */
	0,                              /* tp_as_number */
	0,                              /* tp_as_sequence */
	0,                              /* tp_as_mapping */
	0,                              /* tp_hash */
	0,                              /* tp_call */
	0,                              /* tp_str */
	0,                              /* tp_getattro */
	0,                              /* tp_setattro */
	0,                              /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT | Py_TPFLAGS_BASETYPE, /* tp_flags */
	ZstdDecompressionWriter__doc,   /* tp_doc */
	0,                              /* tp_traverse */
	0,                              /* tp_clear */
	0,                              /* tp_richcompare */
	0,                              /* tp_weaklistoffset */
	0,                              /* tp_iter */
	0,                              /* tp_iternext */
	ZstdDecompressionWriter_methods,/* tp_methods */
	0,                              /* tp_members */
	0,                              /* tp_getset */
	0,                              /* tp_base */
	0,                              /* tp_dict */
	0,                              /* tp_descr_get */
	0,                              /* tp_descr_set */
	0,                              /* tp_dictoffset */
	0,                              /* tp_init */
	0,                              /* tp_alloc */
	PyType_GenericNew,              /* tp_new */
};

void decompressionwriter_module_init(PyObject* mod) {
	Py_TYPE(&ZstdDecompressionWriterType) = &PyType_Type;
	if (PyType_Ready(&ZstdDecompressionWriterType) < 0) {
		return;
	}
}
