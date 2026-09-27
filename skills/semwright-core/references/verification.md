# Verification

Verification should observe the state that matters to the user's intent.

Prefer, in order:

1. a provider semantic read of the mutated object;
2. an explicit verify capability when the provider exposes one;
3. a returned artifact digest/metadata plus destination inspection;
4. a terminal Semwright job state.

Do not treat a successful transport envelope as proof of application state when the operation itself is asynchronous or can have an unknown outcome.

If verification contradicts the requested result, report the mismatch. Do not silently apply additional mutations unless the procedure explicitly calls for recovery and the new operation is independently authorized.
