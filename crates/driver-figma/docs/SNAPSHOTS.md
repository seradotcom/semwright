# Snapshots and diff

Identity mode retains same-file IDs. Portable mode removes volatile session/document/node IDs for cross-file semantic comparison. Both remove volatile timestamps/user/session nonce fields, round floats to four decimals and use deterministic object-key serialization.

Semantic diff is bounded and reports added/removed/changed paths. Ordered arrays stay ordered so layer z-order is not accidentally normalized away.
