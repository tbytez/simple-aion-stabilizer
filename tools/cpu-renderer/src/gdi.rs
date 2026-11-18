//! Present an already-rasterized opaque bitmap at native physical resolution.
use std::{io, mem::size_of};
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        GetDC, ReleaseDC, SetDIBitsToDevice, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HDC,
    },
};
struct Dc {
    hwnd: HWND,
    dc: HDC,
}
impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(self.hwnd, self.dc);
        }
    }
}
/// The caller must supply a live HWND belonging to its current UI thread.
/// The bitmap is BGRX, tightly packed, top-down, at physical window resolution.
///
/// # Safety
/// `hwnd` must identify a valid window for the duration of this call.
pub unsafe fn present(hwnd: HWND, size: [usize; 2], pixels: &[u32]) -> io::Result<()> {
    let [w, h] = size;
    if w == 0 || h == 0 {
        return Ok(());
    }
    if w > 16384 || h > 16384 || w.checked_mul(h) != Some(pixels.len()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid GDI buffer",
        ));
    }
    let dc = unsafe { GetDC(hwnd) };
    if dc.is_null() {
        return Err(io::Error::last_os_error());
    }
    let dc = Dc { hwnd, dc };
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    let rows = unsafe {
        SetDIBitsToDevice(
            dc.dc,
            0,
            0,
            w as u32,
            h as u32,
            0,
            0,
            0,
            h as u32,
            pixels.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
        )
    };
    // An entirely clipped/occluded DC may report zero scanlines without error.
    // GetLastError is not defined for this API; do not report stale errors.
    if rows < 0 {
        return Err(io::Error::other("GDI bitmap presentation failed"));
    }
    Ok(())
}

/// Retained top-down DIB. Deliberately !Send/!Sync: GDI objects stay on the UI thread.
#[derive(Default)]
pub struct Presenter {
    bitmap: Option<Bitmap>,
}
struct Bitmap {
    dc: HDC,
    bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    old: windows_sys::Win32::Graphics::Gdi::HGDIOBJ,
    bits: *mut u32,
    capacity: [usize; 2],
    monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR,
}
impl Drop for Bitmap {
    fn drop(&mut self) {
        use windows_sys::Win32::Graphics::Gdi::{DeleteDC, DeleteObject, SelectObject};
        // Every submission is flushed before returning; no outstanding GDI access.
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
#[derive(Default)]
pub struct PresentTimings {
    pub prepare_us: u64,
    pub copy_us: u64,
    pub blit_us: u64,
    pub flush_us: u64,
}
fn micros(start: std::time::Instant) -> u64 {
    start.elapsed().as_micros() as u64
}
impl Presenter {
    /// # Safety
    /// `hwnd` must be a live window owned by the calling UI thread.
    pub unsafe fn present(
        &mut self,
        hwnd: HWND,
        size: [usize; 2],
        pixels: &[u32],
    ) -> io::Result<PresentTimings> {
        use std::{ptr, time::Instant};
        use windows_sys::Win32::Graphics::Gdi::{
            BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush,
            MonitorFromWindow, SelectObject, MONITOR_DEFAULTTONEAREST, SRCCOPY,
        };
        let start = Instant::now();
        let [w, h] = size;
        if w == 0 || h == 0 {
            return Ok(PresentTimings::default());
        }
        if w > 16384
            || h > 16384
            || w.checked_mul(h) != Some(pixels.len())
            || pixels.len() > 32 * 1024 * 1024
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid GDI buffer",
            ));
        }
        let target = unsafe { GetDC(hwnd) };
        if target.is_null() {
            return Err(io::Error::last_os_error());
        }
        let target = Dc { hwnd, dc: target };
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if self
            .bitmap
            .as_ref()
            .is_none_or(|b| w > b.capacity[0] || h > b.capacity[1] || b.monitor != monitor)
        {
            let capacity = crate::dib_capacity(self.bitmap.as_ref().map(|b| b.capacity), size);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: capacity[0] as i32,
                    biHeight: -(capacity[1] as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let dc = unsafe { CreateCompatibleDC(target.dc) };
            if dc.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut bits = ptr::null_mut();
            let bitmap = unsafe {
                CreateDIBSection(
                    target.dc,
                    &info,
                    DIB_RGB_COLORS,
                    &mut bits,
                    ptr::null_mut(),
                    0,
                )
            };
            if bitmap.is_null() || bits.is_null() {
                let error = io::Error::last_os_error();
                unsafe {
                    if !bitmap.is_null() {
                        DeleteObject(bitmap);
                    }
                    DeleteDC(dc);
                }
                return Err(error);
            }
            let old = unsafe { SelectObject(dc, bitmap) };
            if old.is_null() || old as isize == -1 {
                let error = io::Error::last_os_error();
                unsafe {
                    DeleteObject(bitmap);
                    DeleteDC(dc);
                }
                return Err(error);
            }
            self.bitmap = Some(Bitmap {
                dc,
                bitmap,
                old,
                bits: bits.cast(),
                capacity,
                monitor,
            });
        }
        let mut timing = PresentTimings {
            prepare_us: micros(start),
            ..Default::default()
        };
        let bitmap = self.bitmap.as_ref().unwrap();
        let start = Instant::now();
        // Previous use was flushed. Copy only initialized visible rows, respecting
        // the retained DIB's larger stride; no scaling or dirty-region assumptions.
        let destination = unsafe {
            std::slice::from_raw_parts_mut(bitmap.bits, bitmap.capacity[0] * bitmap.capacity[1])
        };
        crate::copy_dib_rows(destination, bitmap.capacity[0], pixels, size);
        timing.copy_us = micros(start);
        let start = Instant::now();
        let ok = unsafe {
            BitBlt(
                target.dc, 0, 0, w as i32, h as i32, bitmap.dc, 0, 0, SRCCOPY,
            )
        };
        let error = (ok == 0).then(io::Error::last_os_error);
        timing.blit_us = micros(start);
        let start = Instant::now();
        // Required before CPU access/reallocation of a DIB used by batched GDI.
        // Include completion cost in this frame, never hide it in the next frame.
        let flushed = unsafe { GdiFlush() };
        timing.flush_us = micros(start);
        if let Some(error) = error {
            return Err(error);
        }
        if flushed == 0 {
            return Err(io::Error::other("GDI batch flush failed"));
        }
        Ok(timing)
    }
}
