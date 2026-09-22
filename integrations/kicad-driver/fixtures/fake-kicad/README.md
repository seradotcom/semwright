# Fake IPC independiente

`server.py` usa stdlib Python, un socket Unix temporal y bytes protobuf. Implementa SP IPC/REQ-REP v0 y las respuestas curadas, commits staged, selección y fallos controlados. No usa el parser Go ni abre archivos de proyecto. La biblioteca Google Protobuf valida por separado un fixture de envelope/version.

El fake soporta estados normal/busy/hang/drop/oversized/malformed/wrong_id/reject_item/apply_then_drop. Tests controlan cambio de token, documento, eliminación/reuso y reemplazo del socket. Los únicos tokens incluidos son constantes explícitamente sintéticas. No son credenciales KiCad.

Esta implementación **no es libnng ni KiCad**. Un pass verifica el contrato implementado y sus pruebas negativas, no la compatibilidad de un editor real.
