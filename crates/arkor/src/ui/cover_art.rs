//! Cover pictures, with animated ones (WebP, GIF) streamed: the first frame is decoded at
//! once, the following ones a few at a time while the cover is on screen. A long animation
//! therefore costs a handful of frames of memory, not all of them (gpui's own `img` decodes
//! every frame up front and keeps each one in video memory: over a gigabyte for a
//! half-minute cover).

use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gpui::{App, Bounds, Canvas, Pixels, RenderImage, Window, canvas, point, px, size};
use image::codecs::{gif::GifDecoder, webp::WebPDecoder};
use image::{AnimationDecoder, Frame, ImageFormat, RgbaImage};

/// Wider pictures are scaled down to this while decoding.
const MAX_WIDTH: u32 = 640;
/// Frames decoded ahead of the one shown.
const AHEAD: usize = 3;
/// A cover that was not drawn for this long stops decoding.
const IDLE: Duration = Duration::from_millis(1200);
/// Covers that may play at the same time; further ones show their first frame.
const MAX_PLAYING: usize = 16;

static PLAYING: AtomicUsize = AtomicUsize::new(0);
static START: LazyLock<Instant> = LazyLock::new(Instant::now);
static ART: LazyLock<Mutex<HashMap<PathBuf, Arc<Art>>>> = LazyLock::new(Default::default);
/// First frames are decoded by a few threads, in the order they are asked for.
/// A first frame to decode, and where it goes.
type Job = (PathBuf, Arc<Art>);
static LOADER: LazyLock<Mutex<Sender<Job>>> = LazyLock::new(|| {
    let (tx, rx) = channel::<Job>();
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..4 {
        let rx = rx.clone();
        std::thread::spawn(move || {
            loop {
                let job = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                let Ok((path, art)) = job else { return };
                let first = first_frame(&path);
                let mut inner = art.lock();
                match first {
                    Some((image, animated)) => {
                        inner.still = Some(image);
                        inner.animated = animated;
                    }
                    None => inner.failed = true,
                }
            }
        });
    }
    Mutex::new(tx)
});

#[derive(Default)]
struct Inner {
    still: Option<Arc<RenderImage>>,
    failed: bool,
    animated: bool,
    /// A thread is decoding frames.
    playing: bool,
    queue: VecDeque<(Arc<RenderImage>, Duration)>,
    current: Option<Arc<RenderImage>>,
    next_at: Option<Instant>,
    /// The frame before `current`; an element drawn earlier may still show it.
    last: Option<Arc<RenderImage>>,
}

#[derive(Default)]
struct Art {
    inner: Mutex<Inner>,
    /// When it was last drawn (milliseconds since `START`).
    drawn: AtomicU64,
}

impl Art {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn idle(&self) -> bool {
        let now = START.elapsed().as_millis() as u64;
        now.saturating_sub(self.drawn.load(Ordering::Relaxed)) > IDLE.as_millis() as u64
    }
}

fn art(path: &Path) -> Arc<Art> {
    let mut map = ART.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(a) = map.get(path) {
        return a.clone();
    }
    let a = Arc::new(Art::default());
    map.insert(path.to_path_buf(), a.clone());
    let _ = LOADER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send((path.to_path_buf(), a.clone()));
    a
}

/// Starts loading `path` and tells whether its picture is there (or will never be).
pub(super) fn ready(path: &Path) -> bool {
    let a = art(path);
    let inner = a.lock();
    inner.still.is_some() || inner.failed
}

/// The file at `path` was replaced or removed: read it again.
pub(super) fn forget(path: &Path) {
    ART.lock().unwrap_or_else(|e| e.into_inner()).remove(path);
}

/// Whether any cover is moving (the window then needs to redraw).
pub(super) fn any_playing() -> bool {
    PLAYING.load(Ordering::Relaxed) > 0
}

