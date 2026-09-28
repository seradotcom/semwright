# Composition benchmark harness

This benchmark compares equivalent authoring workflows without declaring a winner.

It is intentionally split into **task definition**, **execution evidence**, and **summary**. The harness never fabricates application output and never interprets tool-call count as visual quality. Each arm must operate on the same task input, asset digests, output profile and acceptance checks.

## Arms

- `low_level`: a competent implementation using the existing lower-level semantic capabilities and templates/APIs available to the provider.
- `high_level`: the Composition/authoring lifecycle over the same provider and same native application/runtime.

A task may omit one arm only with a recorded blocker. An opaque flattened artifact is not a valid substitute when the task requires native editability.

## Recorded fields

Per arm: setup duration, execution duration, calls, retries, errors, optional observable token usage, interventions, resulting artifact digests, native editability, exact-change checks, validation/verification status and limitations. Setup is separate from repeated-run cost.

The summary reports raw values and deltas only. It does not assign a score, tier or winner. Creative review is a separate human evidence field.

## Usage

1. Copy `evidence.example.json` to a private/job-local evidence file.
2. Populate it from one exact tested SHA and the configured task.
3. Run `python benchmarks/composition/run.py validate <file>`.
4. Run `python benchmarks/composition/run.py summarize <file> <summary.json>`.
5. Keep generated evidence under `verification/` or a CI artifact, not Git, unless it is a deliberately small reviewed fixture.

The harness does not execute Figma, Motion Canvas or audio by itself; native execution belongs to their Broker/Driver Host E2E suites. This separation prevents the benchmark runner from becoming a parallel authority path.
