# Routing and units

Use sample frames for portable audio processing and the shared rational media-time contract for AV exchange.

Keep units explicit: linear gain, milli-dBFS, milli-dBTP, LUFS-milli, samples, frames and normalized pan are not interchangeable. Validate channel counts, bus/send targets, automation targets and feedback cycles. Algebraic feedback is invalid; a declared bounded delay may make a feedback graph meaningful.

Ardour semantic IDs returned by project inspection are the only IDs an agent should feed back into deep route/clip mutations. Native Ardour IDs remain an adapter concern.
