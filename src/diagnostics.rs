//! Local observations for a disconnect the player marks.
use crate::controller::Status;
use std::fmt::Write;
use std::time::{Duration, Instant};

const RECENT: Duration = Duration::from_secs(30);
const FRESH: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Default)]
pub struct Connectivity {
    latest: Option<(Instant, bool)>,
    last_without_internet: Option<Instant>,
}

impl Connectivity {
    #[cfg(any(windows, test))]
    fn observe(&mut self, at: Instant, internet: Option<bool>) {
        self.latest = internet.map(|connected| (at, connected));
        if internet == Some(false) {
            self.last_without_internet = Some(at);
        }
    }
}

pub struct Event {
    pub time: String,
    pub description: &'static str,
}

pub struct DisconnectReport {
    pub finding: &'static str,
    pub explanation: &'static str,
    pub next_step: &'static str,
    pub events: Vec<Event>,
    pub has_warning: bool,
    pub marked_at: Option<String>,
}

impl DisconnectReport {
    pub fn capture(at: Instant, status: &Status, connectivity: Connectivity) -> Self {
        let recent = |time| {
            at.checked_duration_since(time)
                .is_some_and(|age| age <= RECENT)
        };
        let engine_exit = status
            .last_unexpected_engine_exit
            .filter(|&time| recent(time));
        let internet_loss = connectivity
            .last_without_internet
            .filter(|&time| recent(time));
        let internet_now = connectivity.latest.filter(|&(time, _)| {
            at.checked_duration_since(time)
                .is_some_and(|age| age <= FRESH)
        });

        let (finding, explanation, next_step) = match (engine_exit, internet_loss) {
            (Some(_), None) => (
                "The stabilizer stopped",
                "The engine exited unexpectedly shortly before you marked this disconnect.",
                if status.running {
                    "The engine is running again. Try reconnecting in Aion."
                } else {
                    "Restart the engine, then reconnect in Aion."
                },
            ),
            (None, Some(_)) => (
                "Windows reported no internet access",
                "Windows reported no internet access around the time you marked this disconnect.",
                "Check your connection before reconnecting in Aion.",
            ),
            (Some(_), Some(_)) => (
                "Engine and internet problems observed",
                "The engine exited unexpectedly and Windows reported no internet access. Either may have contributed.",
                "Check your connection and make sure the engine is running before reconnecting.",
            ),
            (None, None) => (
                "Cause unclear",
                "The available observations did not identify a cause.",
                "Check official Aion notices for your region or copy this report for support.",
            ),
        };

        let mut observations = Vec::new();
        if let Some(time) = engine_exit {
            observations.push((time, "Stabilizer exited unexpectedly"));
        }
        if let Some(time) = internet_loss {
            observations.push((time, "Windows reported no internet access"));
        }
        match internet_now {
            Some((time, true)) => observations.push((time, "Windows reported internet access")),
            None if internet_loss.is_none() => {
                observations.push((at, "Current Windows connectivity status unavailable"));
            }
            _ => {}
        }
        observations.push((
            at,
            if !status.watch {
                "Disconnect marked · Aion detection off"
            } else if status.game_present {
                "Disconnect marked · Aion detected"
            } else {
                "Disconnect marked · Aion not detected"
            },
        ));
        observations.sort_by_key(|&(time, _)| time);
        let events = observations
            .into_iter()
            .map(|(time, description)| {
                let seconds = at.duration_since(time).as_secs();
                Event {
                    time: if seconds == 0 {
                        "Now".into()
                    } else {
                        format!("−{seconds} s")
                    },
                    description,
                }
            })
            .collect();

        Self {
            finding,
            explanation,
            next_step,
            events,
            has_warning: engine_exit.is_some() || internet_loss.is_some(),
            marked_at: marked_at(),
        }
    }

