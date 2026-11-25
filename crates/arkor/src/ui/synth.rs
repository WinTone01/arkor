//! Console mode's sounds, made here from nothing (no files): every one of them is a few notes
//! of the same scale (D major pentatonic) on a soft glass-like voice, so they fit together
//! whatever follows what. The start-up music follows the start-up animation's clock.

use std::f32::consts::{PI, TAU};

use super::console_intro::{ARRIVE, DIVE, IGNITE, PART_STEP, PARTS, TRACE, TRACE_SECS};

pub(super) const RATE: u32 = 48_000;
const SR: f32 = RATE as f32;

// Notes (Hz).
const D1: f32 = 36.71;
const D2: f32 = 73.42;
const A2: f32 = 110.;
const D3: f32 = 146.83;
const A3: f32 = 220.;
const D4: f32 = 293.66;
const E4: f32 = 329.63;
const FS4: f32 = 369.99;
const A4: f32 = 440.;
const B4: f32 = 493.88;
const D5: f32 = 587.33;
const E5: f32 = 659.26;
const FS5: f32 = 739.99;
const A5: f32 = 880.;
const B5: f32 = 987.77;
const D6: f32 = 1174.66;
const E6: f32 = 1318.51;
const FS6: f32 = 1479.98;
const A6: f32 = 1760.;

/// What can be heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Cue {
    /// The focus moved.
    Move,
    /// There is nothing further that way.
    Edge,
    Accept,
    Back,
    /// Another section.
    Tab,
    /// Something opens over the screen.
    Open,
    On,
    Off,
    /// A choice turned one step.
    Turn,
    Key,
    Erase,
    Error,
    Done,
    /// A game starts; it ended.
    Launch,
    Return,
    /// The start-up music, and its short form.
    Welcome,
    Hello,
    /// Console mode is left.
    Bye,
}

impl Cue {
    pub(super) const ALL: [Cue; 18] = [
        Cue::Move,
        Cue::Edge,
        Cue::Accept,
        Cue::Back,
        Cue::Tab,
        Cue::Open,
        Cue::On,
        Cue::Off,
        Cue::Turn,
        Cue::Key,
        Cue::Erase,
        Cue::Error,
        Cue::Done,
        Cue::Launch,
        Cue::Return,
        Cue::Welcome,
        Cue::Hello,
        Cue::Bye,
    ];

    /// How loud (the peak it is brought to). What is heard often is quiet.
    fn peak(self) -> f32 {
        match self {
            Cue::Move => 0.2,
            Cue::Key | Cue::Erase => 0.2,
            Cue::Turn => 0.24,
            Cue::Edge => 0.32,
            Cue::Back | Cue::Off => 0.32,
            Cue::Tab | Cue::On | Cue::Open => 0.36,
            Cue::Accept => 0.4,
            Cue::Error | Cue::Done | Cue::Return | Cue::Bye => 0.45,
            Cue::Launch | Cue::Hello => 0.75,
            Cue::Welcome => 0.9,
        }
    }
}

/// Stereo sound being made.
struct Pad {
    l: Vec<f32>,
    r: Vec<f32>,
    seed: u32,
}

