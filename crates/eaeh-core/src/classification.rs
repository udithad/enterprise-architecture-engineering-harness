//! Data classification (Phase 2). Ordered from least to most sensitive; used for
//! knowledge visibility ceilings and model-egress policy decisions.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Public,
    #[default]
    Internal,
    Confidential,
    Restricted,
}

impl Classification {
    pub fn as_str(self) -> &'static str {
        match self {
            Classification::Public => "public",
            Classification::Internal => "internal",
            Classification::Confidential => "confidential",
            Classification::Restricted => "restricted",
        }
    }

    pub fn rank(self) -> i64 {
        self as i64
    }

    pub fn parse(s: &str) -> crate::Result<Self> {
        Ok(match s {
            "public" => Self::Public,
            "internal" => Self::Internal,
            "confidential" => Self::Confidential,
            "restricted" => Self::Restricted,
            _ => {
                return Err(crate::HarnessError::invalid_input(format!(
                    "unknown classification '{s}'"
                )));
            }
        })
    }
}

impl fmt::Display for Classification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_and_parsing() {
        assert!(Classification::Public < Classification::Restricted);
        assert_eq!(Classification::default(), Classification::Internal);
        assert_eq!(
            Classification::parse("confidential").unwrap(),
            Classification::Confidential
        );
        assert!(Classification::parse("secret").is_err());
    }
}
