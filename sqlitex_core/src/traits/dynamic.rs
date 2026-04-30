#[derive(Debug, Clone)]
pub enum Value {
    Integer(i64),
    Real(f64),
    Text(String),
    // Blob(Vec<u8>),
    Null,
}

impl Value {
    /// Returns the value as a String.
    pub fn as_string(&self) -> String {
        match self {
            Value::Integer(i) => i.to_string(),
            Value::Real(f) => f.to_string(),
            Value::Text(s) => s.clone(),
            // Value::Blob(_) => "<Binary Data>".to_string(),
            Value::Null => "NULL".to_string(),
        }
    }
    /// Returns the value as an i64.
    pub fn as_i64(&self) -> i64 {
        match self {
            Value::Integer(i) => *i,
            Value::Real(f) => *f as i64,
            _ => 0,
        }
    }

    /// Returns the value as an i32
    pub fn as_i32(&self) -> i32 {
        self.as_i64() as i32
    }

    /// Returns the value as an f64. If it's an Integer, it casts it.
    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Real(f) => *f,
            Value::Integer(i) => *i as f64,
            _ => 0.0,
        }
    }

    /// Returns the value as an f32.
    pub fn as_f32(&self) -> f32 {
        self.as_f64() as f32
    }

    /// SQLite handles booleans as 0 (false) and 1 (true).
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Integer(i) => *i != 0,
            Value::Real(f) => *f != 0.0,
            Value::Text(s) => {
                let s = s.to_lowercase();
                s == "true" || s == "1" || s == "yes"
            }
            _ => false,
        }
    }

    /// checks for nulls.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}