    pub fn copy_text(&self) -> String {
        let mut text = format!(
            "Aion Stabilizer v{} — Disconnect report\n{}\n{}\n",
            env!("CARGO_PKG_VERSION"),
            self.finding,
            self.explanation,
        );
        if let Some(marked_at) = &self.marked_at {
            let _ = writeln!(text, "Marked at {marked_at}");
        }
        for event in &self.events {
            let _ = writeln!(text, "{}  {}", event.time, event.description);
        }
        let _ = writeln!(text, "{}", self.next_step);
        text
    }
}

fn marked_at() -> Option<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::SystemInformation::GetSystemTime;

        let mut time = SYSTEMTIME::default();
        unsafe { GetSystemTime(&mut time) };
        Some(format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond,
        ))
    }
    #[cfg(not(windows))]
    None
}

#[cfg(windows)]
pub use windows_monitor::Monitor;

#[cfg(windows)]
mod windows_monitor {
    use super::Connectivity;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::VARIANT_TRUE;
    use windows::Win32::Networking::NetworkListManager::{INetworkListManager, NetworkListManager};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    #[derive(Default)]
    pub struct Monitor(Arc<Mutex<Connectivity>>);

    impl Monitor {
        pub fn start() -> Self {
            let monitor = Self::default();
            let state = Arc::downgrade(&monitor.0);
            let _ = std::thread::Builder::new()
                .name("connectivity".into())
                .spawn(move || {
                    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                        return;
                    }
                    {
                        let network: windows::core::Result<INetworkListManager> =
                            unsafe { CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL) };
                        if let Ok(network) = network {
                            while let Some(state) = state.upgrade() {
                                let internet = unsafe { network.IsConnectedToInternet() }
                                    .ok()
                                    .map(|value| value == VARIANT_TRUE);
                                if let Ok(mut state) = state.lock() {
                                    state.observe(Instant::now(), internet);
                                }
                                drop(state);
                                std::thread::sleep(Duration::from_secs(2));
                            }
                        }
                    }
                    unsafe { CoUninitialize() };
                });
            monitor
        }

        pub fn snapshot(&self) -> Connectivity {
            self.0.lock().map(|state| *state).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_exit_with_recent_internet_access_is_visible_in_order() {
        let marked = Instant::now();
        let mut connectivity = Connectivity::default();
        connectivity.observe(marked - Duration::from_secs(2), Some(true));
        let status = Status {
            last_unexpected_engine_exit: Some(marked - Duration::from_secs(4)),
            watch: true,
            game_present: true,
            ..Status::default()
        };
        let report = DisconnectReport::capture(marked, &status, connectivity);
        assert_eq!(report.finding, "The stabilizer stopped");
        assert_eq!(report.events[0].time, "−4 s");
        assert!(report
            .copy_text()
            .contains("Windows reported internet access"));
        assert!(report.copy_text().contains("Aion detected"));
    }

    #[test]
    fn recent_network_loss_remains_visible_after_recovery() {
        let marked = Instant::now();
        let mut connectivity = Connectivity::default();
        connectivity.observe(marked - Duration::from_secs(8), Some(false));
        connectivity.observe(marked - Duration::from_secs(2), Some(true));
        let report = DisconnectReport::capture(marked, &Status::default(), connectivity);
        assert_eq!(report.finding, "Windows reported no internet access");
        assert_eq!(
            report.events[0].description,
            "Windows reported no internet access"
        );
        assert_eq!(
            report.events[1].description,
            "Windows reported internet access"
        );
    }

    #[test]
    fn stale_signals_do_not_assign_a_cause() {
        let marked = Instant::now();
        let old = marked - Duration::from_secs(31);
        let mut connectivity = Connectivity::default();
        connectivity.observe(old, Some(false));
        let status = Status {
            last_unexpected_engine_exit: Some(old),
            ..Status::default()
        };
        let report = DisconnectReport::capture(marked, &status, connectivity);
        assert_eq!(report.finding, "Cause unclear");
        assert!(report
            .copy_text()
            .contains("Current Windows connectivity status unavailable"));
    }
}
