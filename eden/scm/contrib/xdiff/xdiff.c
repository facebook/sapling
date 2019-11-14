#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "lib/third-party/xdiff/xdiff.h"

#define abort(...)                                                             \
	{                                                                      \
		fprintf(stderr, __VA_ARGS__);                                  \
		exit(-1);                                                      \
	}

void readfile(const char *path, mmfile_t *file)
{
	memset(file, 0, sizeof(*file));
	FILE *fp = fopen(path, "r");
	if (!fp) {
		abort("cannot open %s\n", path);
	}
	while (!feof(fp)) {
		char buf[40960];
		size_t size = fread(buf, 1, sizeof buf, fp);
		if (size > 0) {
			size_t new_size = file->size + size;
			file->ptr = realloc(file->ptr, new_size);
			if (!file->ptr) {
				abort("cannot allocate\n");
			}
			memcpy(file->ptr + file->size, buf, size);
			file->size = new_size;
		}
	}
	fclose(fp);
}

static int hunk_consumer(int64_t a1, int64_t a2, int64_t b1, int64_t b2,
                         void *priv)
{
	printf("@@ -%ld,%ld +%ld,%ld @@\n", a1, a2, b1, b2);
}

int main(int argc, char const *argv[])
{
	if (argc < 3) {
		abort("usage: %s FILE1 FILE2\n", argv[0]);
	}

	mmfile_t a, b;

	readfile(argv[1], &a);
	readfile(argv[2], &b);

	xpparam_t xpp = {
	    0, /* flags */
	};
	xdemitconf_t xecfg = {
	    0,             /* flags */
	    hunk_consumer, /* hunk_consume_func */
	};
	xdemitcb_t ecb = {
	    0, /* priv */
	};

	xdl_diff(&a, &b, &xpp, &xecfg, &ecb);

	free(a.ptr);
	free(b.ptr);

	return 0;
}
