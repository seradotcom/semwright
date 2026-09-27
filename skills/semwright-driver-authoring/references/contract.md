# Driver contract

Treat the checked-in Driver SDK as the source of truth. Protocol versions, interfaces, native-ref support, jobs/events, artifact metadata, sandbox mounts, and cancellation evolve independently of this Skill.

A driver capability belongs under its owner-assigned identity namespace. Keep descriptors deterministic and bounded. Prefer a small coherent semantic surface over hundreds of aliases that repeat the underlying API without product meaning.

Application detection belongs in the manifest/application identity contract. Provider identity and provenance come from the owner-loaded host, not from untrusted driver text.
