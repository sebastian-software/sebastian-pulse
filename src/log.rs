use chrono::{SecondsFormat, Utc};
use serde_json::{Map, Value};

/// Emit only caller-selected safe fields, never driver messages or request data.
pub fn event(name: &str, fields: Value) {
    let mut record = Map::new();
    record.insert(
        "time".into(),
        Utc::now()
            .to_rfc3339_opts(SecondsFormat::Millis, true)
            .into(),
    );
    record.insert("event".into(), name.into());
    if let Value::Object(fields) = fields {
        record.extend(fields);
    }
    println!("{}", Value::Object(record));
}

pub fn error_code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .filter(|code| {
            (2..=40).contains(&code.len())
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
        .map_or_else(|| "UNCLASSIFIED".into(), |code| code.into_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn driver_messages_are_not_a_log_code() {
        let error = sqlx::Error::Protocol("postgresql://private:secret@host/db".into());
        assert_eq!(super::error_code(&error), "UNCLASSIFIED");
    }
}
