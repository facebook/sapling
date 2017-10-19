#ifndef _HG_BITMANIPULATION_H_
#define _HG_BITMANIPULATION_H_

#include <string.h>

#include "compat.h"

static inline uint32_t getbe32(const char *c)
{
	const unsigned char *d = (const unsigned char *)c;

	return ((d[0] << 24) | (d[1] << 16) | (d[2] << 8) | (d[3]));
}

static inline int16_t getbeint16(const char *c)
{
	const unsigned char *d = (const unsigned char *)c;

	return ((d[0] << 8) | (d[1]));
}

static inline uint16_t getbeuint16(const char *c)
{
	const unsigned char *d = (const unsigned char *)c;

	return ((d[0] << 8) | (d[1]));
}

static inline void putbe32(uint32_t x, char *c)
{
	c[0] = (x >> 24) & 0xff;
	c[1] = (x >> 16) & 0xff;
	c[2] = (x >> 8) & 0xff;
	c[3] = (x)&0xff;
}

static inline double getbefloat64(const char *c)
{
	const unsigned char *d = (const unsigned char *)c;
	double ret;
	int i;
	uint64_t t = 0;
	for (i = 0; i < 8; i++) {
		t = (t << 8) + d[i];
	}
	memcpy(&ret, &t, sizeof(t));
	return ret;
}

#endif
