//! Unpublished test instrumentation. Bounded queue, background I/O, ten-minute cap.
use std::{
    collections::BTreeMap,
    io::Write,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, OnceLock,
    },
    time::{Duration, Instant},
};
static SENDER: OnceLock<Option<mpsc::SyncSender<Event>>> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);
pub fn mode() -> &'static str {
    static MODE: OnceLock<&'static str> = OnceLock::new();
    MODE.get_or_init(|| {
        if std::env::args().any(|a| a == "--cpu-gdi-direct") {
            "direct"
        } else {
            "bitblt"
        }
    })
}
#[derive(Default)]
pub struct Frame {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub total_us: u64,
    pub pixels_per_point: f32,
    pub texture_us: u64,
    pub raster_us: u64,
    pub present_us: u64,
    pub prepare_us: u64,
    pub copy_us: u64,
    pub blit_us: u64,
    pub flush_us: u64,
}
enum Event {
    Phase(Sample),
    Frame(Frame),
}
pub fn frame(frame: Frame) {
    if let Some(tx) = sender() {
        if tx.try_send(Event::Frame(frame)).is_err() {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
}
struct Sample {
    name: &'static str,
    us: u64,
    width: u32,
    height: u32,
}
#[derive(Default)]
struct Stats {
    count: u64,
    sum: u64,
    max: u64,
    over16: u64,
    over50: u64,
    width: u32,
    height: u32,
}
fn sender() -> Option<&'static mpsc::SyncSender<Event>> {
    SENDER.get_or_init(|| {
        if !std::env::args().any(|a| a == "--cpu-metrics") || std::env::args().any(|a| a == "--cpu-no-metrics") { return None; }
        let path = std::env::current_exe().ok()?.with_file_name(format!("CPU-GDI-{}-timings.csv", mode()));
        let frames_path = path.with_file_name(format!("CPU-GDI-{}-frames.csv", mode()));
        let (tx, rx) = mpsc::sync_channel::<Event>(8192);
        std::thread::Builder::new().name("cpu-gdi-metrics".into()).spawn(move || {
            let Ok(file) = std::fs::File::create(path) else { return; };
            let mut out = std::io::BufWriter::new(file);
            let Ok(file) = std::fs::File::create(frames_path) else { return; };
            let mut frames = std::io::BufWriter::new(file);
            if writeln!(frames, "frame,width,height,total_us,texture_us,raster_us,present_us,prepare_us,copy_us,blit_us,flush_us,dropped_total,pixels_per_point").is_err() { return; }
            let mut frame_count = 0_u64;
            if writeln!(out, "second,phase,count,mean_us,max_us,over_16ms,over_50ms,last_width,last_height,dropped_total").is_err() { return; }
            let start = Instant::now();
            let mut next = start + Duration::from_secs(1);
            let mut stats: BTreeMap<&'static str, Stats> = BTreeMap::new();
            loop {
                match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(Event::Frame(f)) => {
                        // Bound file size even if a driver/event loop generates excessive frames.
                        if frame_count < 100_000 {
                            if writeln!(frames, "{},{},{},{},{},{},{},{},{},{},{},{},{}", f.id, f.width, f.height, f.total_us, f.texture_us, f.raster_us, f.present_us, f.prepare_us, f.copy_us, f.blit_us, f.flush_us, DROPPED.load(Ordering::Relaxed), f.pixels_per_point).is_err() { return; }
                            frame_count += 1;
                        } else { DROPPED.fetch_add(1, Ordering::Relaxed); }
                    }
                    Ok(Event::Phase(s)) => {
                        let a = stats.entry(s.name).or_default();
                        a.count += 1; a.sum += s.us; a.max = a.max.max(s.us);
                        a.over16 += u64::from(s.us > 16_667); a.over50 += u64::from(s.us > 50_000);
                        a.width = s.width; a.height = s.height;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if Instant::now() >= next {
                    let second = start.elapsed().as_secs();
                    for (phase, a) in &stats {
                        if writeln!(out, "{second},{phase},{},{},{},{},{},{},{},{}", a.count, a.sum / a.count, a.max, a.over16, a.over50, a.width, a.height, DROPPED.load(Ordering::Relaxed)).is_err() { return; }
                    }
                    if frames.flush().is_err() || out.flush().is_err() { return; }
                    stats.clear();
                    if second >= 600 { break; }
                    next = Instant::now() + Duration::from_secs(1);
                }
            }
        }).ok()?;
        Some(tx)
    }).as_ref()
}
pub struct Span {
    start: Instant,
    name: &'static str,
    width: u32,
    height: u32,
}
impl Span {
    pub fn new(name: &'static str, width: u32, height: u32) -> Self {
        let _ = sender();
        Self {
            start: Instant::now(),
            name,
            width,
            height,
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some(tx) = sender() {
            let s = Sample {
                name: self.name,
                us: self.start.elapsed().as_micros().min(u64::MAX as u128) as u64,
                width: self.width,
                height: self.height,
            };
            if tx.try_send(Event::Phase(s)).is_err() {
                DROPPED.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
