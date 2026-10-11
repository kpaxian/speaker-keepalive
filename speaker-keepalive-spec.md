# Bluetooth Speaker Keepalive — Implementation Specification

## Authorized amendment — 2026-10-07

Following intermittent shutdowns after the earlier pulse trial, the user approved
SK-08: add `run --mode tone` and use it in the login service at 100 Hz and amplitude
0.0005. This mode uses a continuous phase-coherent sine stream, a single startup
fade, bounded buffers and existing cleanup/retry behavior. It accepts frequency
and volume; explicit interval/duration options are invalid. Plain `run` retains
pulse defaults and silence mode remains available. No device discovery or other
later milestone is authorized by this amendment. Physical efficacy is pending.

## 1. Goal and environment

Build a small Rust CLI utility for Fedora Linux with PipeWire, WirePlumber and systemd user services. It should prevent a **JBL Charge Essential 2** Bluetooth speaker from switching off while idle, with minimal resource use.

The user currently keeps the speaker awake by playing a silent YouTube stream. This establishes that continuous playback works in their setup; it does **not** establish that short pulses work, that nonzero samples are required, or what the speaker's inactivity threshold is.

Preferred behavior: send a short, extremely quiet audio pulse every **30 seconds**, with a configurable interval. A continuous digital-silence mode is an experimental fallback.

No GUI, web server, telemetry, application network access, root requirement, or Bluetooth connection management.

## 2. Instructions to the implementing Codex agent

Implement **Milestone 1 only**, then stop and report how to build and test it. Do not implement later milestones until the user reports the physical speaker test outcome and authorizes continuation.

Do not claim hardware validation from unit tests or successful audio-process exit codes. The user must verify that the speaker stays powered on and that the signal is acceptable.

Keep the code synchronous and small. Prefer `pw-play` for v1 rather than direct PipeWire bindings. Verify the installed tool's supported arguments and input formats before implementing the subprocess protocol. If raw stdin introduces uncertainty, an in-memory WAV sent through a supported input path or a single reusable temporary WAV is acceptable; document the choice.

## 3. Default pulse parameters

| Parameter | Default | Meaning |
| --- | --- | --- |
| Mode | `pulse` | Short signal followed by idle time |
| Interval | 30 s | Desired time between pulse starts |
| Duration | 300 ms | Signal duration |
| Frequency | 100 Hz | Experimental starting point, not a documented JBL threshold |
| Amplitude | 0.0005 | Linear PCM peak amplitude relative to full scale |
| Sample rate | 48,000 Hz | Internal generation rate |
| Channels | 2 | Identical signal in both channels |

Amplitude 0.0005 corresponds to approximately **−66 dBFS peak**, not −70 dBFS. Actual acoustic loudness depends on system volume, speaker volume, codec and hardware. Inaudibility cannot be guaranteed from this value alone.

Generate a sine wave with short fade-in and fade-out (for example 10 ms each, shortened for very short pulses) to avoid discontinuities and clicks. Use floating-point PCM if supported, or sufficient precision to preserve low amplitudes. Apply amplitude once: do not scale samples and then accidentally apply the same attenuation again through stream volume.

Validate all CLI and configuration inputs. Reject NaN, infinity and invalid values with clear errors; do not silently turn invalid input into louder playback. Suggested initial bounds:

- interval: 1–3600 seconds;
- duration: 20–5000 milliseconds, shorter than the interval;
- frequency: 20–1000 Hz, below Nyquist;
- amplitude: 0–0.05, with zero explicitly allowed for silence experiments.

These are input limits, not guarantees about safe or inaudible acoustic output.

## 4. Milestone 1 — Physical feasibility experiment

### Required CLI

```bash
speaker-keepalive run
speaker-keepalive run --interval 30 --duration 300 --frequency 100 --volume 0.0005
speaker-keepalive test --frequency 100 --duration 300 --volume 0.0005
speaker-keepalive run --mode silence
```

`--volume` means the PCM amplitude above; explain this consistently in help and README. `test` plays one pulse and exits. `run --mode silence` keeps an audio stream open with zero-valued PCM until interrupted; interval/frequency/pulse duration do not apply in this mode, and incompatible explicit options should be rejected or clearly explained.

For this milestone the user selects the JBL as the system default output. Explicit targeting with a known sink name may be offered if simple, but discovery is deferred. Document that default routing may change and that users should stop the experiment before changing outputs.

