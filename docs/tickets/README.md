# Session tickets

Source of truth: [speaker-keepalive-spec.md](../../speaker-keepalive-spec.md).
This is the accepted Milestone 1 plan, saved for independent implementation
sessions. Implement one ticket per session, in order. Read the specification,
this index, and the predecessor handoff before editing. An unfinished ticket
continues in the next session; do not bypass its acceptance criteria.

After user-observed pulse success, the user authorized SK-07 as a narrow
exception to the original order: prepare a login service while remaining SK-06
measurements stay open. SK-08 is a subsequent user-authorized continuous-tone experiment after reported
pulse unreliability. Other later-milestone features remain deferred.

| Ticket | Deliverable | Status | Prerequisite |
| --- | --- | --- | --- |
| [SK-01](SK-01.md) | CLI contracts and audio generation | Complete | Specification and plan |
| [SK-02](SK-02.md) | Bounded subprocess backend and one-shot test | Complete | SK-01 complete |
| [SK-03](SK-03.md) | Periodic pulse runtime | Complete | SK-02 complete |
| [SK-04](SK-04.md) | Continuous digital silence | Complete | SK-03 complete |
| [SK-05](SK-05.md) | Release checks and experiment handoff | Complete | SK-04 complete |
| [SK-06](SK-06.md) | User-led physical validation | In progress: pulse success reported; measurements pending | SK-05 complete; user observation |
| [SK-07](SK-07.md) | Minimal start-at-login user service | Implemented; desktop checks pending | User-authorized plan after pulse trial |

| [SK-08](SK-08.md) | Continuous quiet tone and service trial | Implemented and installed; physical observation pending | User-approved continuous-tone plan |

## Fixed implementation decisions

- Synchronous Rust, edition 2021, compatible with installed Rust 1.82; keep a
  lockfile and add dependencies only when used.
- Use `pw-play --raw --format f32 --rate 48000 --channels 2 -`, after checking
  installed capabilities and the exact raw sample representation in SK-02.
  Local PipeWire 1.6.9 help/man pages advertise raw stdin and `f32` support.
- Cache pulses in memory; continuous silence reuses a bounded zero buffer.
  Continuous tone uses phase-preserving generation in fixed-size buffers.
  Apply amplitude once in samples and retain unity backend stream volume.
- Use the system default output. Device selection/discovery, config files,
  playback activity detection, and status are deferred. The minimal user service
  is authorized separately in SK-07.
- Keep the specification's defaults and input bounds. Do not adjust audio
  configuration, change output volume, or increase amplitude automatically.
- Tests must use fake backends and must not unexpectedly play physical audio.
- Software completion is separate from physical validation. Stop after
  Milestone 1; later milestones need the user's results and authorization.

## Required session handoff

Append a handoff to the ticket and update this index at the end of each session:

1. **Status:** ready, in progress, blocked, or complete.
2. **Changes:** files changed and behavior delivered.
3. **Verification:** exact commands, results, and checks not performed.
4. **Decisions:** interfaces, dependencies, and deviations with reasons.
5. **Open issues:** concrete failures or missing evidence.
6. **Next session:** next ticket ID and exact starting task.
7. **Workspace state:** relevant unfinished work and commit identifier if Git
   is available. Do not invent a commit when the workspace is not a Git repo.

Mark a ticket complete only when its acceptance criteria pass. Later tickets
remain blocked until their prerequisite is complete. A blocked hardware test
must not be represented as successful because software checks pass.
