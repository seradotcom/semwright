#ifndef SEMWRIGHT_NATIVE_ABI_H
#define SEMWRIGHT_NATIVE_ABI_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
/* In-process ABI only. All input pointers are borrowed for the duration of a call.
   call() returns malloc-owned bytes, released once with native_free(). */
int32_t semwright_native_begin(const char *id);
void semwright_native_cancel(const char *id);
void *semwright_native_call(const uint8_t *bytes, size_t length, size_t *output_length);
void semwright_native_free(void *bytes);
void semwright_native_pump(void);
bool sw_secure_input_active(void);
#endif
