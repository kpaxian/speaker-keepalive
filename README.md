# speaker-keepalive

A small Rust CLI experiment intended to keep a JBL Charge Essential 2 awake on
Fedora with PipeWire. See [the specification](speaker-keepalive-spec.md) for the
requirements and [the session tickets](docs/tickets/README.md) for implementation
order and handoffs.

**Current status: default pulses worked in the user's JBL trial for at least
27½ minutes**, exceeding its usual idle timeout, with no reported artifacts.
Exact resource measurements and playback-child cleanup remain unverified in
[the worksheet](docs/hardware-validation.md). A user-service file and setup
instructions are now available below; installation and activation are separate.

## Build and check

Requires Linux and Rust/Cargo 1.82 or newer. The lockfile records the tested
dependencies. Tests also require `/usr/bin/python3` for fake playback executables;
they never use a real audio device. Playback requires `pw-play` on PATH (provided
by `pipewire-utils` on this Fedora installation) and an available PipeWire session.

```bash
cargo build --locked
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
./target/release/speaker-keepalive --help
./target/release/speaker-keepalive run --help
./target/release/speaker-keepalive test --help
```

Direct dependencies are pinned to versions verified with Rust 1.82:

- `clap` 4.5.20 supplies argument parsing, help, and useful errors; its color
  feature is disabled to keep dependencies small.
- `ctrlc` 3.4.5 installs the Ctrl+C handler.
- `nix` 0.29.0 supplies safe Unix wrappers for nonblocking pipes, polling, and
  process signals. It is also used by `ctrlc`, avoiding a second I/O library.

There is no async runtime or runtime network access. Python is a test dependency
only; the built utility does not invoke Python. Tests need no PipeWire session.

## Start automatically at login

The [user-service file](systemd/speaker-keepalive.service) runs the existing
pulse defaults: every 30 seconds, 300 ms, 100 Hz, amplitude 0.0005. It uses the
current default output, including desktop speakers before the JBL connects.
After connecting the JBL, select it as the default output for subsequent pulses.
There is no device detection or automatic Bluetooth connection.

The service starts with the systemd user session at login. No root access or
lingering setup is needed. Existing lingering settings are left alone; if you
already enabled lingering, the user manager may also run outside a login session.

### Install the files

Run from the project directory. First stop any foreground keepalive with Ctrl+C;
check for duplicates with `pgrep -af '[s]peaker-keepalive run'`. For an upgrade,
stop the existing service with `systemctl --user stop speaker-keepalive.service`
before replacing its executable. Do not run both a foreground instance and the
service. These installation commands copy files but do not start playback:

```bash
cargo build --release --locked
install -D -b -m 0755 target/release/speaker-keepalive "$HOME/.local/bin/speaker-keepalive"
install -D -b -m 0644 systemd/speaker-keepalive.service "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/speaker-keepalive.service"
systemd-analyze --user verify "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/speaker-keepalive.service"
systemctl --user daemon-reload
```

Stop and resolve errors before continuing. `install -b` preserves overwritten
files with a `~` suffix; preserve any older backup separately before another
upgrade. The service uses `%h/.local/bin/speaker-keepalive`, independent of this
checkout or its build directory. The existing local-cache build alternative is
recorded in the [SK-05 handoff](docs/tickets/SK-05.md).

### Activate and inspect

This explicitly starts playback now and enables future login startup:

```bash
systemctl --user enable --now speaker-keepalive.service
systemctl --user status speaker-keepalive.service
journalctl --user -u speaker-keepalive.service -n 50 --no-pager
```

Check that status reports `active (running)`. Normal successful operation is
quiet, so an absence of pulse log messages is expected. To follow errors live:

```bash
journalctl --user -u speaker-keepalive.service -f
```

Ctrl+C here stops the log viewer, not the service. The service restarts after
unexpected process failure with a ten-second delay. The application handles
transient audio errors on its existing pulse schedule; it does not require
PipeWire to be ready at a particular login ordering point.

### Stop, disable, and verify

To stop for the current session (leaving future startup enabled):

```bash
systemctl --user stop speaker-keepalive.service
```

To stop and also disable automatic startup:

```bash
systemctl --user disable --now speaker-keepalive.service
systemctl --user is-active speaker-keepalive.service
systemctl --user is-enabled speaker-keepalive.service
```

After disabling, expect `inactive` and `disabled`; those checks return nonzero
for those states. Stopping sends SIGINT to the main process so its existing
handler terminates/reaps playback. Systemd forcibly cleans up remaining service
processes if necessary, with a five-second stop timeout. Confirm the service
and its tagged playback child are gone:

```bash
pgrep -af '[s]peaker-keepalive run'
pgrep -af '[p]w-play.*application.id.*speaker-keepalive'
```

With no other instance running, neither should print a process. To remove the
setup entirely, disable it first, remove the installed unit and executable,
then run `systemctl --user daemon-reload` again.

After enabling, log out and back in when convenient and verify `active (running)`
again. Actual login startup, systemd restart after failure, and live child
cleanup remain pending desktop-session checks; they are not established by the
fake-backend tests. See [SK-07](docs/tickets/SK-07.md) for verification limits.