### Runtime requirements

- Generate/cache the pulse once per parameter set, not repeatedly on disk.
- Sleep between pulse starts using monotonic time; avoid busy waiting and accumulated playback-duration drift.
- Never overlap pulses or replay a backlog after suspend or a delayed playback.
- Handle Ctrl+C promptly, including during sleep, writes and playback.
- Close, terminate if needed, and reap all child processes on shutdown.
- Bound playback/subprocess hangs; never block indefinitely on an unresponsive backend.
- Treat a missing `pw-play` or unsupported invocation as an actionable startup error.
- Transient audio errors must not cause a tight retry loop. Retry at the configured interval and rate-limit repeated diagnostics.
- Continuous silence needs bounded buffers/backpressure and clean shutdown; no unbounded allocation or file growth.
- Keep normal logs minimal; offer `--verbose` for pulse/result diagnostics.

A persistent silent stream and periodic pulses can have different Bluetooth/backend behavior. Switching on a short stream may involve startup latency, and a successful write does not prove the full pulse reached the speaker. Document these limitations.

### User validation procedure

1. Connect the JBL and set it as the default output. Stop the YouTube workaround and all other playback.
2. Observe the baseline shutdown time without keepalive. Use that duration to set a meaningful test window.
3. Run `test` and confirm routing using PipeWire tools. If necessary, explicitly use a modest louder amplitude such as 0.005; never increase volume automatically.
4. Run default pulse mode for **at least 15 minutes and longer than the observed baseline timeout**, with margin. Record settings, whether the speaker stays on, and any audible artifacts.
5. If ineffective, try 500 ms at the same amplitude, then explicitly increase amplitude: 0.0005 → 0.001 → 0.002 → 0.005. Change one parameter at a time; stop increasing if audible or uncomfortable.
6. If short pulses remain ineffective, test continuous digital silence over the same window. Do not infer failure from inaudibility alone: verify the route and stream.
7. If only silence works, report that result and propose keeping it as the operating mode. If both work, retain pulse mode as the preferred option and measure resource use.

Do not automate physical calibration or invent a `calibrate` command that claims to detect speaker power state. The program cannot determine the threshold without suitable hardware feedback.

### Acceptance and checks

- Release build succeeds on Fedora; `cargo fmt --check`, appropriate tests and `cargo clippy` pass.
- Meaningful tests cover sample count, amplitude/fades, input validation and clean subprocess failure handling where practical.
- User confirms that the speaker remains awake and the signal is practically inaudible for the test window.
- Ctrl+C leaves no playback child running.
- Measure idle CPU and RSS for the release executable; also report the audio child/backend overhead rather than claiming the entire solution uses zero CPU.

**STOP after Milestone 1.** Report files, dependencies and their rationale, build/run/test commands, checks completed, limitations, and the exact hardware observations required from the user.

## 5. Milestone 2 — Persistent configuration

Add optional configuration at `$XDG_CONFIG_HOME/speaker-keepalive/config.toml`, falling back to `~/.config/speaker-keepalive/config.toml`.

```toml
mode = "pulse"
interval_seconds = 30
signal_duration_ms = 300
frequency_hz = 100
volume = 0.0005
device = "auto"
skip_when_audio_playing = true
```

Precedence: explicitly supplied CLI options > configuration > built-in defaults. Missing config uses defaults. Invalid configuration produces an actionable error. No mandatory manual file creation. Document that changes take effect on restart; hot reload is outside v1.

Acceptance: defaults work, config is loaded, partial overrides preserve other settings, and CLI overrides take precedence. Preserve existing user configuration during installation.

## 6. Milestone 3 — Device discovery and selection

Add `speaker-keepalive devices`, listing relevant sinks, descriptions, stable node names, Bluetooth classification and the default output. Prefer structured PipeWire output to parsing human-oriented status text.

Device rules:

- `device = "auto"`: use the default sink **only if it is a Bluetooth audio sink**. If the default is built-in/HDMI, skip playback; do not fall back to an unrelated speaker or headset.
- Explicit selection: use a stable PipeWire node name rather than a transient numeric ID. Verify it resolves to an available audio sink before playback.
- Missing/disconnected target: skip, log only state changes or in verbose mode, and check again next cycle.
- Reconnection: rediscover the node and resume automatically; never reconnect Bluetooth aggressively.
- Bind playback explicitly to the selected sink and prevent backend fallback to another output where supported. Test/document disconnect races and limitations.

