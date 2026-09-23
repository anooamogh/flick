//! Decides what a right-button press means. Pure logic: it sees events, never Windows.
//!
//! - Quick click (released before the menu delay, never left the dead zone) → replay a normal right-click.
//! - Released outside the dead zone → fire that direction (menu shown or not).
//! - Released in the dead zone after the menu appeared, or after leaving and coming back → cancel.

use crate::direction::{Direction, classify};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    RightDown(Point),
    Move(Point),
    RightUp(Point),
    /// The menu delay elapsed since the last RightDown.
    MenuDelayElapsed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Replay the swallowed right-click so the normal context menu appears.
    Click,
    Cancel,
    Fire(Direction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// A press was captured; start the menu delay timer.
    Pressed,
    ShowMenu {
        origin: Point,
        highlight: Option<Direction>,
    },
    Highlight(Option<Direction>),
    /// The press is over; hide the menu (if shown) and carry out the outcome.
    Finish(Outcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Response {
    /// True if the hook should block this event from reaching other apps.
    pub swallow: bool,
    pub effect: Option<Effect>,
}

impl Response {
    const PASS: Response = Response {
        swallow: false,
        effect: None,
    };
}

#[derive(Debug, Clone, Copy)]
struct Press {
    origin: Point,
    menu_shown: bool,
    left_center: bool,
    highlight: Option<Direction>,
}

#[derive(Debug)]
pub struct Tracker {
    dead_zone: i32,
    press: Option<Press>,
}

impl Tracker {
    pub fn new(dead_zone: i32) -> Self {
        Tracker {
            dead_zone,
            press: None,
        }
    }

    /// Forget any press in progress (e.g. the hook is being removed).
    pub fn reset(&mut self) {
        self.press = None;
    }

    pub fn handle(&mut self, event: Event) -> Response {
        match (event, self.press.as_mut()) {
            (Event::RightDown(origin), None) => {
                self.press = Some(Press {
                    origin,
                    menu_shown: false,
                    left_center: false,
                    highlight: None,
                });
                Response {
                    swallow: true,
                    effect: Some(Effect::Pressed),
                }
            }
            (Event::Move(p), Some(press)) => {
                let dir = classify(p.x - press.origin.x, p.y - press.origin.y, self.dead_zone);
                press.left_center |= dir.is_some();
                if press.menu_shown && dir != press.highlight {
                    press.highlight = dir;
                    return Response {
                        swallow: false,
                        effect: Some(Effect::Highlight(dir)),
                    };
                }
                press.highlight = dir;
                Response::PASS
            }
            (Event::MenuDelayElapsed, Some(press)) if !press.menu_shown => {
                press.menu_shown = true;
                let effect = Effect::ShowMenu {
                    origin: press.origin,
                    highlight: press.highlight,
                };
                Response {
                    swallow: false,
                    effect: Some(effect),
                }
            }
            (Event::RightUp(p), Some(press)) => {
                let dir = classify(p.x - press.origin.x, p.y - press.origin.y, self.dead_zone);
                let outcome = match dir {
                    Some(d) => Outcome::Fire(d),
                    None if press.menu_shown || press.left_center => Outcome::Cancel,
                    None => Outcome::Click,
                };
                self.press = None;
                Response {
                    swallow: true,
                    effect: Some(Effect::Finish(outcome)),
                }
            }
            // Moves with no press, a second RightDown, stray RightUp, late timer: not ours.
            _ => Response::PASS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Direction::*;

    const O: Point = Point { x: 500, y: 500 };

    fn at(dx: i32, dy: i32) -> Point {
        Point {
            x: O.x + dx,
            y: O.y + dy,
        }
    }

    fn finish(r: Response) -> Outcome {
        assert!(r.swallow);
        match r.effect {
            Some(Effect::Finish(o)) => o,
            other => panic!("expected Finish, got {other:?}"),
        }
    }

    #[test]
    fn down_is_swallowed_and_arms_timer() {
        let mut t = Tracker::new(30);
        assert_eq!(
            t.handle(Event::RightDown(O)),
            Response {
                swallow: true,
                effect: Some(Effect::Pressed)
            }
        );
    }

    #[test]
    fn quick_click_replays_click() {
        let mut t = Tracker::new(30);
        t.handle(Event::RightDown(O));
        t.handle(Event::Move(at(3, 2))); // hand jitter
        assert_eq!(finish(t.handle(Event::RightUp(at(3, 2)))), Outcome::Click);
    }

    #[test]
    fn hold_then_flick_fires_on_release() {
        let mut t = Tracker::new(30);
        t.handle(Event::RightDown(O));
        let r = t.handle(Event::MenuDelayElapsed);
        assert_eq!(
            r.effect,
            Some(Effect::ShowMenu {
                origin: O,
                highlight: None
            })
        );
        let r = t.handle(Event::Move(at(80, 80)));
        assert_eq!(
            r,
            Response {
                swallow: false,
                effect: Some(Effect::Highlight(Some(DownRight)))
            }
        );
        assert_eq!(
            finish(t.handle(Event::RightUp(at(80, 80)))),
            Outcome::Fire(DownRight)
        );
    }

    #[test]
    fn highlight_only_emitted_on_change() {
        let mut t = Tracker::new(30);
        t.handle(Event::RightDown(O));
        t.handle(Event::MenuDelayElapsed);
        assert!(t.handle(Event::Move(at(0, -50))).effect.is_some());
        assert_eq!(t.handle(Event::Move(at(0, -60))), Response::PASS);
        assert_eq!(
            t.handle(Event::Move(at(0, -5))).effect,
            Some(Effect::Highlight(None))
        );
    }

    #[test]
    fn hold_then_release_in_center_cancels() {
        let mut t = Tracker::new(30);
        t.handle(Event::RightDown(O));
        t.handle(Event::MenuDelayElapsed);
        assert_eq!(finish(t.handle(Event::RightUp(at(5, 0)))), Outcome::Cancel);
    }

    #[test]
    fn fast_flick_before_menu_fires() {
        let mut t = Tracker::new(30);
        t.handle(Event::RightDown(O));
        assert_eq!(t.handle(Event::Move(at(-100, 0))), Response::PASS); // no menu yet, no highlight
        assert_eq!(
            finish(t.handle(Event::RightUp(at(-100, 0)))),
            Outcome::Fire(Left)
        );
    }

    #[test]
    fn menu_shown_late_includes_current_highlight() {
        let mut t = Tracker::new(30);
        t.handle(Event::RightDown(O));
        t.handle(Event::Move(at(0, 90)));
        let r = t.handle(Event::MenuDelayElapsed);
        assert_eq!(
            r.effect,
            Some(Effect::ShowMenu {
                origin: O,
                highlight: Some(Down)
            })
        );
    }

    #[test]
    fn flick_out_and_back_quickly_cancels_not_clicks() {
        let mut t = Tracker::new(30);
        t.handle(Event::RightDown(O));
        t.handle(Event::Move(at(60, 0)));
        t.handle(Event::Move(at(2, 0)));
        assert_eq!(finish(t.handle(Event::RightUp(at(2, 0)))), Outcome::Cancel);
    }

    #[test]
    fn events_without_a_press_pass_through() {
        let mut t = Tracker::new(30);
        assert_eq!(t.handle(Event::Move(O)), Response::PASS);
        assert_eq!(t.handle(Event::RightUp(O)), Response::PASS);
        assert_eq!(t.handle(Event::MenuDelayElapsed), Response::PASS);
    }

    #[test]
    fn late_timer_after_release_is_ignored() {
        let mut t = Tracker::new(30);
        t.handle(Event::RightDown(O));
        t.handle(Event::RightUp(O));
        assert_eq!(t.handle(Event::MenuDelayElapsed), Response::PASS);
    }

    #[test]
    fn reset_drops_press() {
        let mut t = Tracker::new(30);
        t.handle(Event::RightDown(O));
        t.reset();
        assert_eq!(t.handle(Event::RightUp(O)), Response::PASS);
    }
}