impl Pad {
    fn new(secs: f32) -> Self {
        let n = (secs * SR) as usize;
        Pad {
            l: vec![0.; n],
            r: vec![0.; n],
            seed: 0x2545_f491,
        }
    }

    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2. - 1.
    }

    /// Adds `f(seconds since at)` for `secs`, at `pan` (-1 left … 1 right).
    fn add(&mut self, at: f32, secs: f32, pan: f32, mut f: impl FnMut(f32) -> f32) {
        let a = (pan.clamp(-1., 1.) + 1.) * PI / 4.;
        let (gl, gr) = (a.cos(), a.sin());
        let from = (at * SR) as usize;
        let n = (secs * SR) as usize;
        for i in 0..n {
            let Some(l) = self.l.get_mut(from + i) else {
                break;
            };
            let v = f(i as f32 / SR);
            *l += v * gl;
            self.r[from + i] += v * gr;
        }
    }

    /// A soft glass note: a sine with two quiet partials that die away sooner.
    fn glass(&mut self, at: f32, hz: f32, amp: f32, decay: f32, pan: f32) {
        self.add(at, decay * 6., pan, |t| {
            let attack = (t / 0.003).min(1.);
            let e = (-t / decay).exp();
            let body = (TAU * hz * t).sin();
            let second = (TAU * hz * 2.004 * t).sin() * 0.22 * (-t / (decay * 0.5)).exp();
            let bell = (TAU * hz * 3.98 * t).sin() * 0.1 * (-t / (decay * 0.16)).exp();
            (body + second + bell) * e * attack * amp
        });
    }

    /// A warm chord note: detuned saws through a low-pass that opens and closes.
    #[allow(clippy::too_many_arguments)]
    fn warm(
        &mut self,
        at: f32,
        secs: f32,
        hz: f32,
        amp: f32,
        pan: f32,
        cutoff: impl Fn(f32) -> f32,
        level: impl Fn(f32) -> f32,
    ) {
        for (k, det) in [0.996_f32, 1.0015, 1.0052].into_iter().enumerate() {
            let side = pan + (k as f32 - 1.) * 0.45;
            let (mut phase, mut a, mut b) = (0.13 * k as f32, 0., 0.);
            let hz = hz * det;
            let (cutoff, level) = (&cutoff, &level);
            self.add(at, secs, side, |t| {
                phase = (phase + hz / SR).fract();
                // The corner of the saw is rounded, or it would whistle.
                let dt = hz / SR;
                let mut saw = 2. * phase - 1.;
                if phase < dt {
                    let x = phase / dt;
                    saw -= x + x - x * x - 1.;
                } else if phase > 1. - dt {
                    let x = (phase - 1.) / dt;
                    saw -= x * x + x + x + 1.;
                }
                let c = 1. - (-TAU * cutoff(t).min(9000.) / SR).exp();
                a += (saw - a) * c;
                b += (a - b) * c;
                b * level(t) * amp
            });
        }
    }

    /// Noise through a moving band: air, sweeps, whooshes.
    fn air(
        &mut self,
        at: f32,
        secs: f32,
        pan: (f32, f32),
        centre: impl Fn(f32) -> f32,
        width: f32,
        level: impl Fn(f32) -> f32,
    ) {
        let n = (secs * SR) as usize;
        let from = (at * SR) as usize;
        let (mut low, mut band) = (0., 0.);
        for i in 0..n {
            let t = i as f32 / SR;
            let x = self.noise();
            // A state-variable filter.
            let g = 2. * (PI * centre(t).clamp(30., 9000.) / SR).sin();
            let high = x - low - width * band;
            band += g * high;
            low += g * band;
            let v = band * level(t) * width.sqrt();
            let p = pan.0 + (pan.1 - pan.0) * t / secs;
            let a = (p.clamp(-1., 1.) + 1.) * PI / 4.;
            let Some(l) = self.l.get_mut(from + i) else {
                break;
            };
            *l += v * a.cos();
            self.r[from + i] += v * a.sin();
        }
    }

    /// A deep hit: a sine falling from `from` to `to`.
    fn thump(&mut self, at: f32, from: f32, to: f32, amp: f32, decay: f32) {
        let mut phase = 0.;
        self.add(at, decay * 6., 0., |t| {
            let hz = to + (from - to) * (-t / 0.045).exp();
            phase += TAU * hz / SR;
            phase.sin() * (-t / decay).exp() * (t / 0.002).min(1.) * amp
        });
    }

    /// A room around it: eight crossed delay lines, darker with every turn.
    fn room(&mut self, wet: f32, seconds: f32) {
        const LINES: [usize; 8] = [1433, 1601, 1867, 2053, 2251, 2399, 2617, 2797];
        let mut lines: Vec<(Vec<f32>, usize, f32)> =
            LINES.iter().map(|n| (vec![0.; *n], 0, 0.)).collect();
        for i in 0..self.l.len() {
            let (inl, inr) = (self.l[i], self.r[i]);
            let outs: Vec<f32> = lines.iter().map(|(b, p, _)| b[*p]).collect();
            let sum: f32 = outs.iter().sum::<f32>() * 0.25;
            let (mut wl, mut wr) = (0., 0.);
            for (k, (buf, p, dark)) in lines.iter_mut().enumerate() {
                let gain = 10f32.powf(-3. * LINES[k] as f32 / SR / seconds);
                let back = (outs[k] - sum) * gain;
                *dark += (back - *dark) * 0.45;
                let dry = if k % 2 == 0 { inl } else { inr };
                buf[*p] = dry * 0.5 + *dark;
                *p = (*p + 1) % buf.len();
                if k % 2 == 0 {
                    wl += outs[k];
                } else {
                    wr += outs[k];
                }
            }
            self.l[i] = inl + wl * wet;
            self.r[i] = inr + wr * wet;
        }
    }

    /// Fuller: the loudest moments give way a little to the rest.
    fn press(&mut self, drive: f32) {
        let top = self
            .l
            .iter()
            .chain(&self.r)
            .fold(1e-6_f32, |m, v| m.max(v.abs()));
        for v in self.l.iter_mut().chain(&mut self.r) {
            *v = (*v / top * drive).tanh();
        }
    }

    /// Brought to its loudness, the ends without a click, left and right in turn.
    fn finish(mut self, peak: f32) -> Vec<f32> {
        // Slow movement nobody hears would only eat room.
        for ch in [&mut self.l, &mut self.r] {
            let (mut x1, mut y1) = (0., 0.);
            for v in ch.iter_mut() {
                let y = *v - x1 + 0.9975 * y1;
                (x1, y1) = (*v, y);
                *v = y;
            }
        }
        let top = self
            .l
            .iter()
            .chain(&self.r)
            .fold(1e-6_f32, |m, v| m.max(v.abs()));
        let k = peak / top;
        let n = self.l.len();
        let tail = (0.03 * SR) as usize;
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let f = ((n - i) as f32 / tail as f32).min(1.) * k;
            out.push(self.l[i] * f);
            out.push(self.r[i] * f);
        }
        out
    }
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    x * x * (3. - 2. * x)
}

