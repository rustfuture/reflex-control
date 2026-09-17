use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Official TypeSafe SystemOne request payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneRequest {
    pub model: String,
    pub state: String,
    pub questions: HashMap<String, QuestionSpec>,
}

/// Official TypeSafe SystemOne question primitives
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum QuestionSpec {
    #[serde(rename = "choice")]
    Choice {
        instructions: String,
        criteria: HashMap<String, String>,
    },
    #[serde(rename = "noul")]
    Noul { instructions: String },
    #[serde(rename = "score")]
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

/// Official TypeSafe SystemOne response payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneResponse {
    pub model: String,
    pub answers: HashMap<String, Answer>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    #[serde(default)]
    pub input_tokens: usize,
    #[serde(default)]
    pub output_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Answer {
    #[serde(rename = "choice")]
    Choice {
        choice: String,
        confidence: f64,
        probabilities: HashMap<String, f64>,
    },
    #[serde(rename = "noul")]
    Noul { noul: f64 },
    #[serde(rename = "score")]
    Score {
        score: f64,
        confidence: f64,
        #[serde(default)]
        legend: Option<HashMap<String, String>>,
        #[serde(default)]
        probabilities: Option<HashMap<String, f64>>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_choice_response() {
        let json_data = r#"{
            "model": "jev-1.13.0",
            "answers": {
                "decision": {
                    "type": "choice",
                    "choice": "accept",
                    "confidence": 0.99,
                    "probabilities": {
                        "verify": 0.01,
                        "accept": 0.99,
                        "escalate": 0.0
                    }
                }
            },
            "usage": {
                "input_tokens": 354,
                "output_tokens": 40
            }
        }"#;

        let res: SystemOneResponse = serde_json::from_str(json_data).unwrap();
        assert_eq!(res.model, "jev-1.13.0");
        assert!(res.usage.is_some());
        assert_eq!(res.usage.unwrap().input_tokens, 354);

        if let Some(Answer::Choice {
            choice,
            confidence,
            probabilities,
        }) = res.answers.get("decision")
        {
            assert_eq!(choice, "accept");
            assert!((confidence - 0.99).abs() < 1e-6);
            assert_eq!(*probabilities.get("accept").unwrap(), 0.99);
        } else {
            panic!("Expected Choice answer");
        }
    }

    #[test]
    fn test_deserialize_noul_response() {
        let json_data = r#"{
            "model": "jev-1.13.0",
            "answers": {
                "is_valid": {
                    "type": "noul",
                    "noul": 0.88
                }
            },
            "usage": {
                "input_tokens": 291,
                "output_tokens": 21
            }
        }"#;

        let res: SystemOneResponse = serde_json::from_str(json_data).unwrap();
        if let Some(Answer::Noul { noul }) = res.answers.get("is_valid") {
            assert!((noul - 0.88).abs() < 1e-6);
        } else {
            panic!("Expected Noul answer");
        }
    }

    #[test]
    fn test_deserialize_score_response() {
        let json_data = r#"{
            "model": "jev-1.13.0",
            "answers": {
                "risk": {
                    "type": "score",
                    "score": 3.97,
                    "confidence": 0.97
                }
            }
        }"#;

        let res: SystemOneResponse = serde_json::from_str(json_data).unwrap();
        if let Some(Answer::Score {
            score, confidence, ..
        }) = res.answers.get("risk")
        {
            assert!((score - 3.97).abs() < 1e-6);
            assert!((confidence - 0.97).abs() < 1e-6);
        } else {
            panic!("Expected Score answer");
        }
    }
}
