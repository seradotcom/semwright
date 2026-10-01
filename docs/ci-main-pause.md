# Temporary automatic main CI pause

Repository variable `SEMWRIGHT_PAUSE_MAIN_CI=true` skips jobs triggered by `push` or
`schedule` on `refs/heads/main` before they are assigned a runner. PR checks and
`workflow_dispatch` remain enabled. Release-tag workflows and branch-specific demo
workflows are unchanged. Workflow concurrency groups include the event name so an
automatic push cannot cancel a manually dispatched run on the same ref.

The pause was requested to free runner capacity during parallel development. A
skipped or cancelled run is not validation evidence; it does not establish that a
new main revision passed. Historical results retain their original source SHA.

Resume normal automatic main validation:

```sh
gh variable set SEMWRIGHT_PAUSE_MAIN_CI --body false --repo seradotcom/semwright
```

Deleting the variable also resumes automatic validation. Setting it to `true`
pauses future automatic main jobs. Changing the variable does not cancel existing
runs; cancel only the intended main push/schedule runs separately.

To validate main while paused, use a workflow's **Run workflow** button, or:

```sh
gh workflow run ci.yml --ref main --repo seradotcom/semwright
```

Re-enabling the variable does not replay skipped revisions. Dispatch the necessary
workflows explicitly if acceptance evidence is needed before the next push.
