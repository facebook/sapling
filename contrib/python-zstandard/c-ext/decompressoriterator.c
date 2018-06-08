/**
* Copyright (c) 2016-present, Gregory Szorc
* All rights reserved.
*
* This software may be modified and distributed under the terms
* of the BSD license. See the LICENSE file for details.
*/

#include "python-zstandard.h"

#define min(a, b) (((a) < (b)) ? (a) : (b))

extern PyObject* ZstdError;

PyDoc_STRVAR(ZstdDecompressorIterator__doc__,
"Represents an iterator of decompressed data.\n"
);

static void ZstdDecompressorIterator_dealloc(ZstdDecompressorIterator* self) {
	Py_XDECREF(self->decompressor);
	Py_XDECREF(self->reader);

	if (self->buffer) {
		PyBuffer_Release(self->buffer);
		PyMem_FREE(self->buffer);
		self->buffer = NULL;
	}

	if (self->input.src) {
		PyMem_Free((void*)self->input.src);
		self->input.src = NULL;
	}

	PyObject_Del(self);
}

static PyObject* ZstdDecompressorIterator_iter(PyObject* self) {
	Py_INCREF(self);
	return self;
}

static DecompressorIteratorResult read_decompressor_iterator(ZstdDecompressorIterator* self) {
	size_t zresult;
	PyObject* chunk;
	DecompressorIteratorResult result;
	size_t oldInputPos = self->input.pos;

	assert(self->decompressor->dstream);

	result.chunk = NULL;

	chunk = PyBytes_FromStringAndSize(NULL, self->outSize);
	if (!chunk) {
		result.errored = 1;
		return result;
	}

	self->output.dst = PyBytes_AsString(chunk);
	self->output.size = self->outSize;
	self->output.pos = 0;

	Py_BEGIN_ALLOW_THREADS
	zresult = ZSTD_decompressStream(self->decompressor->dstream, &self->output, &self->input);
	Py_END_ALLOW_THREADS

	/* We're done with the pointer. Nullify to prevent anyone from getting a
	handle on a Python object. */
	self->output.dst = NULL;

	if (ZSTD_isError(zresult)) {
		Py_DECREF(chunk);
		PyErr_Format(ZstdError, "zstd decompress error: %s",
			ZSTD_getErrorName(zresult));
		result.errored = 1;
		return result;
	}

	self->readCount += self->input.pos - oldInputPos;

	/* Frame is fully decoded. Input exhausted and output sitting in buffer. */
	if (0 == zresult) {
		self->finishedInput = 1;
		self->finishedOutput = 1;
	}

	/* If it produced output data, return it. */
	if (self->output.pos) {
		if (self->output.pos < self->outSize) {
			if (_PyBytes_Resize(&chunk, self->output.pos)) {
				result.errored = 1;
				return result;
			}
		}
	}
	else {
		Py_DECREF(chunk);
		chunk = NULL;
	}

	result.errored = 0;
	result.chunk = chunk;

	return result;
}

