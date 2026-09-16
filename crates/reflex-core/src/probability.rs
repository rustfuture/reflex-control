use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Probability(f64);

impl Probability {
    pub const ZERO: Self = Self(0.0);
    pub const ONE: Self = Self(1.0);

    pub fn new(value: f64) -> Result<Self, CoreError> {
        if !(0.0..=1.0).contains(&value) || value.is_nan() {
            return Err(CoreError::InvalidProbability { value });
        }
        Ok(Self(value))
    }

    /// Clamp into [0.0, 1.0]
    pub fn clamp(value: f64) -> Self {
        if value.is_nan() || value <= 0.0 {
            Self::ZERO
        } else if value >= 1.0 {
            Self::ONE
        } else {
            Self(value)
        }
    }

    pub fn value(&self) -> f64 {
        self.0
    }
}

impl Default for Probability {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Display for Probability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.4}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Confidence(f64);

impl Confidence {
    pub const ZERO: Self = Self(0.0);
    pub const ONE: Self = Self(1.0);

    pub fn new(value: f64) -> Result<Self, CoreError> {
        if !(0.0..=1.0).contains(&value) || value.is_nan() {
            return Err(CoreError::InvalidConfidence { value });
        }
        Ok(Self(value))
    }

    pub fn clamp(value: f64) -> Self {
        if value.is_nan() || value <= 0.0 {
            Self::ZERO
        } else if value >= 1.0 {
            Self::ONE
        } else {
            Self(value)
        }
    }

    pub fn value(&self) -> f64 {
        self.0
    }
}

impl Default for Confidence {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.4}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_probability_and_confidence() {
        assert!(Probability::new(0.5).is_ok());
        assert!(Probability::new(1.0).is_ok());
        assert!(Probability::new(0.0).is_ok());
        assert!(Probability::new(1.01).is_err());
        assert!(Probability::new(-0.01).is_err());
        assert!(Probability::new(f64::NAN).is_err());

        assert!(Confidence::new(0.85).is_ok());
        assert!(Confidence::new(-0.1).is_err());
    }

    #[test]
    fn test_clamp() {
        assert_eq!(Probability::clamp(-1.0).value(), 0.0);
        assert_eq!(Probability::clamp(1.5).value(), 1.0);
        assert_eq!(Probability::clamp(0.42).value(), 0.42);
    }
}