## CLI contract

Select the JBL as the default system output before running a one-shot pulse:

```bash
./target/release/speaker-keepalive test --frequency 100 --duration 300 --volume 0.0005
./target/release/speaker-keepalive test --verbose
```

Each command above plays one pulse and exits. For periodic playback until Ctrl+C:

```bash
./target/release/speaker-keepalive run
./target/release/speaker-keepalive run --interval 30 --duration 300 --frequency 100 --volume 0.0005
```

For a continuous digital-silence stream until Ctrl+C:

```bash
./target/release/speaker-keepalive run --mode silence
```

`--verbose` is accepted before or after the subcommand and reports one-shot
parameters, each periodic attempt/result, bounded backend diagnostics, and
completion timing. Normal successful
playback is quiet. Unknown options, duplicate settings, malformed values, and
invalid ranges exit 2. Startup and one-shot backend failures exit 1.
Both run modes retry transient backend failures as described below.
Successful one-shot playback, handled Ctrl+C, and help/version exit 0.

| Option | Default | Accepted values |
| --- | --- | --- |
| `run --mode` | `pulse` | `pulse` or `silence` |
| `run --interval` | 30 seconds | Integer 1–3600 seconds |
| `--duration` | 300 milliseconds | Integer 20–5000 milliseconds |
| `--frequency` | 100 Hz | Finite 20–1000 Hz, below Nyquist |
| `--volume` | 0.0005 | Finite PCM peak amplitude 0–0.05, including zero |

For pulse `run`, duration must be strictly shorter than interval. `test` accepts
only signal options and `--verbose`, not `--interval` or `--mode`. Silence mode
rejects explicitly supplied interval, duration, frequency, and volume settings,
even if they equal defaults or zero. No configuration file is loaded.

## Audio representation and limits

Pulses are interleaved 48,000 Hz `f32` PCM with identical left and right channels.
Generation uses a sine wave with linear fade-in/out up to 10 ms, shortened for
the minimum-duration pulse so the ramps do not overlap. Both endpoints are zero.
The default pulse has 14,400 stereo frames, or 28,800 samples (115,200 bytes).
Periodic playback generates and encodes it once, then reuses the cached bytes.

`--volume` means PCM peak amplitude relative to full scale. It does not change
system or speaker volume. Amplitude is applied once during sample generation;
`pw-play` stream volume is explicitly set to unity. The default is approximately
−66 dBFS peak. These input limits do not guarantee inaudibility or acceptable
acoustic output; loudness also depends on the speaker, codec, and volume controls.

The backend serializes native-endian `f32` samples and sends them through stdin:

```text
pw-play --raw --format f32 --rate 48000 --channels 2 --volume 1 \
  --properties '{"application.id":"speaker-keepalive","application.name":"speaker-keepalive"}' -
```