/// Scales down and turns RGBA into the BGRA gpui draws.
fn to_image(mut buf: RgbaImage) -> Arc<RenderImage> {
    if buf.width() > MAX_WIDTH {
        let h = (u64::from(buf.height()) * u64::from(MAX_WIDTH) / u64::from(buf.width())) as u32;
        buf = image::imageops::thumbnail(&buf, MAX_WIDTH, h.max(1));
    }
    for p in buf.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![Frame::new(buf)]))
}

/// The frames of an animation, decoded as they are asked for; `None` for a still picture.
fn frames<'a>(bytes: &'a [u8], format: ImageFormat) -> Option<image::Frames<'a>> {
    match format {
        ImageFormat::WebP => {
            let d = WebPDecoder::new(Cursor::new(bytes)).ok()?;
            d.has_animation().then(|| d.into_frames())
        }
        ImageFormat::Gif => Some(GifDecoder::new(Cursor::new(bytes)).ok()?.into_frames()),
        _ => None,
    }
}

/// The picture to show first, and whether more frames follow.
fn first_frame(path: &Path) -> Option<(Arc<RenderImage>, bool)> {
    let bytes = std::fs::read(path).ok()?;
    let format = image::guess_format(&bytes).ok()?;
    if let Some(mut frames) = frames(&bytes, format) {
        let first = frames.next()?.ok()?;
        return Some((to_image(first.into_buffer()), true));
    }
    let picture = image::load_from_memory_with_format(&bytes, format).ok()?;
    Some((to_image(picture.into_rgba8()), false))
}

/// Decodes frames into the queue until the cover is no longer drawn.
fn play(path: PathBuf, art: Arc<Art>) {
    struct Playing(Arc<Art>);
    impl Drop for Playing {
        fn drop(&mut self) {
            let mut inner = self.0.lock();
            inner.playing = false;
            inner.queue.clear();
            inner.next_at = None;
            PLAYING.fetch_sub(1, Ordering::Relaxed);
        }
    }
    let _playing = Playing(art.clone());
    let still = || art.lock().animated = false;
    let Ok(bytes) = std::fs::read(&path) else {
        return still();
    };
    let Ok(format) = image::guess_format(&bytes) else {
        return still();
    };
    loop {
        let Some(frames) = frames(&bytes, format) else {
            return still();
        };
        let mut count = 0;
        for frame in frames {
            // A damaged frame ends this round; the next one starts over.
            let Ok(frame) = frame else { break };
            count += 1;
            let (n, d) = frame.delay().numer_denom_ms();
            let delay = match n / d.max(1) {
                0..=10 => 100,
                ms => ms,
            };
            let image = to_image(frame.into_buffer());
            loop {
                if art.idle() {
                    return;
                }
                let mut inner = art.lock();
                if inner.queue.len() < AHEAD {
                    inner
                        .queue
                        .push_back((image, Duration::from_millis(delay.into())));
                    break;
                }
                drop(inner);
                std::thread::sleep(Duration::from_millis(6));
            }
        }
        if count < 2 {
            return still();
        }
    }
}

impl Inner {
    /// Moves to the frame due at `now`; returns the frames that are done with.
    fn advance(&mut self, now: Instant) -> Vec<Arc<RenderImage>> {
        let mut done = Vec::new();
        while self.next_at.is_none_or(|t| now >= t) {
            let Some((frame, delay)) = self.queue.pop_front() else {
                break;
            };
            // Keep the rhythm of the animation; after a pause start counting afresh.
            let base = self
                .next_at
                .filter(|t| now.saturating_duration_since(*t) < Duration::from_millis(200))
                .unwrap_or(now);
            self.next_at = Some(base + delay);
            let before = self.current.replace(frame);
            done.extend(std::mem::replace(&mut self.last, before));
        }
        done
    }
}

/// Where a picture of `w`×`h` is drawn so that it covers `bounds` (its overflow is cut off).
fn cover_fit(bounds: Bounds<Pixels>, w: f32, h: f32) -> Bounds<Pixels> {
    let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if w <= 0. || h <= 0. || bw <= 0. || bh <= 0. {
        return bounds;
    }
    let k = (bw / w).max(bh / h);
    let (iw, ih) = (w * k, h * k);
    Bounds::new(
        point(
            bounds.origin.x - px((iw - bw) / 2.),
            bounds.origin.y - px((ih - bh) / 2.),
        ),
        size(px(iw), px(ih)),
    )
}

