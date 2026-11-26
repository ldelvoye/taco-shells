use jsonc_parser::cst::{CstInputValue, CstObject, CstObjectProp, CstRootNode};
use jsonc_parser::errors::{ParseError, ParseErrorKind};
use jsonc_parser::{ParseOptions, parse_to_serde_value};
use serde_json::{Map, Value};

pub const JSONC_PARSE_OPTIONS: ParseOptions = ParseOptions {
    allow_comments: true,
    allow_loose_object_property_names: false,
    allow_trailing_commas: true,
    allow_missing_commas: false,
    allow_single_quoted_strings: false,
    allow_hexadecimal_numbers: false,
    allow_unary_plus_numbers: false,
    allow_bare_decimal_point_numbers: false,
    allow_non_finite_numbers: false,
    allow_extended_string_escapes: false,
};

#[derive(Clone, Debug, PartialEq)]
pub struct JsoncResult {
    pub value: Option<Map<String, Value>>,
    pub problems: Vec<String>,
}

fn refused(problem: String) -> JsoncResult {
    JsoncResult {
        value: None,
        problems: vec![problem],
    }
}

fn line_of_error(error: &ParseError, text: &str) -> usize {
    let start = error.range().start;
    let bytes = text.as_bytes();
    let end = start.min(bytes.len());
    let before_error = &bytes[..end];
    let newline_count = before_error.iter().filter(|byte| **byte == b'\n').count();
    newline_count + 1
}

fn describe_error(error: &ParseError, text: &str, label: &str) -> String {
    if matches!(error.kind(), ParseErrorKind::NestingDepthExceeded) {
        return format!("{label}: the file is too deeply nested to read");
    }
    let line = line_of_error(error, text);
    let kind = error.kind().to_string();
    let description = kind.to_lowercase();
    format!("{label}: {description} at line {line}")
}

fn accepted(map: Map<String, Value>) -> JsoncResult {
    JsoncResult {
        value: Some(map),
        problems: Vec::new(),
    }
}

pub fn parse_jsonc(text: &str, label: &str) -> JsoncResult {
    let parsed: Result<Option<Value>, ParseError> =
        parse_to_serde_value(text, &JSONC_PARSE_OPTIONS);

    let value = match parsed {
        Ok(value) => value,
        Err(error) => {
            let problem = describe_error(&error, text, label);
            return refused(problem);
        }
    };

    match value {
        Some(Value::Object(map)) => accepted(map),
        Some(_) => refused(format!("{label}: the file must hold a JSON object")),
        None => accepted(Map::new()),
    }
}

fn input_from(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(flag) => CstInputValue::Bool(*flag),
        Value::Number(number) => CstInputValue::Number(number.to_string()),
        Value::String(text) => CstInputValue::String(text.clone()),
        Value::Array(items) => {
            let converted: Vec<CstInputValue> = items.iter().map(input_from).collect();
            CstInputValue::Array(converted)
        }
        Value::Object(entries) => {
            let mut converted: Vec<(String, CstInputValue)> = Vec::new();
            for (name, entry) in entries {
                converted.push((name.clone(), input_from(entry)));
            }
            CstInputValue::Object(converted)
        }
    }
}

fn properties_named(object: &CstObject, name: &str) -> Vec<CstObjectProp> {
    let properties = object.properties();
    let mut matching = Vec::new();
    for property in properties {
        let decoded = property.decoded_name();
        if decoded.as_deref() == Some(name) {
            matching.push(property);
        }
    }
    matching
}