/// The sound as stereo samples, left and right in turn.
pub(super) fn make(cue: Cue) -> Vec<f32> {
    let peak = cue.peak();
    let mut p;
    match cue {
        Cue::Move => {
            p = Pad::new(0.12);
            p.glass(0., D5, 0.8, 0.022, 0.);
            p.glass(0., A5, 0.25, 0.012, 0.);
        }
        Cue::Edge => {
            p = Pad::new(0.22);
            p.thump(0., 150., 92., 1., 0.035);
        }
        Cue::Accept => {
            p = Pad::new(0.75);
            p.glass(0., D5, 0.75, 0.07, -0.1);
            p.glass(0.05, A5, 1., 0.12, 0.1);
            p.room(0.22, 0.7);
        }
        Cue::Back => {
            p = Pad::new(0.6);
            p.glass(0., A4, 0.8, 0.06, 0.1);
            p.glass(0.05, D4, 1., 0.1, -0.1);
            p.room(0.18, 0.6);
        }
        Cue::Tab => {
            p = Pad::new(0.6);
            p.air(
                0.,
                0.16,
                (-0.5, 0.5),
                |t| 700. + 3600. * smooth(t / 0.14),
                0.5,
                |t| (PI * (t / 0.16)).sin().powi(2) * 0.5,
            );
            p.glass(0.04, E5, 0.8, 0.07, 0.);
            p.glass(0.04, B5, 0.3, 0.05, 0.);
            p.room(0.18, 0.5);
        }
        Cue::Open => {
            p = Pad::new(0.8);
            p.air(
                0.,
                0.26,
                (0., 0.),
                |t| 400. + 2600. * smooth(t / 0.24),
                0.6,
                |t| (PI * (t / 0.26)).sin().powi(2) * 0.35,
            );
            for (k, hz) in [A4, D5, FS5].into_iter().enumerate() {
                p.glass(0.035 * k as f32, hz, 0.8, 0.08, (k as f32 - 1.) * 0.3);
            }
            p.room(0.25, 0.8);
        }
        Cue::On => {
            p = Pad::new(0.6);
            p.glass(0., A5, 0.7, 0.04, 0.);
            p.glass(0.06, D6, 1., 0.09, 0.);
            p.room(0.18, 0.5);
        }
        Cue::Off => {
            p = Pad::new(0.5);
            p.glass(0., D5, 0.8, 0.04, 0.);
            p.glass(0.06, A4, 1., 0.07, 0.);
            p.room(0.14, 0.4);
        }
        Cue::Turn => {
            p = Pad::new(0.2);
            p.glass(0., FS5, 0.9, 0.035, 0.);
        }
        Cue::Key => {
            p = Pad::new(0.12);
            p.glass(0., B5, 0.8, 0.018, 0.);
            p.thump(0., 320., 190., 0.5, 0.012);
        }
        Cue::Erase => {
            p = Pad::new(0.14);
            p.glass(0., E5, 0.8, 0.022, 0.);
            p.thump(0., 260., 140., 0.6, 0.016);
        }
        Cue::Error => {
            p = Pad::new(0.9);
            for (at, hz) in [(0., 174.61), (0.13, 155.56)] {
                p.warm(
                    at,
                    0.4,
                    hz,
                    0.7,
                    0.,
                    |t| 1400. * (-t / 0.12).exp() + 250.,
                    |t| (t / 0.006).min(1.) * (-t / 0.11).exp(),
                );
                p.glass(at, hz * 2., 0.4, 0.07, 0.);
            }
            p.room(0.2, 0.6);
        }
        Cue::Done => {
            p = Pad::new(1.6);
            for (k, hz) in [D5, FS5, A5, D6].into_iter().enumerate() {
                p.glass(0.085 * k as f32, hz, 0.8, 0.2, (k as f32 - 1.5) * 0.3);
            }
            p.room(0.35, 1.3);
        }
        Cue::Launch => {
            p = Pad::new(3.);
            p.air(
                0.,
                0.62,
                (-0.7, 0.7),
                |t| 250. + 4200. * (t / 0.62).powi(2),
                0.45,
                |t| (t / 0.62).powi(2) * 0.8,
            );
            for (k, hz) in [D4, A4, D5, FS5].into_iter().enumerate() {
                p.glass(0.1 * k as f32, hz, 0.4, 0.15, (k as f32 - 1.5) * 0.4);
            }
            p.thump(0.6, 170., 46., 1.5, 0.3);
            for (hz, pan) in [(D3, -0.2), (A3, 0.2), (D4, -0.4), (FS4, 0.4), (A4, 0.)] {
                p.warm(
                    0.58,
                    2.1,
                    hz,
                    0.3,
                    pan,
                    |t| 300. + 3800. * (-t / 0.5).exp(),
                    |t| (t / 0.02).min(1.) * (-t / 0.6).exp(),
                );
            }
            p.glass(0.6, A5, 0.5, 0.4, 0.3);
            p.glass(0.68, D6, 0.4, 0.4, -0.3);
            p.room(0.4, 1.8);
            p.press(1.9);
        }
        Cue::Return => {
            p = Pad::new(1.7);
            for (k, hz) in [A5, FS5, D5, A4].into_iter().enumerate() {
                p.glass(0.09 * k as f32, hz, 0.8, 0.22, (1.5 - k as f32) * 0.3);
            }
            p.room(0.35, 1.4);
        }
        Cue::Bye => {
            p = Pad::new(1.6);
            p.air(
                0.,
                0.5,
                (0.4, -0.4),
                |t| 3800. * (-t / 0.16).exp() + 200.,
                0.5,
                |t| (PI * (t / 0.5)).sin().powi(2) * 0.4,
            );
            for (k, hz) in [D5, A4, D4].into_iter().enumerate() {
                p.glass(0.08 * k as f32, hz, 0.8, 0.2, 0.);
            }
            p.room(0.35, 1.2);
        }
        Cue::Welcome => return welcome(0., peak),
        Cue::Hello => return welcome(super::console_intro::SHORT_FROM, peak),
    }
    p.finish(peak)
}