static PyObject* ZstdDecompressorIterator_iternext(ZstdDecompressorIterator* self) {
	PyObject* readResult = NULL;
	char* readBuffer;
	Py_ssize_t readSize;
	Py_ssize_t bufferRemaining;
	DecompressorIteratorResult result;

	if (self->finishedOutput) {
		PyErr_SetString(PyExc_StopIteration, "output flushed");
		return NULL;
	}

	/* If we have data left in the input, consume it. */
	if (self->input.pos < self->input.size) {
		result = read_decompressor_iterator(self);
		if (result.chunk || result.errored) {
			return result.chunk;
		}

		/* Else fall through to get more data from input. */
	}

read_from_source:

	if (!self->finishedInput) {
		if (self->reader) {
			readResult = PyObject_CallMethod(self->reader, "read", "I", self->inSize);
			if (!readResult) {
				return NULL;
			}

			PyBytes_AsStringAndSize(readResult, &readBuffer, &readSize);
		}
		else {
			assert(self->buffer && self->buffer->buf);

			/* Only support contiguous C arrays for now */
			assert(self->buffer->strides == NULL && self->buffer->suboffsets == NULL);
			assert(self->buffer->itemsize == 1);

			/* TODO avoid memcpy() below */
			readBuffer = (char *)self->buffer->buf + self->bufferOffset;
			bufferRemaining = self->buffer->len - self->bufferOffset;
			readSize = min(bufferRemaining, (Py_ssize_t)self->inSize);
			self->bufferOffset += readSize;
		}

		if (readSize) {
			if (!self->readCount && self->skipBytes) {
				assert(self->skipBytes < self->inSize);
				if ((Py_ssize_t)self->skipBytes >= readSize) {
					PyErr_SetString(PyExc_ValueError,
						"skip_bytes larger than first input chunk; "
						"this scenario is currently unsupported");
					Py_XDECREF(readResult);
					return NULL;
				}

				readBuffer = readBuffer + self->skipBytes;
				readSize -= self->skipBytes;
			}

			/* Copy input into previously allocated buffer because it can live longer
			than a single function call and we don't want to keep a ref to a Python
			object around. This could be changed... */
			memcpy((void*)self->input.src, readBuffer, readSize);
			self->input.size = readSize;
			self->input.pos = 0;
		}
		/* No bytes on first read must mean an empty input stream. */
		else if (!self->readCount) {
			self->finishedInput = 1;
			self->finishedOutput = 1;
			Py_XDECREF(readResult);
			PyErr_SetString(PyExc_StopIteration, "empty input");
			return NULL;
		}
		else {
			self->finishedInput = 1;
		}

		/* We've copied the data managed by memory. Discard the Python object. */
		Py_XDECREF(readResult);
	}

	result = read_decompressor_iterator(self);
	if (result.errored || result.chunk) {
		return result.chunk;
	}

	/* No new output data. Try again unless we know there is no more data. */
	if (!self->finishedInput) {
		goto read_from_source;
	}

	PyErr_SetString(PyExc_StopIteration, "input exhausted");
	return NULL;
}

PyTypeObject ZstdDecompressorIteratorType = {
	PyVarObject_HEAD_INIT(NULL, 0)
	"zstd.ZstdDecompressorIterator",   /* tp_name */
	sizeof(ZstdDecompressorIterator),  /* tp_basicsize */
	0,                                 /* tp_itemsize */
	(destructor)ZstdDecompressorIterator_dealloc, /* tp_dealloc */
	0,                                 /* tp_print */
	0,                                 /* tp_getattr */
	0,                                 /* tp_setattr */
	0,                                 /* tp_compare */
	0,                                 /* tp_repr */
	0,                                 /* tp_as_number */
	0,                                 /* tp_as_sequence */
	0,                                 /* tp_as_mapping */
	0,                                 /* tp_hash */
	0,                                 /* tp_call */
	0,                                 /* tp_str */
	0,                                 /* tp_getattro */
	0,                                 /* tp_setattro */
	0,                                 /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT | Py_TPFLAGS_BASETYPE, /* tp_flags */
	ZstdDecompressorIterator__doc__,   /* tp_doc */
	0,                                 /* tp_traverse */
	0,                                 /* tp_clear */
	0,                                 /* tp_richcompare */
	0,                                 /* tp_weaklistoffset */
	ZstdDecompressorIterator_iter,     /* tp_iter */
	(iternextfunc)ZstdDecompressorIterator_iternext, /* tp_iternext */
	0,                                 /* tp_methods */
	0,                                 /* tp_members */
	0,                                 /* tp_getset */
	0,                                 /* tp_base */
	0,                                 /* tp_dict */
	0,                                 /* tp_descr_get */
	0,                                 /* tp_descr_set */
	0,                                 /* tp_dictoffset */
	0,                                 /* tp_init */
	0,                                 /* tp_alloc */
	PyType_GenericNew,                 /* tp_new */
};

void decompressoriterator_module_init(PyObject* mod) {
	Py_TYPE(&ZstdDecompressorIteratorType) = &PyType_Type;
	if (PyType_Ready(&ZstdDecompressorIteratorType) < 0) {
		return;
	}
}
