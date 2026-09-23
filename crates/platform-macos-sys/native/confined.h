#ifndef SEMWRIGHT_CONFINED_H
#define SEMWRIGHT_CONFINED_H
#include <stddef.h>
/* Return: >=0 descriptor/success; negative stable error classes (never errno text). */
enum { SW_DENIED=-1, SW_INVALID=-2, SW_NOT_FOUND=-3, SW_BUDGET=-4, SW_IO=-5, SW_UNSUPPORTED=-6, SW_UNCERTAIN=-7 };
int sw_root_open(const char *absolute);
int sw_child_read_open(int root, const char *child, size_t limit);
int sw_child_write_atomic(int root, const char *child, const char *temporary, const unsigned char *data, size_t size);
#endif