/// The start-up music: embers, the flight that rises, and the arrival. `skip` leaves out its
/// beginning (the short animation starts in the middle of the flight).
fn welcome(skip: f32, peak: f32) -> Vec<f32> {
    let mut p = Pad::new(ARRIVE + 4.6);
    // The ground it stands on, waking up.
    for (hz, amp) in [(D1, 0.5), (D2, 0.55), (A2, 0.16)] {
        p.add(0., ARRIVE, 0., |t| {
            let rise = smooth(t / IGNITE) * 0.35 + smooth((t - DIVE) / (ARRIVE - DIVE)).powi(2);
            (TAU * hz * t).sin() * rise * amp
        });
    }
    // Embers: sparse sparks of glass, more and higher towards the light.
    let notes = [D5, E5, FS5, A5, B5, D6, E6, FS6, A6];
    let mut t = 0.1;
    let mut k = 0u32;
    while t < IGNITE {
        let x = t / IGNITE;
        let pick = super::backdrop::rnd(k, 71);
        let hz = notes[((x * 0.65 + pick * 0.35) * (notes.len() - 1) as f32).round() as usize];
        let pan = super::backdrop::rnd(k, 72) * 1.6 - 0.8;
        p.glass(t, hz, 0.05 + 0.09 * x, 0.09, pan);
        t += 0.13 - 0.09 * x + 0.03 * super::backdrop::rnd(k, 73);
        k += 1;
    }
    // The spark catches: a breath drawn in, and a first soft hit.
    p.air(
        IGNITE - 0.45,
        0.45,
        (0., 0.),
        |t| 500. + 5000. * (t / 0.45).powi(3),
        0.4,
        |t| (t / 0.45).powi(3) * 0.4,
    );
    p.thump(IGNITE, 130., 49., 0.9, 0.22);
    for (hz, pan) in [(D4, -0.3), (A4, 0.3), (D5, 0.)] {
        p.glass(IGNITE, hz, 0.3, 0.35, pan);
    }
    // The flight: wind that opens up, a chord that swells, gates and covers rushing by.
    let flight = ARRIVE - DIVE;
    p.air(
        DIVE,
        flight,
        (-0.2, 0.2),
        |t| 220. + 3600. * (t / flight).powf(1.7),
        0.55,
        |t| smooth(t / flight) * 0.5,
    );
    p.air(
        DIVE,
        flight,
        (0.5, -0.5),
        |t| 120. + 900. * (t / flight).powi(2),
        0.35,
        |t| smooth(t / flight) * 0.5,
    );
    for (hz, pan) in [(D3, 0.), (A3, -0.4), (D4, 0.4), (E4, -0.2), (A4, 0.2)] {
        p.warm(
            DIVE,
            flight,
            hz,
            0.2,
            pan,
            |t| 200. + 2600. * (t / flight).powf(2.4),
            |t| smooth(t / flight).powf(1.6),
        );
    }
    for i in 0..9 {
        let x = i as f32 / 9.;
        let at = DIVE + flight * (0.16 + 0.8 * x.powf(0.8));
        let side = if i % 2 == 0 { -1. } else { 1. };
        p.air(
            (at - 0.12).max(0.),
            0.24,
            (side * 0.2, side),
            |t| 2600. - 1700. * smooth(t / 0.24),
            0.4,
            |t| (PI * (t / 0.24)).sin().powi(3) * 0.4,
        );
    }
    // The last moment holds its breath; then everything lands.
    let a = ARRIVE;
    p.thump(a, 190., 41., 2.4, 0.5);
    p.thump(a, 420., 90., 0.7, 0.06);
    p.air(
        a,
        1.1,
        (0., 0.),
        |t| 5200. * (-t / 0.16).exp() + 300.,
        0.8,
        |t| (-t / 0.16).exp() * 1.1,
    );
    for (hz, pan, amp) in [
        (D2, 0., 0.5),
        (D3, -0.2, 0.36),
        (A3, 0.2, 0.34),
        (D4, -0.5, 0.3),
        (E4, 0.45, 0.22),
        (FS4, 0.5, 0.28),
        (A4, -0.45, 0.24),
        (D5, 0., 0.16),
    ] {
        p.warm(
            a,
            3.6,
            hz,
            amp,
            pan,
            |t| 420. + 5200. * (-t / 0.7).exp(),
            |t| (t / 0.012).min(1.) * (-t / 1.05).exp(),
        );
    }
    // The name's letters, one note each.
    for (k, hz) in [D5, FS5, A5, D6, E6].into_iter().enumerate() {
        let t = a + 0.1 + 0.085 * k as f32;
        p.glass(t, hz, 0.5, 0.5, (k as f32 - 2.) * 0.3);
        p.glass(t, hz * 2., 0.1, 0.3, (2. - k as f32) * 0.3);
    }
    p.glass(a + 0.62, A6, 0.16, 0.45, 0.2);
    p.glass(a + 0.62, B4, 0.12, 0.6, -0.2);
    // Two lights run around the gamepad, left and right, and meet.
    for side in [-1., 1.] {
        p.air(
            TRACE,
            TRACE_SECS,
            (0., side),
            |t| 900. + 4200. * smooth(t / TRACE_SECS),
            0.3,
            |t| (PI * t / TRACE_SECS).sin() * 0.16,
        );
    }
    p.glass(TRACE + TRACE_SECS, D6, 0.4, 0.4, 0.);
    p.glass(TRACE + TRACE_SECS, A6, 0.14, 0.25, 0.);
    // Its parts light up where they are.
    for (n, (hz, pan)) in [(A4, -0.7), (D5, -0.35), (FS5, 0.35), (A5, 0.7), (B5, 0.)]
        .into_iter()
        .enumerate()
    {
        let t = PARTS + n as f32 * PART_STEP;
        p.glass(t, hz, 0.34, 0.14, pan);
        p.thump(t, 420., 260., 0.1, 0.02);
    }
    p.room(0.42, 2.4);
    p.press(2.2);
    let from = ((skip * SR) as usize).min(p.l.len());
    p.l.drain(..from);
    p.r.drain(..from);
    if skip > 0. {
        let n = (0.12 * SR) as usize;
        for i in 0..n.min(p.l.len()) {
            let k = smooth(i as f32 / n as f32);
            p.l[i] *= k;
            p.r[i] *= k;
        }
    }
    p.finish(peak)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_is_clean() {
        for cue in Cue::ALL {
            let s = make(cue);
            let secs = s.len() as f32 / 2. / SR;
            assert!(s.iter().all(|v| v.is_finite()), "{cue:?}");
            let top = s.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!((top - cue.peak()).abs() < 0.02, "{cue:?} peaks at {top}");
            // No click at either end.
            assert!(s[0].abs() < 0.02 && s[s.len() - 1].abs() < 0.005, "{cue:?}");
            // What repeats while a direction is held is over before the next one.
            if matches!(cue, Cue::Move | Cue::Key | Cue::Turn) {
                assert!(secs < 0.25, "{cue:?} takes {secs}");
            }
        }
    }

    /// `ARKOR_SOUNDS=<folder> cargo test -p arkor dump_sounds -- --ignored` writes them out.
    #[test]
    #[ignore = "writes wav files"]
    fn dump_sounds() {
        let dir = std::path::PathBuf::from(std::env::var("ARKOR_SOUNDS").unwrap());
        for cue in Cue::ALL {
            let s = make(cue);
            let mut b = Vec::new();
            let data = (s.len() * 2) as u32;
            b.extend(b"RIFF");
            b.extend((36 + data).to_le_bytes());
            b.extend(b"WAVEfmt ");
            b.extend(16u32.to_le_bytes());
            b.extend([1, 0, 2, 0]);
            b.extend(RATE.to_le_bytes());
            b.extend((RATE * 4).to_le_bytes());
            b.extend([4, 0, 16, 0]);
            b.extend(b"data");
            b.extend(data.to_le_bytes());
            for v in s {
                b.extend(((v * 32767.) as i16).to_le_bytes());
            }
            std::fs::write(dir.join(format!("{cue:?}.wav")), b).unwrap();
        }
    }
}
