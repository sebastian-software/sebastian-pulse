//! Product-owned workspace configuration operations shared by HTTP and MCP.
//!
//! Actor UUIDs must come from a trusted, verified login integration, never a request
//! parameter or an unverified email address. This module does not authenticate users
//! or implement an OAuth authorization server. Verified managed access is scoped
//! to the current operation and never copied into persistent local membership grants.

mod store;
pub use store::WorkspaceStore;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace access denied")]
    Denied,
    #[error("configuration item not found")]
    NotFound,
    #[error("configuration changed; reload before retrying")]
    Conflict,
    #[error("invalid configuration: {0}")]
    InvalidInput(&'static str),
    #[error("configuration database operation failed")]
    Database(
        #[from]
        #[source]
        sqlx::Error,
    ),
}

pub type Result<T> = std::result::Result<T, WorkspaceError>;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Project {
    pub workspace_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Website,
    Product,
    Cli,
}

impl ProjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Website => "website",
            Self::Product => "product",
            Self::Cli => "cli",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewProject {
    pub name: String,
    pub kind: ProjectKind,
}

impl NewProject {
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Goal {
    pub workspace_id: Uuid,
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub event_name: String,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewGoal {
    pub project_id: Uuid,
    pub name: String,
    pub event_name: String,
}

impl NewGoal {
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        if self.event_name.is_empty()
            || self.event_name.len() > 128
            || !self
                .event_name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.:-".contains(&c))
        {
            return Err(WorkspaceError::InvalidInput(
                "event_name must be 1–128 ASCII letters, digits, or _.:-",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Recipient {
    pub workspace_id: Uuid,
    pub id: Uuid,
    pub email: String,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRecipient {
    pub email: String,
}

impl NewRecipient {
    pub fn validate(&self) -> Result<()> {
        // An intentionally narrow initial mailbox format, not proof of ownership or
        // a permission grant. Login must independently verify email before report access.
        let Some((local, domain)) = self.email.split_once('@') else {
            return Err(WorkspaceError::InvalidInput(
                "email must be a mailbox address",
            ));
        };
        if self.email.len() > 254
            || local.is_empty()
            || local.len() > 64
            || !local
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&c))
            || local.starts_with('.')
            || local.ends_with('.')
            || local.contains("..")
            || !domain.contains('.')
            || domain.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
        {
            return Err(WorkspaceError::InvalidInput(
                "email must be a valid ASCII mailbox address",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_after: Option<Uuid>,
}

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
        return Err(WorkspaceError::InvalidInput(
            "name must contain 1–120 non-control characters",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_rejects_unbounded_or_unstructured_text() {
        for name in [
            "".to_string(),
            " \t".to_string(),
            "x".repeat(121),
            "bad\nname".to_string(),
        ] {
            assert!(
                NewProject {
                    name,
                    kind: ProjectKind::Website
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            NewProject {
                name: "Product 🚀".into(),
                kind: ProjectKind::Product
            }
            .validate()
            .is_ok()
        );
        for event in [
            "",
            "request?email=person@example.com",
            "path/to/file",
            "with spaces",
        ] {
            assert!(
                NewGoal {
                    project_id: Uuid::nil(),
                    name: "Inquiry".into(),
                    event_name: event.into()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            NewGoal {
                project_id: Uuid::nil(),
                name: "Inquiry".into(),
                event_name: "inquiry.completed:v1".into()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn recipient_configuration_is_not_email_verification() {
        for email in [
            "",
            "a@@example.com",
            "person@example..com",
            "a\r\nBcc: victim@example.com",
            "a@-example.com",
            ".a@example.com",
            "a..b@example.com",
        ] {
            assert!(
                NewRecipient {
                    email: email.into()
                }
                .validate()
                .is_err(),
                "{email:?}"
            );
        }
        assert!(
            NewRecipient {
                email: "reports+monthly@example.com".into()
            }
            .validate()
            .is_ok()
        );
        assert!(
            serde_json::from_str::<NewRecipient>(r#"{"email":"a@example.com","verified":true}"#)
                .is_err()
        );
    }
}
