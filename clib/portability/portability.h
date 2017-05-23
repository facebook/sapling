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

#endif /* #ifndef PORTABILITY_PORTABILITY_H */
