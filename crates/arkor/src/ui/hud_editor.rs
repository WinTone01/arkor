//! MangoHud layout editor, shared by the global settings page and per-game "Custom" mode.
//! Every control calls `Library::edit_hud` (or `edit_hud_quiet` for text fields), which
//! applies the change to whichever HUD is being edited and saves it immediately.

use arkor_core::mangohud::{FONT_SIZE_RANGE, HudSettings, Metric, Orientation, Position, Preset};
use gpui::{AnyElement, Context, Div, div, prelude::*, px, rgb, rgba};

use super::Library;
use super::widgets::{self, theme::*};

use Metric::*;

/// Metric groups shown in the editor; together they cover `Metric::ALL`.
const GROUPS: [&[Metric]; 4] = [
    &[
        GpuStats,
        GpuTemp,
        GpuJunctionTemp,
        GpuMemTemp,
        GpuCoreClock,
        GpuMemClock,
        GpuPower,
        GpuPowerLimit,
        GpuVoltage,
        GpuFan,
        ThrottlingStatus,
        ThrottlingStatusGraph,
        Vram,
    ],
    &[
        CpuStats,
        CpuTemp,
        CpuMhz,
        CpuPower,
        CoreLoad,
        CoreBars,
        Ram,
        Swap,
        Procmem,
        IoRead,
        IoWrite,
        Network,
        Fan,
        Battery,
        BatteryWatt,
        BatteryTime,
    ],
    &[
        Fps,
        Frametime,
        FrameTiming,
        FrameTimingDetailed,
        FrameCount,
        ShowFpsLimit,
    ],
    &[
        Resolution,
        RefreshRate,
        PresentMode,
        Fsr,
        Hdr,
        Wine,
        Winesync,
        EngineVersion,
        VulkanDriver,
        GpuName,
        Arch,
        DisplayServer,
        Gamemode,
        Vkbasalt,
        MediaPlayer,
        Fcat,
        Version,
        Time,
    ],
];

/// Colour keys and MangoHud's defaults (shown as hints).
const COLORS: [(&str, &str); 13] = [
    ("text_color", "FFFFFF"),
    ("background_color", "020202"),
    ("gpu_color", "2E9762"),
    ("cpu_color", "2E97CB"),
    ("vram_color", "AD64C1"),
    ("ram_color", "C26693"),
    ("engine_color", "EB5B5B"),
    ("frametime_color", "00FF00"),
    ("wine_color", "EB5B5B"),
    ("battery_color", "FF9078"),
    ("io_color", "A491D3"),
    ("network_color", "E07B85"),
    ("media_player_color", "FFFFFF"),
];

const KEYS: [(&str, &str); 6] = [
    ("toggle_hud", "Shift_R+F12"),
    ("toggle_fps_limit", "Shift_L+F1"),
    ("toggle_logging", "Shift_L+F2"),
    ("reload_cfg", "Shift_L+F4"),
    ("toggle_hud_position", "Shift_R+F11"),
    ("toggle_preset", "Shift_R+F10"),
];

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches('#'), 16)
        .ok()
        .filter(|_| s.trim().trim_start_matches('#').len() == 6)
}

