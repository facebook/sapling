#ifndef PORTABILITY_PORTABILITY_H
#define PORTABILITY_PORTABILITY_H

#if defined(_MSC_VER)
	/* MSVC2015 supports compound literals in C mode (/TC)
	   but does not support them in C++ mode (/TP) */
	#if defined(__cplusplus)
		#define COMPOUND_LITERAL(typename_) typename_
	#else /* #if defined(__cplusplus) */
		#define COMPOUND_LITERAL(typename_) (typename_)
	#endif /* #if defined(__cplusplus) */
#else /* #if defined(_MSC_VER) */
	#define COMPOUND_LITERAL(typename_) (typename_)
#endif /* #if defined(_MSC_VER) */

#if defined(_MSC_VER)
	#define PACKEDSTRUCT(__Declaration__) __pragma(pack(push, 1)) \
	                                      __Declaration__ __pragma(pack(pop))
#else
	#define PACKEDSTRUCT(__Declaration__) __Declaration__ __attribute__((packed))
#endif

#endif /* #ifndef PORTABILITY_PORTABILITY_H */
