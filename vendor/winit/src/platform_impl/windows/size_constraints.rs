//! Bound minimum tracking and initial size to the current monitor's usable area.
//! Inputs/outputs are physical pixels; non-client decorations are included.
pub(crate) fn cap_outer(size: [u32; 2], work: Option<[u32; 2]>) -> [u32; 2] {
    match work {
        Some([w, h]) if w > 0 && h > 0 => [size[0].min(w), size[1].min(h)],
        _ => size,
    }
}
pub(crate) fn cap_client(client: [u32; 2], outer: [u32; 2], work: Option<[u32; 2]>) -> [u32; 2] {
    let capped = cap_outer(outer, work);
    [
        client[0]
            .saturating_sub(outer[0].saturating_sub(capped[0]))
            .max(1),
        client[1]
            .saturating_sub(outer[1].saturating_sub(capped[1]))
            .max(1),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimum_and_startup_fit_work_area_across_scales() {
        for scale in [1.0_f64, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 8.0] {
            for work in [
                [1920, 1040],
                [1840, 1080],
                [2560, 1560],
                [640, 440],
                [320, 240],
            ] {
                let border = [(16.0 * scale).round() as u32, (39.0 * scale).round() as u32];
                for logical in [[360, 480], [360, 700], [420, 700]] {
                    let client = logical.map(|v| (v as f64 * scale).round() as u32);
                    let outer = [client[0] + border[0], client[1] + border[1]];
                    let track = cap_outer(outer, Some(work));
                    assert!(track[0] <= work[0] && track[1] <= work[1]);
                    let initial = cap_client(client, outer, Some(work));
                    for axis in 0..2 {
                        assert!(initial[axis] <= client[axis]);
                        if border[axis] < work[axis] {
                            assert!(initial[axis] + border[axis] <= work[axis]);
                        }
                        assert_eq!(track[axis], outer[axis].min(work[axis]));
                    }
                }
            }
        }
    }
    #[test]
    fn missing_invalid_and_tiny_work_area_are_safe() {
        for work in [None, Some([0, 100]), Some([100, 0])] {
            assert_eq!(cap_outer([376, 519], work), [376, 519]);
            assert_eq!(cap_client([360, 480], [376, 519], work), [360, 480]);
        }
        assert_eq!(cap_client([360, 480], [376, 519], Some([1, 1])), [1, 1]);
    }
    #[test]
    fn monitor_changes_do_not_reuse_previous_cap() {
        let requested = [916, 1298];
        assert_eq!(cap_outer(requested, Some([800, 600])), [800, 600]);
        assert_eq!(cap_outer(requested, Some([2560, 1400])), requested);
    }
}

/// Preserve an OS-suggested size except where it undershoots the allowed minimum.
pub(crate) fn floor_dpi_size(size: [u32; 2], minimum: [u32; 2], work: Option<[u32; 2]>) -> [u32; 2] {
    let minimum = cap_outer(minimum, work);
    [size[0].max(minimum[0]), size[1].max(minimum[1])]
}
#[cfg(test)]
mod dpi_floor_tests {
    use super::*;
    #[test]
    fn return_trip_undershoot_is_floored_without_shrinking_larger_windows() {
        assert_eq!(floor_dpi_size([372,735], [376,739], Some([1920,1040])), [376,739]);
        assert_eq!(floor_dpi_size([1000,900], [376,739], Some([1920,1040])), [1000,900]);
        assert_eq!(floor_dpi_size([300,200], [376,739], Some([320,240])), [320,240]);
        assert_eq!(floor_dpi_size([372,735], [376,739], None), [376,739]);
    }
}
