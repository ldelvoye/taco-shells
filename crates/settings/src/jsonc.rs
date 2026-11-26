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

#[cfg(test)]
mod tests {
    use super::*;

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
}
