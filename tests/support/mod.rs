use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub directory: PathBuf,
    pub program: PathBuf,
}

impl Fixture {
    pub fn new(case: &str) -> Self {
        assert!(
            Path::new("/usr/bin/python3").exists(),
            "fake-backend tests require /usr/bin/python3"
        );
        let directory = std::env::temp_dir().join(format!(
            "speaker-keepalive-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        let program = directory.join("pw-play");
        fs::write(&program, include_str!("fake_pw_play.py")).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(directory.join("case"), case).unwrap();
        Self { directory, program }
    }

    pub fn assert_reaped(&self) {
        let pids = fs::read_to_string(self.directory.join("pids")).unwrap();
        for pid in pids.lines() {
            assert!(
                !Path::new(&format!("/proc/{pid}")).exists(),
                "fake child {pid} is still present (including zombies)"
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Cleanup even after a failed assertion, but only signal a PID whose
        // current command line still identifies this particular fixture.
        if let Ok(pids) = fs::read_to_string(self.directory.join("pids")) {
            for pid in pids.lines() {
                if let (Ok(pid_number), Ok(command)) =
                    (pid.parse::<i32>(), fs::read(format!("/proc/{pid}/cmdline")))
                {
                    if String::from_utf8_lossy(&command).contains(self.program.to_str().unwrap()) {
                        let _ = nix::sys::signal::kill(
                            nix::unistd::Pid::from_raw(pid_number),
                            nix::sys::signal::Signal::SIGKILL,
                        );
                    }
                }
            }
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub fn wait_for_file(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