pub fn set_top_level(
    text: Option<&str>,
    key: &str,
    value: &Value,
    label: &str,
) -> Result<String, String> {
    let source = match text {
        Some(source) if !source.trim().is_empty() => source,
        _ => {
            let name = Value::from(key);
            return Ok(format!("{{\n  {name}: {value}\n}}\n"));
        }
    };

    let parsed = parse_jsonc(source, label);
    if let Some(problem) = parsed.problems.into_iter().next() {
        return Err(problem);
    }
    if parsed.value.is_none() {
        return Err(format!("{label}: the file must hold a JSON object"));
    }

    let root = match CstRootNode::parse(source, &JSONC_PARSE_OPTIONS) {
        Ok(root) => root,
        Err(error) => return Err(describe_error(&error, source, label)),
    };
    let object = root.object_value_or_set();
    let matching = properties_named(&object, key);

    // The reader takes the last of two, the edit would change the first, so a write would silently do nothing.
    if matching.len() > 1 {
        return Err(format!("{label} has \"{key}\" twice"));
    }

    let input = input_from(value);
    match matching.first() {
        Some(property) => property.set_value(input),
        None => {
            object.append(key, input);
        }
    }
    Ok(root.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConfigTexts, resolve_config};
    use crate::settings::ShellRowName;
    use serde_json::json;

    #[test]
    fn broken_files_are_refused_whole_with_the_line_named() {
        let missing_comma = "{\n  \"a\": 1,\n  \"b\": 2 \"c\": 3\n}";
        let broken = parse_jsonc(missing_comma, "settings.json");
        assert_eq!(broken.value, None);
        assert_eq!(broken.problems.len(), 1);
        assert!(
            broken.problems[0].starts_with("settings.json: ")
                && broken.problems[0].ends_with(" at line 3"),
            "{:?}",
            broken.problems
        );

        let deep = "[".repeat(100_000);
        let nested = parse_jsonc(&deep, "settings.json");
        assert_eq!(nested.value, None);
        assert_eq!(nested.problems.len(), 1);

        let array = parse_jsonc("[1, 2]", "settings.json");
        assert_eq!(array.value, None);
        assert_eq!(
            array.problems,
            vec!["settings.json: the file must hold a JSON object"]
        );

        let blank = parse_jsonc("  // nothing here\n", "settings.json");
        assert_eq!(blank.value, Some(Map::new()));
        assert!(blank.problems.is_empty());

        let trailing = parse_jsonc("{ \"a\": 1, }", "settings.json");
        assert!(trailing.problems.is_empty());
    }

    const MESSY: &str = "{\n\t// my note\n\t\"fontSize\": 12,\n\t\"futureThing\": 1,\n}\n";

    #[test]
    fn replacing_a_value_changes_only_that_value() {
        let written = set_top_level(Some(MESSY), "fontSize", &json!(14), "settings.json");

        let expected = "{\n\t// my note\n\t\"fontSize\": 14,\n\t\"futureThing\": 1,\n}\n";
        assert_eq!(written, Ok(expected.to_string()));
    }

    #[test]
    fn adding_a_missing_key_keeps_the_file_and_reads_back() {
        let written = set_top_level(Some(MESSY), "shellRowName", &json!("path"), "settings.json");
        let text = written.expect("a readable file is written");

        assert_eq!(
            text,
            "{\n\t// my note\n\t\"fontSize\": 12,\n\t\"futureThing\": 1,\n\t\"shellRowName\": \"path\",\n}\n"
        );
        let texts = ConfigTexts {
            settings: Some(text),
            keybindings: None,
        };
        let config = resolve_config(&texts, false);
        assert_eq!(config.settings.shell_row_name, ShellRowName::Path);
        assert_eq!(config.settings.font_size, 12.0);
    }

    #[test]
    fn a_missing_or_blank_file_gets_the_one_key_file() {
        let expected = "{\n  \"fontSize\": 14\n}\n";
        let missing = set_top_level(None, "fontSize", &json!(14), "settings.json");
        assert_eq!(missing, Ok(expected.to_string()));

        let blank = set_top_level(Some(" \n\t"), "fontSize", &json!(14), "settings.json");
        assert_eq!(blank, Ok(expected.to_string()));

        let null = set_top_level(None, "cmd+k", &Value::Null, "keybindings.json");
        assert_eq!(null, Ok("{\n  \"cmd+k\": null\n}\n".to_string()));

        let nulled = set_top_level(Some("{ \"cmd+k\": \"x\" }"), "cmd+k", &Value::Null, "k");
        assert_eq!(nulled, Ok("{ \"cmd+k\": null }".to_string()));
    }

    #[test]
    fn a_duplicate_key_or_an_unreadable_file_is_never_written() {
        let twice = "{ \"fontSize\": 12, \"fontSize\": 13 }";
        let duplicate = set_top_level(Some(twice), "fontSize", &json!(14), "settings.json");
        assert_eq!(
            duplicate,
            Err("settings.json has \"fontSize\" twice".to_string())
        );

        let broken = "{ \"fontSize\": }";
        let refused = set_top_level(Some(broken), "fontSize", &json!(14), "settings.json");
        let problem = refused.expect_err("an unreadable file is refused");
        assert!(problem.starts_with("settings.json: "), "{problem}");

        let array = set_top_level(Some("[1]"), "fontSize", &json!(14), "settings.json");
        assert_eq!(
            array,
            Err("settings.json: the file must hold a JSON object".to_string())
        );
    }
}
