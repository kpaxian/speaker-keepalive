# Hardware validation worksheet — SK-06

**Current experiment: continuous tone; physical result pending.** On 2026-10-07,
the user reported that the speaker still shuts off without a clear pattern and
selected continuous 100 Hz / 0.0005 playback through the login service for the
next trial. This does not establish the cause of shutdowns.

**Earlier result: default pulse feasibility supported by user observation.** The user
reported 1650.000 seconds (27½ minutes) with the JBL still on, confirmed this
exceeded its usual idle timeout from personal experience, reported no artifacts,
and described resource use as visually negligible. The parent stopped after
Ctrl+C. Exact measurements, route inspection, and child cleanup remain pending.
Software checks alone do not establish these physical results.
Follow the [README procedure](../README.md#physical-experiment-procedure).
Run only one keepalive instance, with the JBL selected as default output, and
stop before changing outputs. Commands below are for the user-led session;
creating this worksheet did not run them against a physical speaker.

## Continuous-tone follow-up — 2026-10-07

Use `run --mode tone --frequency 100 --volume 0.0005` through the updated service.
Keep only one instance, verify its route to the JBL, leave system/speaker volume
unchanged, and keep the computer awake. Plan at least one hour, extending beyond
the latest observed shutdown time if longer. Record interruptions separately from
healthy continuous playback; transport progress alone is not hardware success.

| Observation | Result |
| --- | --- |
| Start/end time and elapsed duration | Started 2026-10-07 19:22:11 EEST after cleanup verification; end/duration pending |
| Latest pulse-mode shutdown time, if known | No consistent pattern reported |
| JBL route and mute/volume state | Running keepalive node 78; active stereo links 79 and 74 to JBL Charge Essential 2; default sink volume 0.24, not muted, unchanged |
| Speaker stayed powered on | Pending user observation |
| Audible tone, clicks, or other artifacts | Pending user observation |
| Stream failures/restarts during trial | None in initial journal check; longer trial pending |
| Parent, playback-child, and backend CPU/RSS | Initial ps snapshot: parent 3440 KiB RSS / 0.4% lifetime CPU; child 8480 KiB / 0.0% rounded CPU. Not a steady-state measurement; backend comparison pending |
| Stop leaves no parent or playback child | Passed before final trial start: PIDs 33788 and 33799 both absent after systemctl stop; restarted as parent 34382 / child 34386 |

Retain the original pulse observations below; this is a separate experiment.

## Setup and baseline

| Field | Observation |
| --- | --- |
| Date / observer | Pending |
| Fedora, kernel, CPU | Pending |
| Release binary version / SHA-256 (`sha256sum target/release/speaker-keepalive`) | Pending |
| PipeWire / WirePlumber versions | Pending |
| Speaker model, battery/charging state | Pending |
| Bluetooth profile / codec, if visible | Pending |
| JBL sink name and default-output confirmation | Pending |
| System/speaker volume and mute state (keep constant across trials) | Pending |
| YouTube workaround and other playback stopped | Pending |
| Baseline start / shutdown time / elapsed duration | User confirms usual timeout is shorter than 27½ minutes; no timed baseline recorded |
| Baseline repetitions or “did not shut down within…” | Pending |
| Planned trial duration and margin beyond baseline | Pending |

Use at least 15 minutes **and** longer than the observed baseline with margin.
For example, choose `max(15 minutes, baseline + 5 minutes)` and record that
choice. If baseline shutdown has not been observed, a bounded observation is
not a measured timeout: extend the baseline or leave the comparison inconclusive.
Do not keep a routing-test stream or audio monitor playing during baseline.

## Confirm the route

Before playback, inspect the default sink without changing it:

```bash
wpctl status -n
wpctl inspect @DEFAULT_AUDIO_SINK@
wpctl get-volume @DEFAULT_AUDIO_SINK@
```

Confirm the sink is the JBL Bluetooth output, not a built-in/HDMI device. Start
this read-only link monitor in another terminal **before** the one-shot pulse:

```bash
pw-link -lmI
```

Then run the README one-shot command. Look for the keepalive stream's output
ports linking to the recorded JBL sink's playback ports. Stop the monitor with
Ctrl+C after inspection. `wpctl status -n` and `wpctl inspect STREAM_ID` can also
inspect a stream while it exists; replace `STREAM_ID` with the observed numeric
ID. Its expected `application.id` is `speaker-keepalive`. Short pulses can
vanish before a snapshot; repeat the manual one-shot if necessary. A default
sink setting alone does not prove that a particular stream reached it. If the
link path is indirect, inspect its intermediate nodes and leave routing pending
until it can be traced to the JBL.

These are the documented [WirePlumber inspection commands](https://pipewire.pages.freedesktop.org/wireplumber/man/wpctl.html)
and [PipeWire link-list/monitor options](https://docs.pipewire.org/page_man_pw-link_1.html).

| Route evidence | Observation |
| --- | --- |
| One-shot command, timestamp, exit status | Pending |
| Stream identifier / source ports / links / JBL destination | Pending |
| Speaker output observed, if audible | Pending |
| Any explicitly chosen louder routing test (e.g. 0.005) | Pending |
| Default 0.0005 amplitude restored before feasibility trial | Pending |

Inaudibility is not evidence of failure. Conversely, process success or pipe
writes do not prove that the full short pulse reached the speaker.

## Trials

Copy a row for every trial; do not overwrite failures. Stop each run with Ctrl+C
before changing one parameter. Never run this progression as a script or loop.
Keep interval 30 s and frequency 100 Hz while following the prescribed sequence.

| Trial | Mode | Duration ms | PCM amplitude | Start/end, elapsed | JBL stayed on? | Audible artifacts / comfort | Route and cleanup evidence | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Default | pulse | 300 | 0.0005 | At least 1650 s reported; timestamps unrecorded | Yes, user reported | No artifacts reported | Parent stopped; route/child inspection pending | User-observed success beyond usual timeout |
| If default ineffective | pulse | 500 | 0.0005 | Pending | Pending | Pending | Pending | Not run |
| If still ineffective | pulse | 500 | 0.001 | Pending | Pending | Pending | Pending | Not run |
| If still ineffective | pulse | 500 | 0.002 | Pending | Pending | Pending | Pending | Not run |
| If still ineffective | pulse | 500 | 0.005 | Pending | Pending | Pending | Pending | Not run |
| If pulses ineffective | silence | N/A | digital zero | Pending | Pending | Pending | Pending | Not run |

Stop increasing amplitude if audible or uncomfortable. Record shutdown time if
it occurs, any reconnection/routing changes, backend errors, suspend, and other
playback that could confound a trial. Repeat confounded trials before drawing a
conclusion. A viable result needs acceptable practical inaudibility throughout
the full test window, not only a successful command exit.

## Verify Ctrl+C cleanup

Before starting, check that no earlier experiment is running:

```bash
pgrep -af '[s]peaker-keepalive run'
pgrep -af '[p]w-play.*application.id.*speaker-keepalive'
```

No matching output is expected. If there is an earlier instance, inspect it and
stop that experiment first; do not kill unrelated audio processes.

While the foreground run is active, in a second terminal identify its PID from
the first command above and record it (the variable is local to that terminal):

```bash
read -r -p 'Keepalive PID from the process listing: ' KEEPALIVE_PID
ps -p "$KEEPALIVE_PID" --ppid "$KEEPALIVE_PID" -o pid,ppid,stat,rss,args
```

Record any playback-child PID too. Pulse mode normally has no child between
pulses. Press Ctrl+C in the playback terminal; after it exits, run:

```bash
ps -p "$KEEPALIVE_PID" -o pid,ppid,stat,args
pgrep -af '[p]w-play.*application.id.*speaker-keepalive'
```

The parent and tagged playback processes must be absent. Also check each
recorded child PID with `ps -p CHILD_PID -o pid,ppid,stat,args`, replacing
`CHILD_PID` with its number; no row, including a `Z` zombie row, should remain.
Recheck links/streams with `wpctl status -n` if desired. Record observed shutdown
latency and any leftover process instead of inferring cleanup from exit status.
Check both during pulse idle and during active playback/silence when practical.

## Measure resource use

Measure the release executable, without `--verbose`, after establishing a
working mode. Use the same quiet desktop workload and Bluetooth connection for
baseline and keepalive windows. Record at least 180 seconds per window (several
pulse cycles), separate from the longer physical-effectiveness trial. Repeat
if unrelated workload changes. A parent-only number omits most audio work.

The following commands require `pidstat` from **sysstat**, an optional observation
tool, not a runtime dependency. It was not installed in the SK-05 environment;
arrange its availability before measurement. `ps`, `pgrep`, and `watch` are
procps tools. Run monitors in separate terminals so Ctrl+C in one does not stop
the foreground experiment inadvertently.

With keepalive stopped, collect backend baseline:

```bash
LC_ALL=C pidstat -u -r -p ALL -C 'pipewire|wireplumber' 1 180
```

Start the selected release `run` command from the README in its own terminal.
Identify `KEEPALIVE_PID` as above. In a monitoring terminal, collect the parent,
including CPU accounting for reaped short-lived children:

```bash
LC_ALL=C pidstat -u -r -T ALL -p "$KEEPALIVE_PID" 1 180
```

In another terminal collect active playback children and audio services:

```bash
LC_ALL=C pidstat -u -r -p ALL -C 'pw-play|pipewire|wireplumber' 1 180
```

Repeat the baseline after stopping keepalive to check background drift. For
silence, the persistent `pw-play` should appear in repeated samples. For pulses,
one-second sampling may miss a short child; use the parent's collected child
CPU accounting rather than declaring the missing samples zero. For a separate
short child-RSS observation, watch across a pulse start, then stop the watcher:

```bash
watch -n 0.1 "ps -p $KEEPALIVE_PID --ppid $KEEPALIVE_PID -o pid,ppid,stat,rss,args"
```

Record child RSS while present. This is sampled RSS, not a guaranteed peak; if
missed, mark it unmeasured and repeat observation. Keep this faster watcher out
of the main CPU comparison, since monitoring itself adds work.

Interpretation, following the [upstream pidstat manual](https://github.com/sysstat/sysstat/blob/master/man/pidstat.1):

- TASK rows give parent CPU and RSS (KiB). Record idle intervals separately
  from startup/pulse intervals; report the window average and observed RSS range.
- CHILD rows include the selected parent **and** its children; they are not
  child-only CPU. Child accounting arrives after exit/reaping. Sum reported
  user/system milliseconds over the window, avoiding a second count of summary
  rows. Divide by elapsed milliseconds and multiply by 100 for average CPU.
  Subtract the parent's CPU over the same window to estimate child-only cost.
- Do not add TASK CPU to CHILD aggregate CPU. Do not count individual `pw-play`
  CPU again after including it in the aggregate. RSS is sampled per process;
  adding RSS double-counts shared pages and is not unique physical memory.
- Compare PipeWire, pipewire-pulse (if present), and WirePlumber before/during/
  after. Report their incremental cost separately; Bluetooth/kernel/codec costs
  may not all be attributable to these processes. This is not a power measurement.

| Window / mode / settings | Length | Parent idle / mean CPU | Parent RSS range KiB | Child or aggregate CPU, labeled | Child sampled RSS KiB | PipeWire / WirePlumber CPU and RSS | Baseline difference / caveats |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Before baseline | Pending | N/A | N/A | N/A | N/A | Pending | Pending |
| Pulse, if viable | Pending | Pending | Pending | Pending | Pending | Pending | Pending |
| Silence, if viable | Pending | Pending | Pending | Pending | Pending | Pending | Pending |
| After baseline | Pending | N/A | N/A | N/A | N/A | Pending | Pending |

Near-zero parent idle CPU and under roughly 10–20 MB parent RSS are goals, not
results. Retain measurement resolution and raw output with the observations;
`0.00%` is a rounded sample, not proof of zero CPU use.

## Decision and stop condition

| Question | Recorded answer |
| --- | --- |
| Which mode/settings kept the JBL awake for the required window? | Default pulse run, at least 27½ minutes; longer than usual timeout per user |
| Was the signal practically inaudible and acceptable? | No audible artifacts reported; user considers trial passed |
| Did Ctrl+C leave no playback child? | Parent stopped per user; playback child not independently checked |
| Parent and child/backend resource measurements complete? | No; user described process-list use as negligible, exact CPU/RSS and backend costs unmeasured |
| Any inconclusive trial / remaining investigation? | Pending |
| User-confirmed Milestone 1 outcome | Pulse feasibility accepted; formal measurement/inspection items remain open |
| Explicit authorization for a later milestone | User approved and requested implementation of SK-07 minimal login service; other later scope not authorized |

If only silence works, report that outcome and propose it as the operating mode.
If both work, retain pulse mode as preferred and compare measured resource use.
If neither works or observations are incomplete, leave Milestone 1 incomplete
and record the next investigation. **Stop here:** later milestones require both
the user's physical result and explicit authorization.
