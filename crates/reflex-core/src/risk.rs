use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub fn requires_mandatory_verification(&self) -> bool {
        matches!(self, RiskLevel::High | RiskLevel::Critical)
    }

    pub fn is_critical(&self) -> bool {
        matches!(self, RiskLevel::Critical)
    }
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "low"),
            RiskLevel::Medium => write!(f, "medium"),
            RiskLevel::High => write!(f, "high"),
            RiskLevel::Critical => write!(f, "critical"),
        }
    }
}

impl std::str::FromStr for RiskLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(RiskLevel::Low),
            "medium" | "med" => Ok(RiskLevel::Medium),
            "high" => Ok(RiskLevel::High),
            "critical" | "crit" => Ok(RiskLevel::Critical),
            other => Err(format!("Unknown risk level: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_level_ordering_and_mandatory() {
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);

        assert!(!RiskLevel::Low.requires_mandatory_verification());
        assert!(!RiskLevel::Medium.requires_mandatory_verification());
        assert!(RiskLevel::High.requires_mandatory_verification());
        assert!(RiskLevel::Critical.requires_mandatory_verification());
    }
}