/// Draws the cover at `path`. `squeeze` draws the whole picture into `bounds` even when that
/// narrows it (a card turned sideways) instead of cutting it to fit.
pub(super) fn paint(
    path: &Path,
    bounds: Bounds<Pixels>,
    radius: f32,
    animate: bool,
    squeeze: bool,
    window: &mut Window,
) {
    if !window.content_mask().bounds.intersects(&bounds) {
        return;
    }
    let art = art(path);
    let mut inner = art.lock();
    let Some(still) = inner.still.clone() else {
        if !inner.failed {
            window.request_animation_frame();
        }
        return;
    };
    let mut done = Vec::new();
    if inner.animated && animate {
        art.drawn
            .store(START.elapsed().as_millis() as u64, Ordering::Relaxed);
        if !inner.playing && PLAYING.load(Ordering::Relaxed) < MAX_PLAYING {
            inner.playing = true;
            PLAYING.fetch_add(1, Ordering::Relaxed);
            let (path, art) = (path.to_path_buf(), art.clone());
            std::thread::spawn(move || play(path, art));
        }
        done = inner.advance(Instant::now());
    }
    let image = inner.current.clone().unwrap_or(still);
    drop(inner);
    // Frames already shown leave video memory once this frame no longer uses them.
    for old in done {
        window.on_next_frame(move |window, _| {
            let _ = window.drop_image(old);
        });
    }
    let px_size = image.size(0);
    let place = if squeeze {
        bounds
    } else {
        cover_fit(bounds, px_size.width.0 as f32, px_size.height.0 as f32)
    };
    let _ = window.paint_image(bounds, place, px(radius).into(), image, 0, false);
}

/// A cover as an element; give it a size.
pub(super) fn cover(path: PathBuf, radius: f32, animate: bool) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window: &mut Window, _: &mut App| {
            let animate = animate && window.is_window_active();
            paint(&path, bounds, radius, animate, false, window);
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_covers_and_keeps_proportions() {
        let b = Bounds::new(point(px(10.), px(20.)), size(px(100.), px(150.)));
        assert_eq!(cover_fit(b, 480., 720.), b, "2:3 fills exactly");
        let wide = cover_fit(b, 300., 150.);
        assert_eq!(wide.size, size(px(300.), px(150.)));
        assert_eq!(wide.origin.x, px(-90.), "centred");
    }

    #[test]
    fn frames_advance_in_rhythm_and_free_the_old_ones() {
        let frame = || to_image(RgbaImage::new(2, 3));
        let ms = Duration::from_millis;
        let mut inner = Inner::default();
        for _ in 0..3 {
            inner.queue.push_back((frame(), ms(40)));
        }
        let t = Instant::now();
        assert!(inner.advance(t).is_empty(), "the first frame replaces none");
        assert!(inner.advance(t + ms(20)).is_empty(), "not due yet");
        assert!(
            inner.advance(t + ms(45)).is_empty(),
            "the one before is kept"
        );
        assert_eq!(
            inner.next_at,
            Some(t + ms(80)),
            "a late draw does not drift"
        );
        assert_eq!(inner.advance(t + ms(81)).len(), 1);
        assert!(
            inner.advance(t + ms(500)).is_empty(),
            "an empty queue keeps the frame"
        );
        assert!(inner.current.is_some());
    }

    #[test]
    fn a_still_picture_loads_and_a_missing_one_is_not_waited_for() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cover.png");
        RgbaImage::new(1200, 1800).save(&path).unwrap();
        let (image, animated) = first_frame(&path).unwrap();
        assert!(!animated);
        assert_eq!(image.size(0).width.0, MAX_WIDTH as i32, "scaled down");
        assert!(first_frame(&tmp.path().join("none.png")).is_none());
    }
}
