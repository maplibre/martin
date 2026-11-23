use crate::Error;
use jsonschema::{self, Evaluation};
use serde_json::Value;

#[derive(Debug)]
/// Validator for cql2 json schema
pub struct Validator {
    validator: jsonschema::Validator,
}

impl Validator {
    /// Instantiate Validator instance loading schema.
    pub fn new() -> Result<Self, Error> {
        let schema_str = include_str!("cql2.json");
        let schema_json: Value = serde_json::from_str(schema_str)?;
        let validator =
            jsonschema::validator_for(&schema_json).expect("Could not construct schema validator.");
        Ok(Validator { validator })
    }

    /// Validate CQL2 Json
    pub fn is_valid(&self, v: &Value) -> bool {
        self.validator.is_valid(v)
    }
    /// Validates a [serde_json::Value].
    ///
    /// # Examples
    ///
    /// ```
    /// use cql2::Validator;
    /// use serde_json::json;
    ///
    /// let validator = Validator::new().unwrap();
    ///
    /// let valid = json!({
    ///     "op": "=",
    ///     "args": [
    ///         { "property": "landsat:scene_id" },
    ///         "LC82030282019133LGN00"
    ///     ]
    /// });
    /// validator.validate(&valid).unwrap();
    ///
    /// let invalid = json!({
    ///     "op": "t_before",
    ///     "args": [{"property": "updated_at"}, {"timestamp": "invalid-timestamp"}],
    /// });
    /// validator.validate(&invalid).unwrap_err();
    /// ```
    pub fn validate<'i>(&self, v: &'i Value) -> Result<(), jsonschema::ValidationError<'i>> {
        self.validator.validate(v)
    }

    /// Apply a validator
    pub fn apply(&self, v: &Value) -> Evaluation {
        self.validator.evaluate(v)
    }

    /// Iterate through validation errors
    pub fn iter_errors<'i>(&'i self, v: &'i Value) -> jsonschema::ErrorIterator<'i> {
        self.validator.iter_errors(v)
    }
}
