//! Backport of upstream #4341: legacy monitor nudging is only for Windows 10.
//! Keep this decision free of Win32 types so downstream host tests can exercise
//! the exact branch used by the native WM_DPICHANGED handler.

pub(crate) fn select_rect<T>(
    windows_build: Option<u32>,
    suggested: T,
    legacy_adjustment: impl FnOnce() -> T,
) -> T {
    if windows_build.is_some_and(|build| build < 22000) {
        legacy_adjustment()
    } else {
        suggested
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_windows_never_runs_monitor_nudging() {
        for build in [None, Some(22000), Some(22621), Some(26100), Some(26200)] {
            // Negative/staggered/vertical monitor layouts must be preserved
            // exactly, not re-anchored to the monitor we are leaving.
            for suggested in [
                [-400, 80, 20, 780],
                [10, -200, 535, 675],
                [200, -900, 725, -25],
                [-2800, -80, -2380, 620],
            ] {
                assert_eq!(
                    select_rect(build, suggested, || panic!(
                        "legacy correction on modern Windows"
                    )),
                    suggested
                );
            }
        }
    }

    #[test]
    fn windows_10_keeps_its_existing_adjustment() {
        let mut called = false;
        let result = select_rect(Some(19045), [0, 0, 525, 875], || {
            called = true;
            [10, 20, 535, 895]
        });
        assert!(called);
        assert_eq!(result, [10, 20, 535, 895]);
    }

    #[test]
    fn repeated_100_125_crossings_keep_os_size_without_growth() {
        // Representative outer rectangles crossing between the reported 2560x1440
        // 100% left monitor and 2560x1600 125% right monitor. The legacy
        // correction would push the rectangle back toward the old monitor.
        for _ in 0..256 {
            for suggested in [[-300, 100, 120, 800], [-150, 100, 375, 975]] {
                let actual = select_rect(Some(26100), suggested, || [-600, 100, -75, 975]);
                assert_eq!(actual, suggested);
                let width = actual[2] - actual[0];
                assert!(width == 420 || width == 525);
                assert_eq!(actual[3] - actual[1], if width == 420 { 700 } else { 875 });
            }
        }
    }
}
