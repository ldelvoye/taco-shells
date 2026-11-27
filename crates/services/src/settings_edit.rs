use jsonc_parser::cst::CstObject;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookEdit {
    Changed(String),
    Unchanged,
    Refused(String),
}

pub(crate) fn unreadable_problem(label: &str) -> String {
    format!("{label} could not be read as JSON, so nothing was changed.")
}

pub(crate) fn uneditable_problem(label: &str) -> String {
    format!("{label} could not be edited, so nothing was changed.")
}

pub(crate) fn properties_named(object: &CstObject, name: &str) -> usize {
    let properties = object.properties();
    let mut count = 0;
    for property in properties {
        let decoded = property.decoded_name();
        if decoded.as_deref() == Some(name) {
            count += 1;
        }
    }
    count
}
