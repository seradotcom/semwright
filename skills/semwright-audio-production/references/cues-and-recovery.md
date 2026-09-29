# Cues and recovery

Use stable cue IDs and the shared media-time mapping for AV. Imported narration timing is valid only for the asset digest/version that produced it.

Do not replay a mutation automatically after timeout or unknown outcome. Reinspect first. Ardour deep mutations compare an expected semantic revision before writing; stale revisions fail.

When an AV phase fails after prior effects, report partial/unknown state and keep intermediate artifacts private. A valid audio artifact can be reused after a visual-only change only when its dependency set is unchanged.
