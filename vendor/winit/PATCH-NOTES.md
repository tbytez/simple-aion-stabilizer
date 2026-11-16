# winit 0.30.13: downstream Windows mixed-DPI backport

Copyright the winit contributors. Licensed under Apache-2.0; see LICENSE
in this source copy or winit-LICENSE.txt in the application's licenses folder.

Base: the published crates.io winit 0.30.13 archive (unmodified files are
retained), SHA-256:
`a6755fa58a9f8350bd1e472d4c3fcc25f824ec358933bba33306d0b63df5978d`.
Upstream source commit from the archive metadata:
`e9809ef54b18499bb4f2cac945719ecc2a61061b`.

This downstream modification backports the Windows build gate and suggested
rectangle behavior from merged upstream PR #4341 (merge 488c036):
https://github.com/rust-windowing/winit/pull/4341

The stable 0.30.13 handler applies a legacy monitor-nudging workaround on
all Windows versions. On Windows 11 that can cause a WM_DPICHANGED feedback
loop when dragging between 100% and 125% monitors, producing flicker and
runaway resizing. See https://github.com/rust-windowing/winit/issues/4600.

Changes in this copy:

- Expose the existing cached Windows build detector to sibling modules.
  Initialize dwOSVersionInfoSize correctly for RtlGetVersion, as required
  by https://learn.microsoft.com/en-us/windows/win32/devnotes/rtlgetversion.
  This prevents the legacy/modern selection from relying on an invalid
  structure-size input inherited from the original crate.
- Put the existing adjustment block behind a portable selection function.
  As in upstream #4341, use it only on known Windows builds below 22000;
  on Windows 11 and unknown builds, apply the Windows-suggested rectangle
  directly. Preserve the rest of the DPI notification and repaint path.
- Add dpi_resize_policy.rs with regression tests. The application includes
  this exact file in its host-side tests so the policy runs on macOS too.

The package cache marker .cargo-ok is omitted. Other original crate files,
including non-Windows backends, package metadata and license, are retained.
No runtime environment override or disabling of per-monitor DPI awareness
is involved. Remove the Cargo patch once a compatible published winit
release includes the upstream fix, then rerun native mixed-DPI drag testing.

Tests verify policy selection and exact preservation of supplied rectangles.
They do not simulate Windows message dispatch or prove native drag behavior
on the user's hardware. Test by hand across a real mixed-DPI boundary.
