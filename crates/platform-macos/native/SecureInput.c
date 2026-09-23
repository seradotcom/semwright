#include <stdbool.h>
#include <Carbon/Carbon.h>
/* Query only. Semwright never disables or alters Secure Event Input. */
bool sw_secure_input_active(void) { return IsSecureEventInputEnabled() != 0; }
