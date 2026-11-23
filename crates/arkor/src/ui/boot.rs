//! The start-up screen: shown while the library's cover art is decoded (animated covers can
//! take a moment), so the window opens on a finished library instead of covers popping in.
//! The bar glides towards the real progress, a line says what is happening, and the screen
//! fades out once every cover is ready (after a few seconds at the latest).

use std::time::{Duration, Instant};

use gpui::{Context, Div, Resource, Window, div, prelude::*, px};

use super::Library;
use super::style::pal;

/// Longest the start-up screen stays, even if a cover never finishes.
const MAX_WAIT: Duration = Duration::from_secs(8);
/// How long the screen takes to fade out.
const FADE: Duration = Duration::from_millis(450);
/// Shortest time the screen is shown, so it does not flash by.
const MIN_SHOWN: Duration = Duration::from_millis(700);

/// State of the start-up screen.
pub(super) struct Boot {
    started: Instant,
    /// The bar as drawn (0..1), easing towards the real progress.
    shown: f32,
    /// When the fade-out began.
    fading: Option<Instant>,
}

impl Library {
    /// Starts the start-up screen (the real window only; tests open straight to the page).
    pub(super) fn start_boot(&mut self, _cx: &mut Context<Self>) {
        self.boot = Some(Boot {
            started: Instant::now(),
            shown: 0.,
            fading: None,
        });
    }

    /// While the start-up screen fades out: how far (0..1, eased), for the page to settle in.
    pub(super) fn boot_reveal(&self) -> Option<f32> {
        let boot = self.boot.as_ref()?;
        let k = (boot.fading?.elapsed().as_secs_f32() / FADE.as_secs_f32()).clamp(0., 1.);
        Some(1. - (1. - k).powi(3))
    }

    /// The start-up screen while covers load, `None` once it has faded out.
    pub(super) fn render_boot(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<Div>> {
        self.boot.as_ref()?;
        let paths: Vec<_> = self.covers.values().flatten().cloned().collect();
        let total = paths.len();
        // Asking for each cover starts its decode.
        let ready = paths
            .into_iter()
            .filter(|p| {
                window
                    .use_asset::<gpui::ImgResourceLoader>(&Resource::Path(p.as_path().into()), cx)
                    .is_some()
            })
            .count();
        let games = self.games.len();
        let boot = self.boot.as_mut()?;
        let elapsed = boot.started.elapsed();
        let timed_out = elapsed >= MAX_WAIT;
        // A little of the bar is for reading the library itself, the rest for the covers.
        let target = if total == 0 || timed_out {
            1.
        } else {
            0.1 + 0.9 * ready as f32 / total as f32
        };
        // Ease towards the target: never jump, never go back.
        boot.shown = (boot.shown + (target - boot.shown) * 0.14)
            .max(boot.shown + 0.004)
            .min(target);
        let filled = boot.shown >= 0.995;
        if filled && elapsed >= MIN_SHOWN && boot.fading.is_none() {
            boot.fading = Some(Instant::now());
        }
        let alpha = match boot.fading {
            Some(t) => {
                let k = t.elapsed().as_secs_f32() / FADE.as_secs_f32();
                if k >= 1. {
                    self.boot = None;
                    return None;
                }
                // Ease out: quick at first, settling softly.
                (1. - k).powi(2)
            }
            None => 1.,
        };
        let shown = boot.shown;
        // Keep drawing frames while the screen moves.
        window.request_animation_frame();

        let status = if filled {
            self.tr("Ready", "Hazır").to_string()
        } else if elapsed < Duration::from_millis(250) || total == 0 {
            format!(
                "{} · {games}",
                self.tr("Reading your library", "Kütüphane okunuyor")
            )
        } else {
            format!(
                "{} · {ready}/{total}",
                self.tr("Loading cover art", "Kapak resimleri getiriliyor")
            )
        };
        Some(
            div()
                .id("boot")
                .absolute()
                .inset_0()
                // Clicks do not reach the page underneath.
                .occlude()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_5()
                .bg(pal::bg())
                .opacity(alpha)
                .child(
                    gpui::svg()
                        .path(super::icons::MARK)
                        .size(px(72.))
                        .text_color(pal::accent_light()),
                )
                .child(
                    div()
                        .text_2xl()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Arkor"),
                )
                .child(
                    div()
                        .w(px(280.))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(super::downloads_page::progress_bar(Some(shown)))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .text_xs()
                                .text_color(pal::muted())
                                .child(status)
                                .child(format!("{:.0}%", shown * 100.)),
                        ),
                ),
        )
    }
}
