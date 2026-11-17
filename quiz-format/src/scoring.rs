//! The points a correct answer scores, falling from the most at once to a floor at the time limit, along a curve which
//! is easy to change, so that Bruce can try different ones.

use serde::{Deserialize, Serialize};

/// The shape of the fall in points as time passes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Curve {
    /// Points fall evenly from the most to the floor at the time limit.
    #[default]
    Linear,
    /// Points fall steeply at first and then flatten, reaching the floor at the time limit.
    Logarithmic,
}

/// How the points for a correct answer fall as time passes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoringSettings {
    /// The shape of the fall.
    pub curve: Curve,
    /// The fraction of the most points a correct answer still scores at the time limit, from 0 to 1.
    pub floor: f64,
    /// How steeply a logarithmic curve falls at first: the larger, the steeper. Not used by a linear curve.
    pub steepness: f64,
}

/// Half the points at the time limit, so that a slow correct answer still beats a wrong one by plenty.
pub(crate) fn default_floor() -> f64 {
    0.5
}

/// A logarithmic curve which has lost about half of its fall a fifth of the way through.
pub(crate) fn default_steepness() -> f64 {
    9.0
}

impl Default for ScoringSettings {
    fn default() -> Self {
        Self {
            curve: Curve::Linear,
            floor: default_floor(),
            steepness: default_steepness(),
        }
    }
}

impl ScoringSettings {
    /// The fraction of the most points a correct answer scores after `elapsed` of `limit`, from 1 at once to the floor
    /// at the time limit, and the floor after it, e.g. for an answer which arrived late but within the latency allowed.
    pub fn fraction(&self, elapsed: f64, limit: f64) -> f64 {
        let floor = self.floor.clamp(0.0, 1.0);
        let x = if limit > 0.0 {
            (elapsed / limit).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let remaining = match self.curve {
            Curve::Linear => 1.0 - x,
            Curve::Logarithmic => {
                let k = self.steepness.max(f64::EPSILON);
                1.0 - (1.0 + k * x).ln() / (1.0 + k).ln()
            }
        };
        floor + (1.0 - floor) * remaining
    }

    /// The points a correct answer scores after `elapsed_ms` of a `time_limit` in seconds, out of `max_points`,
    /// rounded to the nearest point.
    pub fn points(&self, max_points: u32, time_limit: u32, elapsed_ms: u64) -> u32 {
        let fraction = self.fraction(elapsed_ms as f64, time_limit as f64 * 1000.0);
        (max_points as f64 * fraction).round() as u32
    }

    /// The curve's fraction at `samples` evenly spaced times from the start to the time limit, for the editor's
    /// preview graph.
    pub fn preview(&self, samples: usize) -> Vec<f64> {
        let last = samples.saturating_sub(1).max(1) as f64;
        (0..samples).map(|i| self.fraction(i as f64, last)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_falls_evenly_to_the_floor() {
        let scoring = ScoringSettings::default();
        assert_eq!(scoring.points(1000, 20, 0), 1000);
        assert_eq!(scoring.points(1000, 20, 10_000), 750);
        assert_eq!(scoring.points(1000, 20, 20_000), 500);
        assert_eq!(scoring.points(1000, 20, 30_000), 500);
    }

    #[test]
    fn logarithmic_falls_steeply_then_flattens() {
        let scoring = ScoringSettings {
            curve: Curve::Logarithmic,
            ..Default::default()
        };
        assert_eq!(scoring.points(1000, 20, 0), 1000);
        assert_eq!(scoring.points(1000, 20, 20_000), 500);
        let early = 1000 - scoring.points(1000, 20, 4_000);
        let late = scoring.points(1000, 20, 16_000) - 500;
        assert!(early > 200, "falls {early} in the first fifth");
        assert!(late < 60, "falls only {late} in the last fifth");
    }

    #[test]
    fn preview_runs_from_one_to_the_floor() {
        let preview = ScoringSettings::default().preview(5);
        assert_eq!(preview, vec![1.0, 0.875, 0.75, 0.625, 0.5]);
    }
}