impl Library {
    fn metric_label(&self, m: Metric) -> &'static str {
        match m {
            GpuStats | CpuStats => self.tr("Usage", "Kullanım"),
            GpuTemp | CpuTemp => self.tr("Temperature", "Sıcaklık"),
            GpuJunctionTemp => self.tr("Junction temp", "Junction sıcaklığı"),
            GpuMemTemp => self.tr("Memory temp", "Bellek sıcaklığı"),
            GpuCoreClock => self.tr("Core clock", "Çekirdek saati"),
            GpuMemClock => self.tr("Memory clock", "Bellek saati"),
            GpuPower | CpuPower => self.tr("Power", "Güç"),
            GpuPowerLimit => self.tr("Power limit", "Güç limiti"),
            GpuVoltage => self.tr("Voltage", "Voltaj"),
            GpuFan => self.tr("Fan", "Fan"),
            ThrottlingStatus => self.tr("Throttling", "Kısma"),
            ThrottlingStatusGraph => self.tr("Throttling graph", "Kısma grafiği"),
            Vram => "VRAM",
            CpuMhz => self.tr("Frequency", "Frekans"),
            CoreLoad => self.tr("Per-core load", "Çekirdek başına yük"),
            CoreBars => self.tr("Core load as bars", "Çekirdek yükü çubuk"),
            Ram => "RAM",
            Swap => "Swap",
            Procmem => self.tr("Process memory", "Süreç belleği"),
            IoRead => self.tr("Disk read", "Disk okuma"),
            IoWrite => self.tr("Disk write", "Disk yazma"),
            Network => self.tr("Network", "Ağ"),
            Fan => self.tr("System fan", "Sistem fanı"),
            Battery => self.tr("Battery", "Pil"),
            BatteryWatt => self.tr("Battery watts", "Pil watt"),
            BatteryTime => self.tr("Battery time left", "Kalan pil süresi"),
            Fps => "FPS",
            Frametime => "Frametime (ms)",
            FrameTiming => self.tr("Frametime graph", "Frametime grafiği"),
            FrameTimingDetailed => self.tr("Detailed frametime", "Ayrıntılı frametime"),
            FrameCount => self.tr("Frame count", "Kare sayacı"),
            ShowFpsLimit => self.tr("FPS limit in use", "Etkin FPS limiti"),
            Resolution => self.tr("Resolution", "Çözünürlük"),
            RefreshRate => self.tr("Refresh rate", "Yenileme hızı"),
            PresentMode => self.tr("Present mode", "Present modu"),
            Fsr => "FSR",
            Hdr => "HDR",
            Wine => self.tr("Wine/Proton version", "Wine/Proton sürümü"),
            Winesync => self.tr("Wine sync", "Wine senkronizasyonu"),
            EngineVersion => self.tr("DXVK/VKD3D version", "DXVK/VKD3D sürümü"),
            VulkanDriver => self.tr("Vulkan driver", "Vulkan sürücüsü"),
            GpuName => self.tr("GPU name", "GPU adı"),
            Arch => self.tr("Architecture", "Mimari"),
            DisplayServer => self.tr("Display server", "Görüntü sunucusu"),
            Gamemode => "GameMode",
            Vkbasalt => "vkBasalt",
            MediaPlayer => self.tr("Media player", "Medya oynatıcı"),
            Fcat => "FCAT",
            Version => self.tr("MangoHud version", "MangoHud sürümü"),
            Time => self.tr("Clock", "Saat"),
        }
    }

    /// Like `edit_hud`, without a redraw (for text fields, which redraw themselves).
    fn edit_hud_quiet(&mut self, f: impl FnOnce(&mut HudSettings)) {
        match self.selection {
            Some(super::Selection::Global) => f(&mut self.global.mangohud.settings),
            Some(super::Selection::Game(_)) => {
                let global = &self.global.mangohud.settings;
                f(self
                    .profile
                    .mangohud
                    .settings
                    .get_or_insert_with(|| global.clone()));
            }
            _ => return,
        }
        self.save();
    }

    pub(super) fn render_hud_editor(&mut self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let mut editor = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.presets(hud, cx))
            .child(
                widgets::section(self.t().position_section).child(
                    div()
                        .flex()
                        .gap_6()
                        .items_start()
                        .child(self.preview(hud))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_4()
                                .child(self.position_grid(hud, cx))
                                .child(self.orientation(hud, cx)),
                        ),
                ),
            )
            .child(self.steppers(hud, cx))
            .child(self.metrics(hud, cx));
        let open = self.hud_advanced;
        editor = editor.child(
            div().flex().child(
                widgets::chip(
                    "hud-advanced",
                    if open {
                        self.tr("Fewer MangoHud settings", "Daha az MangoHud ayarı")
                    } else {
                        self.tr("More MangoHud settings…", "Daha fazla MangoHud ayarı…")
                    },
                    open,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.hud_advanced = !this.hud_advanced;
                    cx.notify();
                })),
            ),
        );
        if open {
            editor = editor
                .child(self.hud_look(hud, cx))
                .child(self.hud_colors(hud, cx))
                .child(self.hud_keys(cx))
                .child(self.hud_limits(hud, cx))
                .child(self.hud_text(cx))
                .child(self.hud_logging(hud, cx))
                .child(self.hud_blacklist(hud, cx));
        }
        editor
    }

    fn presets(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let current = hud.matching_preset();
        let mut row = div().flex().flex_wrap().gap_2();
        for (ix, preset) in Preset::ALL.into_iter().enumerate() {
            row = row.child(
                widgets::chip(
                    ("preset", ix),
                    self.t().preset(preset),
                    current == Some(preset) && hud.get("preset").is_none(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit_hud(cx, |h| {
                        h.apply_preset(preset);
                        h.set("preset", None);
                    })
                })),
            );
        }
        row = row.child(widgets::badge(self.t().custom_badge, current.is_none()));
        // MangoHud's built-in Steam Deck presets (`preset=N`) replace the layout entirely.
        let deck = hud.get("preset").map(str::to_string);
        let mut deck_row = div().flex().flex_wrap().gap_2();
        for n in 1..=4u8 {
            let value = n.to_string();
            deck_row = deck_row.child(
                widgets::chip(
                    ("deck-preset", n as usize),
                    format!("Steam Deck {n}"),
                    deck.as_deref() == Some(value.as_str()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let v = n.to_string();
                    this.edit_hud(cx, |h| {
                        let same = h.get("preset") == Some(v.as_str());
                        h.set("preset", if same { None } else { Some(&v) });
                    })
                })),
            );
        }
        widgets::section(self.t().presets_section)
            .child(row)
            .child(widgets::label(self.tr(
                "MangoHud's Steam Deck presets (override the layout below):",
                "MangoHud'un Steam Deck preset'leri (aşağıdaki düzenin yerine geçer):",
            )))
            .child(deck_row)
    }

    fn position_grid(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_col().gap_1();
        for row in 0..3u8 {
            let mut line = div().flex().gap_1();
            for col in 0..3u8 {
                let cell = div().w(px(34.)).h(px(22.)).rounded_sm();
                line = line.child(match Position::from_cell(row, col) {
                    Some(pos) => {
                        let active = hud.position == pos;
                        cell.id(("pos", (row * 3 + col) as usize))
                            .cursor_pointer()
                            .border_1()
                            .border_color(rgb(if active { ACCENT() } else { BORDER() }))
                            .bg(rgb(if active { ACCENT() } else { BG() }))
                            .when(!active, |d| d.hover(|s| s.bg(rgb(ROW_HOVER()))))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.edit_hud(cx, |h| h.position = pos)
                            }))
                            .into_any_element()
                    }
                    // The centre is not a MangoHud position: leave a hole.
                    None => cell.into_any_element(),
                });
            }
            grid = grid.child(line);
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(widgets::label(self.t().screen_position))
            .child(grid)
    }

    fn orientation(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let opt = |ix: usize, o: Orientation, text: &'static str, cx: &mut Context<Self>| {
            widgets::chip(("orient", ix), text, hud.orientation == o).on_click(
                cx.listener(move |this, _, _, cx| this.edit_hud(cx, |h| h.orientation = o)),
            )
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(widgets::label(self.t().orientation))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(opt(0, Orientation::Vertical, self.t().vertical, cx))
                    .child(opt(1, Orientation::Horizontal, self.t().horizontal, cx)),
            )
    }

    fn steppers(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let btn = |id: &'static str,
                   glyph: &'static str,
                   f: fn(&mut HudSettings),
                   cx: &mut Context<Self>| {
            widgets::step_button(id, glyph)
                .on_click(cx.listener(move |this, _, _, cx| this.edit_hud(cx, f)))
        };
        let t = self.t();
        let fps = if hud.fps_limit == 0 {
            t.unlimited.to_string()
        } else {
            hud.fps_limit.to_string()
        };
        let (min_font, max_font) = FONT_SIZE_RANGE;
        widgets::section(t.size_section).child(
            div()
                .flex()
                .flex_wrap()
                .gap_6()
                .child(widgets::stepper(
                    t.font_size,
                    format!("{} px", hud.font_size),
                    btn("font-", "−", |h| h.step_font_size(false), cx)
                        .when(hud.font_size <= min_font, |d| d.opacity(0.4)),
                    btn("font+", "+", |h| h.step_font_size(true), cx)
                        .when(hud.font_size >= max_font, |d| d.opacity(0.4)),
                ))
                .child(widgets::stepper(
                    t.bg_opacity,
                    t.percent(hud.background_alpha),
                    btn("alpha-", "−", |h| h.step_alpha(false), cx),
                    btn("alpha+", "+", |h| h.step_alpha(true), cx),
                ))
                .child(widgets::stepper(
                    t.fps_limit,
                    fps,
                    btn("fps-", "−", |h| h.step_fps_limit(false), cx),
                    btn("fps+", "+", |h| h.step_fps_limit(true), cx),
                )),
        )
    }

    fn metrics(&self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let titles = [
            "GPU",
            self.tr("CPU & memory", "CPU ve bellek"),
            self.t().group_frame,
            self.t().group_other,
        ];
        let mut card = widgets::section(self.t().metrics_section);
        for (title, metrics) in titles.into_iter().zip(GROUPS) {
            let mut row = div().flex().flex_wrap().gap_2();
            for &m in metrics {
                row =
                    row.child(
                        widgets::chip(
                            ("metric", m as usize),
                            self.metric_label(m),
                            hud.metrics.contains(&m),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_hud(cx, |h| h.toggle_metric(m))
                        })),
                    );
            }
            card = card.child(
                div()
                    .flex()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .w(px(110.))
                            .pt_1()
                            .text_sm()
                            .text_color(rgb(TEXT()))
                            .child(title),
                    )
                    .child(row.flex_1()),
            );
        }
        card
    }

    fn flag_chip(
        &self,
        ix: usize,
        key: &'static str,
        label: &'static str,
        hud: &HudSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        widgets::chip(("hud-flag", ix), label, hud.flag(key))
            .on_click(cx.listener(move |this, _, _, cx| this.edit_hud(cx, |h| h.toggle_flag(key))))
    }

    /// A text field bound to a raw MangoHud key.
    fn key_field(
        &mut self,
        key: &'static str,
        placeholder: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<super::text_input::TextInput> {
        self.field(
            format!("hud-{key}"),
            move |this| {
                this.current_hud()
                    .and_then(|h| h.get(key).map(String::from))
                    .unwrap_or_default()
            },
            placeholder,
            cx,
            move |this, text| this.edit_hud_quiet(|h| h.set(key, Some(text))),
        )
    }

    fn current_hud(&self) -> Option<&HudSettings> {
        match self.selection {
            Some(super::Selection::Global) => Some(&self.global.mangohud.settings),
            Some(super::Selection::Game(_)) => self.profile.mangohud.settings.as_ref(),
            _ => None,
        }
    }

    fn hud_look(&mut self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let offset_x = self.key_field("offset_x", "0", cx);
        let offset_y = self.key_field("offset_y", "0", cx);
        let flags: [(&'static str, &'static str); 10] = [
            ("text_outline", self.tr("Text outline", "Metin kenarlığı")),
            ("hud_compact", self.tr("Compact", "Kompakt")),
            (
                "horizontal_stretch",
                self.tr("Stretch horizontally", "Yatay uzat"),
            ),
            (
                "hud_no_margin",
                self.tr("No outer margin", "Dış boşluk yok"),
            ),
            (
                "no_display",
                self.tr("Hidden until the toggle key", "Kısayol tuşuna kadar gizli"),
            ),
            ("temp_fahrenheit", "°F"),
            (
                "fps_color_change",
                self.tr("Colour FPS by value", "FPS'i değere göre renklendir"),
            ),
            (
                "gpu_load_change",
                self.tr("Colour GPU load", "GPU yükünü renklendir"),
            ),
            (
                "cpu_load_change",
                self.tr("Colour CPU load", "CPU yükünü renklendir"),
            ),
            (
                "engine_short_names",
                self.tr("Short engine names", "Kısa motor adları"),
            ),
        ];
        let mut chips = div().flex().flex_wrap().gap_2();
        for (ix, (key, label)) in flags.into_iter().enumerate() {
            chips = chips.child(self.flag_chip(ix, key, label, hud, cx));
        }
        let lows = hud.get("fps_metrics").is_some();
        chips = chips.child(
            widgets::chip(
                "hud-lows",
                self.tr("1% / 0.1% low FPS", "%1 / %0.1 düşük FPS"),
                lows,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.edit_hud(cx, |h| {
                    h.set(
                        "fps_metrics",
                        if lows { None } else { Some("avg,0.01,0.001") },
                    )
                })
            })),
        );
        let num = |hud: &HudSettings, key: &str, default: u32| {
            hud.get(key)
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(default)
        };
        let corners = num(hud, "round_corners", 0);
        let columns = num(hud, "table_columns", 3);
        let step = |id: &'static str,
                    glyph: &'static str,
                    key: &'static str,
                    delta: i32,
                    min: i32,
                    max: i32,
                    default: i32,
                    cx: &mut Context<Self>| {
            widgets::step_button(id, glyph).on_click(cx.listener(move |this, _, _, cx| {
                this.edit_hud(cx, |h| {
                    let v = h
                        .get(key)
                        .and_then(|v| v.parse::<i32>().ok())
                        .unwrap_or(default);
                    h.set(key, Some(&(v + delta).clamp(min, max).to_string()));
                })
            }))
        };
        let font = hud.get("font_file").map(String::from);
        widgets::section(self.tr("Look", "Görünüm"))
            .child(chips)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_6()
                    .items_end()
                    .child(widgets::stepper(
                        self.tr("Rounded corners", "Köşe yuvarlama"),
                        format!("{corners} px"),
                        step("corners-", "−", "round_corners", -2, 0, 20, 0, cx),
                        step("corners+", "+", "round_corners", 2, 0, 20, 0, cx),
                    ))
                    .child(widgets::stepper(
                        self.tr("Columns", "Sütun"),
                        columns.to_string(),
                        step("columns-", "−", "table_columns", -1, 1, 6, 3, cx),
                        step("columns+", "+", "table_columns", 1, 1, 6, 3, cx),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(widgets::label(
                                self.tr("Offset X / Y (px)", "Ofset X / Y (px)"),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().w(px(90.)).child(offset_x))
                                    .child(div().w(px(90.)).child(offset_y)),
                            ),
                    ),
            )
            .child(widgets::label(self.tr("Font file", "Yazı tipi dosyası")))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_sm()
                            .child(font.clone().unwrap_or_else(|| {
                                self.tr(
                                    "MangoHud's default font",
                                    "MangoHud'un varsayılan yazı tipi",
                                )
                                .into()
                            })),
                    )
                    .child(
                        widgets::chip("hud-font", self.tr("Choose…", "Seç…"), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                let title = this
                                    .tr("Choose a .ttf/.otf font", "Bir .ttf/.otf yazı tipi seç");
                                this.prompt_path(false, title, cx, |this, path, cx| {
                                    let p = path.display().to_string();
                                    this.edit_hud(cx, |h| h.set("font_file", Some(&p)));
                                });
                            }),
                        ),
                    )
                    .when(font.is_some(), |d| {
                        d.child(
                            widgets::chip(
                                "hud-font-clear",
                                self.tr("Default", "Varsayılan"),
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.edit_hud(cx, |h| h.set("font_file", None))
                            })),
                        )
                    }),
            )
    }

    fn hud_colors(&mut self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_wrap().gap_3();
        for (key, default) in COLORS {
            let input = self.key_field(key, default, cx);
            let shown = hud
                .get(key)
                .and_then(hex)
                .or_else(|| hex(default))
                .unwrap_or(0xffffff);
            grid = grid.child(
                div()
                    .w(px(250.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(widgets::label(
                        key.trim_end_matches("_color").replace('_', " "),
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(22.))
                                    .flex_none()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(rgb(BORDER()))
                                    .bg(rgb(shown)),
                            )
                            .child(div().flex_1().child(input)),
                    ),
            );
        }
        widgets::section(self.tr("Colours (hex, e.g. 2E9762)", "Renkler (hex, ör. 2E9762)"))
            .child(grid)
    }

    fn hud_keys(&mut self, cx: &mut Context<Self>) -> Div {
        let mut grid = div().flex().flex_wrap().gap_3();
        for (key, default) in KEYS {
            let input = self.key_field(key, default, cx);
            grid = grid.child(
                div()
                    .w(px(250.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(widgets::label(key.replace('_', " ")))
                    .child(input),
            );
        }
        widgets::section(self.tr("Keyboard shortcuts", "Klavye kısayolları"))
            .child(widgets::label(self.tr(
                "X11 key names joined with +, e.g. Shift_R+F12.",
                "+ ile birleştirilmiş X11 tuş adları, ör. Shift_R+F12.",
            )))
            .child(grid)
    }

    fn hud_limits(&mut self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let more = self.field(
            "hud-fps-more",
            |this| {
                this.current_hud()
                    .map(|h| {
                        h.fps_limit_more
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default()
            },
            "60,30,0",
            cx,
            |this, text| {
                let values: Vec<u32> = text
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect();
                this.edit_hud_quiet(|h| h.fps_limit_more = values);
            },
        );
        let chooser = |id: &'static str,
                       key: &'static str,
                       options: &[(&'static str, Option<&'static str>)],
                       hud: &HudSettings,
                       cx: &mut Context<Self>| {
            let current = hud.get(key).map(String::from);
            let mut row = div().flex().flex_wrap().gap_2();
            for (ix, (label, value)) in options.iter().enumerate() {
                let value = *value;
                row = row.child(
                    widgets::chip((id, ix), *label, current.as_deref() == value).on_click(
                        cx.listener(move |this, _, _, cx| this.edit_hud(cx, |h| h.set(key, value))),
                    ),
                );
            }
            row
        };
        let auto = self.tr("Default", "Varsayılan");
        widgets::section(self.tr("FPS limit & sync", "FPS limiti ve senkron"))
            .child(widgets::label(self.tr(
                "More limits to cycle with the FPS-limit key (after the main limit above), comma-separated; 0 = unlimited:",
                "FPS limiti tuşuyla geçilecek ek limitler (yukarıdaki ana limitten sonra), virgülle; 0 = sınırsız:",
            )))
            .child(more)
            .child(widgets::label(self.tr("Limiter", "Sınırlayıcı")))
            .child(chooser("limit-method", "fps_limit_method", &[(auto, None), ("late (smoother)", Some("late")), ("early (lower latency)", Some("early"))], hud, cx))
            .child(widgets::label("Vulkan VSync"))
            .child(chooser(
                "vsync",
                "vsync",
                &[(auto, None), ("adaptive", Some("0")), ("off", Some("1")), ("mailbox", Some("2")), ("on", Some("3"))],
                hud,
                cx,
            ))
            .child(widgets::label("OpenGL VSync"))
            .child(chooser("gl-vsync", "gl_vsync", &[(auto, None), ("adaptive", Some("-1")), ("off", Some("0")), ("on", Some("1"))], hud, cx))
    }

    fn hud_text(&mut self, cx: &mut Context<Self>) -> Div {
        let time_format = self.key_field("time_format", "%T", cx);
        let custom = self.key_field(
            "custom_text",
            self.tr("Text shown in the HUD", "HUD'da gösterilecek metin"),
            cx,
        );
        let exec = self.key_field("exec", "cat /sys/class/power_supply/BAT0/capacity", cx);
        widgets::section(self.tr("Text", "Metin"))
            .child(widgets::label(self.tr(
                "Clock format (strftime: %T, %R, %I:%M %p)",
                "Saat biçimi (strftime: %T, %R, %I:%M %p)",
            )))
            .child(time_format)
            .child(widgets::label(self.tr("Custom text", "Özel metin")))
            .child(custom)
            .child(widgets::label(self.tr(
                "Command whose output is shown",
                "Çıktısı gösterilecek komut",
            )))
            .child(exec)
    }

    fn hud_logging(&mut self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let folder = self.key_field("output_folder", "~/mangohud-logs", cx);
        let duration = self.key_field("log_duration", "30", cx);
        let interval = self.key_field("log_interval", "100", cx);
        widgets::section(self.tr("Benchmark logging", "Benchmark loglama"))
            .child(widgets::label(self.tr(
                "Press the logging key in game to record frametimes to CSV (seconds; interval in ms).",
                "Oyunda loglama tuşuna basınca frametime'lar CSV'ye kaydedilir (saniye; aralık ms).",
            )))
            .child(widgets::label(self.tr("Log folder", "Log klasörü")))
            .child(folder)
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(div().w(px(160.)).flex().flex_col().gap_1().child(widgets::label(self.tr("Duration", "Süre"))).child(duration))
                    .child(div().w(px(160.)).flex().flex_col().gap_1().child(widgets::label(self.tr("Interval", "Aralık"))).child(interval)),
            )
            .child(div().flex().child(self.flag_chip(20, "autostart_log", self.tr("Start logging with the game", "Loglamayı oyunla başlat"), hud, cx)))
    }

    fn hud_blacklist(&mut self, hud: &HudSettings, cx: &mut Context<Self>) -> Div {
        let add = self.submit_field(
            "hud-blacklist-add",
            self.tr("Process name, then Enter", "Süreç adı, sonra Enter"),
            cx,
            |this, text, cx| {
                let name = text.trim().to_string();
                if !name.is_empty() {
                    this.edit_hud(cx, |h| {
                        let mut list: Vec<String> = h
                            .get("blacklist")
                            .unwrap_or("")
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if !list.contains(&name) {
                            list.push(name);
                        }
                        h.set("blacklist", Some(&list.join(",")));
                    });
                }
            },
        );
        let list: Vec<String> = hud
            .get("blacklist")
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let mut chips = div().flex().flex_wrap().gap_2();
        for (ix, name) in list.into_iter().enumerate() {
            chips = chips.child(
                widgets::chip(("blacklist", ix), format!("{name}  ✕"), false).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let name = name.clone();
                        this.edit_hud(cx, |h| {
                            let rest: Vec<String> = h
                                .get("blacklist")
                                .unwrap_or("")
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty() && *s != name)
                                .collect();
                            h.set("blacklist", Some(&rest.join(",")));
                        })
                    }),
                ),
            );
        }
        widgets::section(self.tr(
            "Programs MangoHud ignores",
            "MangoHud'un yok saydığı programlar",
        ))
        .child(chips)
        .child(add)
    }

    /// A scaled-down mock of the game screen with the HUD where MangoHud will draw it.
    /// Values are placeholders; the point is position, orientation, size, colours and content.
    fn preview(&self, hud: &HudSettings) -> Div {
        let has = |m: Metric| hud.metrics.contains(&m);
        let color = |key: &str, default: u32| hud.get(key).and_then(hex).unwrap_or(default);
        let scale = hud.font_size as f32 * 0.42;

        let mut rows: Vec<(&str, u32, Vec<&str>)> = Vec::new();
        let pick = |list: &[(Metric, &'static str)]| {
            list.iter()
                .filter(|(m, _)| has(*m))
                .map(|(_, v)| *v)
                .collect::<Vec<_>>()
        };
        let gpu = pick(&[
            (GpuStats, "45%"),
            (GpuTemp, "62°C"),
            (GpuJunctionTemp, "71°C"),
            (GpuCoreClock, "2610MHz"),
            (GpuPower, "220W"),
            (GpuFan, "41%"),
        ]);
        if !gpu.is_empty() {
            rows.push(("GPU", color("gpu_color", 0x2e9762), gpu));
        }
        if has(Vram) {
            rows.push(("VRAM", color("vram_color", 0xad64c1), vec!["7.1GiB"]));
        }
        let cpu = pick(&[
            (CpuStats, "23%"),
            (CpuTemp, "58°C"),
            (CpuMhz, "4850MHz"),
            (CpuPower, "65W"),
        ]);
        if !cpu.is_empty() {
            rows.push(("CPU", color("cpu_color", 0x2e97cb), cpu));
        }
        if has(Ram) {
            rows.push(("RAM", color("ram_color", 0xc26693), vec!["11.4GiB"]));
        }
        if has(Battery) {
            rows.push(("BATT", color("battery_color", 0xff9078), vec!["87%"]));
        }
        let frame = pick(&[(Fps, "144 FPS"), (Frametime, "6.9ms")]);
        if !frame.is_empty() {
            rows.push(("", 0xffffff, frame));
        }
        for (m, l, v) in [
            (Resolution, "RES", "2560x1440"),
            (Wine, "WINE", "Proton 11"),
            (EngineVersion, "DXVK", "2.7"),
            (Time, "", "21:45"),
        ] {
            if has(m) {
                rows.push((l, color("engine_color", 0xeb5b5b), vec![v]));
            }
        }
        let text = color("text_color", 0xffffff);

        let horizontal = hud.orientation == Orientation::Horizontal;
        let mut body = div()
            .flex()
            .gap(px(scale * 0.6))
            .when(!horizontal, |d| d.flex_col())
            .when(horizontal, |d| d.items_center());
        for (label, c, values) in rows {
            let mut line = div().flex().gap(px(scale * 0.5)).items_center();
            if !label.is_empty() {
                line = line.child(div().text_color(rgb(c)).child(label));
            }
            for v in values {
                line = line.child(div().text_color(rgb(text)).child(v));
            }
            body = body.child(line);
        }
        if has(FrameTiming) {
            body = body.child(frametime_graph(
                scale,
                horizontal,
                color("frametime_color", 0x00ff00),
            ));
        }

        let alpha = (hud.background_alpha * 255 / 100) & 0xff;
        let bg = (color("background_color", 0x020202) << 8) | alpha;
        let corners = hud
            .get("round_corners")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.)
            * 0.42;
        let hud_box: AnyElement = if hud.metrics.is_empty() || hud.flag("no_display") {
            div().into_any_element()
        } else {
            div()
                .p(px(scale * 0.4))
                .rounded(px(corners))
                .bg(rgba(bg))
                .text_size(px(scale))
                .line_height(px(scale * 1.25))
                .child(body)
                .into_any_element()
        };

        let (row, col) = hud.position.cell();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(widgets::label(self.t().preview))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(384.))
                    .h(px(216.))
                    .p(px(6.))
                    .rounded_md()
                    .overflow_hidden()
                    .bg(rgb(0x2a3b4f))
                    .border_1()
                    .border_color(rgb(BORDER()))
                    .map(|d| match row {
                        0 => d.justify_start(),
                        1 => d.justify_center(),
                        _ => d.justify_end(),
                    })
                    .map(|d| match col {
                        0 => d.items_start(),
                        1 => d.items_center(),
                        _ => d.items_end(),
                    })
                    .child(hud_box),
            )
    }
}

fn frametime_graph(scale: f32, horizontal: bool, color: u32) -> Div {
    const HEIGHTS: [f32; 24] = [
        0.3, 0.35, 0.3, 0.4, 0.3, 0.32, 0.9, 0.3, 0.28, 0.35, 0.3, 0.33, 0.3, 0.45, 0.3, 0.3, 0.31,
        0.6, 0.3, 0.29, 0.34, 0.3, 0.3, 0.36,
    ];
    let h = scale * 1.6;
    let mut graph =
        div()
            .flex()
            .items_end()
            .h(px(h))
            .w(px(if horizontal { scale * 5. } else { scale * 9. }));
    for f in HEIGHTS {
        graph = graph.child(div().flex_1().h(px(h * f)).bg(rgb(color)));
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_cover_every_metric_once() {
        let mut all: Vec<Metric> = GROUPS.iter().flat_map(|g| g.iter().copied()).collect();
        all.sort();
        let mut expected = Metric::ALL.to_vec();
        expected.sort();
        assert_eq!(all, expected);
    }
}
