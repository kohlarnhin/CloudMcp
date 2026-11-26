use serde_json::Value;

/// Normalize home-directory paths at the transport boundary, before truncation.
pub fn text(input: &str) -> String {
    let Some(home) = dirs::home_dir() else {
        return input.to_owned();
    };
    let mut paths = vec![home.clone()];
    if let Ok(canonical) = home.canonicalize() {
        if canonical != home {
            paths.push(canonical);
        }
    }
    let mut output = input.to_owned();
    for path in paths {
        let path = path.to_string_lossy();
        if path.len() <= 1 {
            continue;
        }
        let mut normalized = String::with_capacity(output.len());
        let mut position = 0;
        for (index, _) in output.match_indices(path.as_ref()) {
            let end = index + path.len();
            if output[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
            {
                continue;
            }
            normalized.push_str(&output[position..index]);
            normalized.push('~');
            position = end;
        }
        normalized.push_str(&output[position..]);
        output = normalized;
    }
    output
}

pub fn value(input: Value) -> Value {
    match input {
        Value::String(s) => Value::String(text(&s)),
        Value::Array(items) => Value::Array(items.into_iter().map(value).collect()),
        Value::Object(items) => Value::Object(
            items
                .into_iter()
                .map(|(key, item)| (text(&key), value(item)))
                .collect(),
        ),
        other => other,
    }
}
