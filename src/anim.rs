//! Visual state of the ring menu, eased over time. Pure math, no Windows calls.

use crate::direction::Direction;
use std::f32::consts::{PI, TAU};

/// Time constants (seconds) for exponential easing; ~3τ to settle.
const GLIDE_TAU: f32 = 0.035; // bubble moving between slices, ~110ms
const GLOW_TAU: f32 = 0.030; // bubble fading in/out at the center
const OPEN_TAU: f32 = 0.035; // ring appearing, ~110ms
const CLOSE_TAU: f32 = 0.022; // ring fading out, ~70ms

const EPSILON: f32 = 0.002;

#[derive(Debug, Clone)]
pub struct Anim {
    /// Bubble angle in radians, CCW from Right, normalized to [0, TAU).
    pub angle: f32,
    /// Bubble opacity, 0..1.
    pub glow: f32,
    /// Ring appearance, 0 (hidden) ..1 (fully open).
    pub open: f32,
    target_angle: f32,
    target_glow: f32,
    target_open: f32,
}

impl Anim {
    /// A ring that starts invisible and animates open.
    pub fn opening(highlight: Option<Direction>) -> Self {
        let mut anim = Anim {
            angle: 0.0,
            glow: 0.0,
            open: 0.0,
            target_angle: 0.0,
            target_glow: 0.0,
            target_open: 1.0,
        };
        anim.set_highlight(highlight);
        anim
    }

    pub fn set_highlight(&mut self, highlight: Option<Direction>) {
        match highlight {
            Some(dir) => {
                self.target_angle = dir.angle();
                self.target_glow = 1.0;
                // Coming out of the center: appear at the slice rather than sliding from a stale spot.
                if self.glow < EPSILON {
                    self.angle = self.target_angle;
                }
            }
            None => self.target_glow = 0.0,
        }
    }

    pub fn close(&mut self) {
        self.target_open = 0.0;
    }

    /// True once a close has finished fading out.
    pub fn closed(&self) -> bool {
        self.target_open == 0.0 && self.open == 0.0
    }

    pub fn settled(&self) -> bool {
        self.angle == self.target_angle
            && self.glow == self.target_glow
            && self.open == self.target_open
    }

    /// Advances the animation by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let delta = shortest_turn(self.angle, self.target_angle);
        self.angle = (self.angle + delta * ease(dt, GLIDE_TAU)).rem_euclid(TAU);
        if shortest_turn(self.angle, self.target_angle).abs() < EPSILON {
            self.angle = self.target_angle;
        }
        self.glow = approach(self.glow, self.target_glow, ease(dt, GLOW_TAU));
        let tau = if self.target_open > 0.0 {
            OPEN_TAU
        } else {
            CLOSE_TAU
        };
        self.open = approach(self.open, self.target_open, ease(dt, tau));
    }
}

/// Fraction of the remaining distance covered in `dt`; composes exactly across frames.
fn ease(dt: f32, tau: f32) -> f32 {
    1.0 - (-dt / tau).exp()
}

fn approach(from: f32, to: f32, t: f32) -> f32 {
    let next = from + (to - from) * t;
    if (to - next).abs() < EPSILON {
        to
    } else {
        next
    }
}

/// Signed angle from `from` to `to` going the short way round, in (-PI, PI].
pub fn shortest_turn(from: f32, to: f32) -> f32 {
    let d = (to - from).rem_euclid(TAU);
    if d > PI { d - TAU } else { d }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Direction::*;

    fn run(anim: &mut Anim, seconds: f32) {
        for _ in 0..(seconds / 0.01) as usize {
            anim.step(0.01);
        }
    }

    #[test]
    fn shortest_turn_wraps() {
        assert!((shortest_turn(UpLeft.angle(), UpRight.angle()) + PI / 2.0).abs() < 1e-5);
        // DownRight (315°) → Right (0°) is +45°, not -315°.
        assert!((shortest_turn(DownRight.angle(), Right.angle()) - PI / 4.0).abs() < 1e-5);
        assert!((shortest_turn(Right.angle(), DownRight.angle()) + PI / 4.0).abs() < 1e-5);
    }

    #[test]
    fn opens_and_settles() {
        let mut a = Anim::opening(None);
        assert_eq!(a.open, 0.0);
        run(&mut a, 0.5);
        assert_eq!(a.open, 1.0);
        assert!(a.settled());
    }

    #[test]
    fn glides_the_short_way() {
        let mut a = Anim::opening(Some(UpLeft));
        run(&mut a, 0.5);
        a.set_highlight(Some(UpRight));
        a.step(0.02);
        // Partway: between UpRight (45°) and UpLeft (135°), i.e. passing over Up.
        assert!(
            a.angle > UpRight.angle() && a.angle < UpLeft.angle(),
            "{}",
            a.angle
        );
        run(&mut a, 0.5);
        assert_eq!(a.angle, UpRight.angle());
    }

    #[test]
    fn glides_across_zero() {
        let mut a = Anim::opening(Some(DownRight));
        run(&mut a, 0.5);
        a.set_highlight(Some(Right));
        a.step(0.02);
        assert!(
            a.angle > DownRight.angle(),
            "went the long way: {}",
            a.angle
        );
        run(&mut a, 0.5);
        assert_eq!(a.angle, Right.angle());
    }

    #[test]
    fn leaving_center_appears_in_place() {
        let mut a = Anim::opening(Some(Left));
        run(&mut a, 0.5);
        a.set_highlight(None);
        run(&mut a, 0.5);
        assert_eq!(a.glow, 0.0);
        assert_eq!(a.angle, Left.angle()); // stays put while fading
        a.set_highlight(Some(Up));
        assert_eq!(a.angle, Up.angle()); // no slide from Left
        a.step(0.01);
        assert!(a.glow > 0.0 && a.glow < 1.0);
    }

    #[test]
    fn close_fades_to_closed() {
        let mut a = Anim::opening(None);
        run(&mut a, 0.5);
        a.close();
        assert!(!a.closed());
        run(&mut a, 0.3);
        assert!(a.closed());
    }

    #[test]
    fn frame_rate_independent() {
        let mut fast = Anim::opening(Some(Up));
        let mut slow = fast.clone();
        for _ in 0..8 {
            fast.step(0.005);
        }
        slow.step(0.04);
        assert!((fast.open - slow.open).abs() < 1e-3);
        assert!((fast.glow - slow.glow).abs() < 1e-3);
    }
}
