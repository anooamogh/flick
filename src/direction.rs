/// One of the eight flick directions, in screen terms (Up = toward the top of the screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Right,
    UpRight,
    Up,
    UpLeft,
    Left,
    DownLeft,
    Down,
    DownRight,
}

impl Direction {
    /// Counter-clockwise from Right, matching the 45° sector index.
    pub const ALL: [Direction; 8] = [
        Direction::Right,
        Direction::UpRight,
        Direction::Up,
        Direction::UpLeft,
        Direction::Left,
        Direction::DownLeft,
        Direction::Down,
        Direction::DownRight,
    ];

    /// Angle in radians, counter-clockwise from Right, with Up positive.
    pub fn angle(self) -> f32 {
        let index = Direction::ALL.iter().position(|&d| d == self).unwrap();
        index as f32 * std::f32::consts::FRAC_PI_4
    }
}

/// Classifies a cursor offset (screen pixels, y grows downward) into a direction.
/// Returns `None` inside the dead zone around the origin.
pub fn classify(dx: i32, dy: i32, dead_zone: i32) -> Option<Direction> {
    let (dx, dy) = (dx as f32, dy as f32);
    let dead_zone = dead_zone as f32;
    if dx * dx + dy * dy < dead_zone * dead_zone {
        return None;
    }
    let angle = (-dy).atan2(dx); // flip y so Up is positive
    let sector = (angle / std::f32::consts::FRAC_PI_4).round() as i32;
    Some(Direction::ALL[sector.rem_euclid(8) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use Direction::*;

    const DZ: i32 = 30;

    #[test]
    fn inside_dead_zone_is_none() {
        assert_eq!(classify(0, 0, DZ), None);
        assert_eq!(classify(29, 0, DZ), None);
        assert_eq!(classify(20, -20, DZ), None); // ~28px diagonal
    }

    #[test]
    fn edge_of_dead_zone_counts() {
        assert_eq!(classify(30, 0, DZ), Some(Right));
    }

    #[test]
    fn eight_directions() {
        assert_eq!(classify(100, 0, DZ), Some(Right));
        assert_eq!(classify(100, -100, DZ), Some(UpRight));
        assert_eq!(classify(0, -100, DZ), Some(Up));
        assert_eq!(classify(-100, -100, DZ), Some(UpLeft));
        assert_eq!(classify(-100, 0, DZ), Some(Left));
        assert_eq!(classify(-100, 100, DZ), Some(DownLeft));
        assert_eq!(classify(0, 100, DZ), Some(Down));
        assert_eq!(classify(100, 100, DZ), Some(DownRight));
    }

    #[test]
    fn sector_boundaries_at_22_5_degrees() {
        // tan(20°) ≈ 0.364, tan(25°) ≈ 0.466
        assert_eq!(classify(100, -36, DZ), Some(Right));
        assert_eq!(classify(100, -47, DZ), Some(UpRight));
        assert_eq!(classify(100, 36, DZ), Some(Right));
        assert_eq!(classify(100, 47, DZ), Some(DownRight));
        // Wrap-around near 180°
        assert_eq!(classify(-100, 36, DZ), Some(Left));
        assert_eq!(classify(-100, -36, DZ), Some(Left));
    }

    #[test]
    fn angles_round_trip_through_classify() {
        for d in Direction::ALL {
            let (x, y) = (d.angle().cos() * 100.0, -d.angle().sin() * 100.0);
            assert_eq!(classify(x.round() as i32, y.round() as i32, DZ), Some(d));
        }
    }
}