This matches PipeWire 1.6.9's [raw stdin reader and f32 mapping](https://github.com/PipeWire/pipewire/blob/1.6.9/src/tools/pw-cat.c)
and [native-endian audio format aliases](https://github.com/PipeWire/pipewire/blob/1.6.9/spa/include/spa/param/audio/raw.h).
The installed tool's help, format list, and non-playing argument check were also
verified. No WAV file or recurring audio-file writes are used.

Both modes and `test` set `application.id=speaker-keepalive` and
`application.name=speaker-keepalive` for inspection and future activity exclusion.
The installed tool advertises `--properties`; its
[documented JSON stream properties](https://docs.pipewire.org/1.4/page_man_pw-cat_1.html)
and [application ID key](https://docs.pipewire.org/1.4/group__pw__keys.html)
are used. Live stream inspection is still pending.

## Subprocess behavior

- Before playback, bounded `--help` and `--list-formats` probes check required
  capabilities; each has a two-second deadline. Missing/incompatible tools fail
  with installation or compatibility guidance.
- Raw audio writes are nonblocking, handle partial writes, and close stdin at
  EOF. The parent drains stderr concurrently, retaining at most 8 KiB and marking
  truncation. Probe stdout is limited to 32 KiB; oversized capability output fails.
- A pulse has an overall deadline of its duration plus ten seconds. Pending
  audio writes also have a ten-second no-progress watchdog. Errors do not retry
  in the one-shot command.
- Ctrl+C is checked during probes, writes, and waiting for playback completion.
  Active I/O polls wait at most 100 ms. On interruption/failure the parent closes
  stdin, sends SIGTERM, allows 500 ms grace, then uses SIGKILL if needed and reaps
  the child. The child is in its own process group so terminal Ctrl+C is handled
  by the parent. Cleanup is also guarded on setup errors/unwinding.

## Periodic scheduling

`run` checks capabilities once and starts its first pulse immediately after
preparation. Subsequent starts follow a monotonic grid anchored to that first
attempt, so playback duration does not accumulate as drift. Only one child runs
at a time, and it is reaped before another starts. Missed slots are skipped;
there is no catch-up backlog. Idle waits use a condition variable awakened by
Ctrl+C, without periodic polling.

Ordinary failures retry at the next scheduled slot. A timeout or an attempt
finishing at/after the following slot requires at least a full interval after
completion (including cleanup), rounded up to the next anchored slot. For
example, with a one-second interval, an attempt finishing at 1.2 seconds retries
at 3 seconds. A wake delayed by an entire interval skips to a future slot.
Linux monotonic time excludes suspend time; resume never triggers backlog replay.

Normal success is quiet. First and changed failures are logged immediately;
identical failures are summarized at most once per minute, with suppressed
counts. Recovery is logged once. `--verbose` reports every attempt and result.

## Continuous silence

Silence mode holds one playback child open and repeatedly sends a fixed 16 KiB
buffer (2048 stereo `f32` frames), all positive-zero samples. Pipe backpressure
paces writes; there is no growing audio queue, sample counter, or audio file.
A healthy stream has no overall deadline. Ten seconds without a successful
write triggers cleanup and a restart, including when stderr remains noisy.
Retained diagnostics use the same bounded buffers as pulse playback.

Every unexpected exit, including exit status 0, is a stream failure. After
terminating and reaping the child, the runtime waits a fixed 30 seconds before
restarting. Ctrl+C interrupts both active I/O and the restart wait. Capability
checks happen once at startup; pulse options remain invalid in silence mode.
Failure diagnostics use the same throttling policy as pulses. After a replacement
stream continues accepting input for ten seconds, a recovery message is emitted;
this describes pipe input progress, not verified audio delivery. Verbose mode
reports each stream start and input progress. Normal healthy operation is quiet.

Default-output routing can change; stop before changing outputs. Explicit
targeting remains a future milestone. The persistent-silence mode and short-lived
pulse streams may behave
differently with Bluetooth, and startup latency may consume part of a short pulse.

## Physical experiment procedure

Use [the validation worksheet](docs/hardware-validation.md) to record settings,
routing, baseline and trial durations, power state, audible artifacts, cleanup,
and resource use. The commands below play real audio when you run them. Run one
instance at a time; stop before changing outputs or settings.

1. Connect the JBL and select it as the default system output. Stop the YouTube
   workaround and all other playback. Keep system/speaker volume constant.
2. Observe shutdown without keepalive and record the elapsed baseline time.
   Plan each trial for **at least 15 minutes and longer than baseline with
   margin** (for example, baseline plus five minutes if that is longer).
3. Start the worksheet's read-only PipeWire link monitor, then run:

   ```bash
   ./target/release/speaker-keepalive test --frequency 100 --duration 300 --volume 0.0005
   ```

   Confirm the stream links to the JBL. If necessary, explicitly choose a
   modest louder routing test, such as `--volume 0.005`; the utility never raises
   amplitude automatically. Restore 0.0005 for the default feasibility trial.
4. Run default pulses for the full planned window:

   ```bash
   ./target/release/speaker-keepalive run
   ```

   Record whether the JBL stays powered on and any audible artifacts. Press
   Ctrl+C at the end and verify that no playback child remains, using the
   worksheet's process checks.
5. If ineffective, try 500 ms at the same amplitude:

   ```bash
   ./target/release/speaker-keepalive run --interval 30 --duration 500 --frequency 100 --volume 0.0005
   ```

   If still ineffective, explicitly change amplitude between separate trials:
   **0.0005 → 0.001 → 0.002 → 0.005**, keeping the other settings fixed. Change
   only one parameter at a time, observe the same full window, and stop
   increasing if audible or uncomfortable. Do not automate the progression.
6. If pulses remain ineffective, try continuous digital silence for the same
   window:

   ```bash
   ./target/release/speaker-keepalive run --mode silence
   ```

   Verify its route and stream; inaudibility alone is not evidence of failure.
   Stop with Ctrl+C and check cleanup.
7. If only silence works, report that result and propose keeping it as the
   operating mode. If both work, retain pulse mode as preferred and compare
   resource use. Follow the worksheet's release parent CPU/RSS, child CPU/RSS,
   and PipeWire/backend before/during/after measurements. Record missing
   observations explicitly; do not treat missing samples as zero overhead.

Default routing may change, including after disconnection. This milestone does
not restrict playback to Bluetooth, detect other playback, or reconnect devices.
Stop the experiment before changing outputs. Startup latency may consume part
of a short pulse; successful writes/process exits do not prove full delivery or
keepalive effectiveness. No calibration or power-state detection is performed.

The user reported that default pulses kept the JBL on beyond its usual timeout,
with no reported artifacts and visually negligible resource use. Exact CPU/RSS
and child/backend overhead remain unmeasured. Near-zero idle CPU and roughly
10–20 MB parent RSS remain goals, not numerical results. The user authorized
bringing forward the minimal login service in SK-07; further milestones still
require explicit authorization.

The [SK-05 handoff](docs/tickets/SK-05.md) records the release checks and complete
Milestone 1 requirement review. Configuration files, device discovery, activity
detection, and a status CLI remain deferred. The minimal user service above is
the separately authorized SK-07 addition.