Acceptance: selection works, unavailable targets do not crash or play elsewhere, and reconnection resumes operation. Keep milestone 1's simple default-output experiment distinct from these production routing rules.

## 7. Milestone 4 — Playback activity detection

Detect running playback streams **routed to the selected sink**, including indirect routing through links where practical. Audio on other outputs must not suppress keepalive. Exclude this application's streams using a stable application identifier.

An open/paused stream is not necessarily running; a running stream is not proof of nonzero sound. Describe detection as playback-stream activity, not acoustic analysis.

If reliable detection is unavailable, expose the limitation and allow explicit `skip_when_audio_playing = false`. Never silently claim the feature works. On an indeterminate activity check, a conservative low-volume pulse may still be sent to preserve keepalive, with a rate-limited diagnostic; document this policy.

For continuous-silence mode, retain the silent stream alongside normal playback in v1. Do not repeatedly close/reopen it on activity transitions unless a later validated design justifies that behavior. Explain that the skip option applies to pulse mode.

Acceptance: relevant active playback skips pulses; pause/stop resumes pulses; unrelated playback does not suppress pulses; the application's stream does not suppress itself.

## 8. Milestone 5 — Status and systemd integration

Add `speaker-keepalive status`, separating:

- effective configuration for this invocation;
- selected sink and availability;
- user service state;
- activity-detection availability.

Do not display a last pulse time or a running daemon's live configuration unless those values are actually available. IPC is unnecessary for v1.

Provide a systemd **user** service with foreground `run`, `Restart=on-failure` and a restart delay. Use a durable absolute executable path when installing (such as `~/.local/bin/speaker-keepalive`), correctly escaped for systemd. Avoid a dependency on a temporary build path. Handle PipeWire availability through retry/backoff rather than assuming session startup order guarantees readiness.

Document:

```bash
systemctl --user daemon-reload
systemctl --user enable --now speaker-keepalive.service
systemctl --user status speaker-keepalive.service
journalctl --user -u speaker-keepalive.service
systemctl --user disable --now speaker-keepalive.service
```

Optional `install` creates the user config if absent and installs the binary/unit, then reloads units. It does **not** enable/start the service. Explicit `enable`/`disable` commands may wrap the corresponding systemctl operations. No automatic system changes during build or ordinary execution. Prevent or document duplicate foreground/service instances targeting the same speaker.

Acceptance: service runs after login, recovers from transient disconnection and backend restarts, preserves config, stops cleanly, and requires no root.

## 9. Architecture and dependencies

Start with only the files Milestone 1 needs; split modules when their responsibilities exist.

```text
speaker-keepalive/
  Cargo.toml
  Cargo.lock
  README.md
  src/main.rs
  src/audio.rs
  src/backend.rs
  src/config.rs          # Milestone 2
  src/devices.rs         # Milestone 3
  src/activity.rs        # Milestone 4
  src/service.rs         # Milestone 5
  examples/config.toml   # Milestone 2
  systemd/speaker-keepalive.service  # Milestone 5
```

Possible dependencies: `clap` for CLI, `ctrlc` for interruption, `serde` + `toml` for config, and `serde_json` if structured device discovery requires it. Use standard-library errors or one small error crate if helpful. Explain each dependency when introduced; do not add all dependencies up front. No async runtime without a concrete need. Runtime dependencies on PipeWire commands must be documented and checked.

## 10. Resource goals and development rules

Aim for near-zero idle CPU in pulse mode and ideally under 10–20 MB RSS for the main process. These are goals to measure, not promised results. Bluetooth transmission, PipeWire and codecs have overhead, particularly with continuous silence. No runtime network access or recurring disk writes beyond minimal logs. Test which mode works before optimizing resource consumption.

1. Do not change unrelated system audio configuration or device volume.
2. No unexpected audible test or automatic amplitude escalation.
3. Keep subprocess arguments structured; never construct shell commands from device names or config values.
4. Ask for help at a genuine integration impasse instead of adding increasingly complex workarounds.
5. Run checks appropriate to each milestone, then report results and unresolved hardware limitations honestly.
6. Do not advance past the first milestone's hardware gate without the user's test result and authorization.
