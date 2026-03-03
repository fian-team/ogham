//! Shared arithmetic and comparison operations on [`Value`].
//!
//! The bytecode VM delegates to the helpers in this module so that
//! arithmetic and comparison logic is defined once.

use crate::runtime::error::VMError;
use crate::runtime::value::Value;

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

pub fn add(a: &Value, b: &Value) -> Result<Value, VMError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a + b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
        (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a + *b as f64)),
        (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
        (Value::String(a), b) => Ok(Value::String(format!("{}{}", a, b))),
        (a, Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
        _ => Err(VMError::TypeMismatch(format!(
            "Cannot add {:?} and {:?}",
            a, b
        ))),
    }
}

pub fn subtract(a: &Value, b: &Value) -> Result<Value, VMError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a - b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
        (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
        (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a - *b as f64)),
        _ => Err(VMError::TypeMismatch(format!(
            "Cannot subtract {:?} from {:?}",
            b, a
        ))),
    }
}

pub fn multiply(a: &Value, b: &Value) -> Result<Value, VMError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a * b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
        (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
        (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a * *b as f64)),
        _ => Err(VMError::TypeMismatch(format!(
            "Cannot multiply {:?} and {:?}",
            a, b
        ))),
    }
}

pub fn divide(a: &Value, b: &Value) -> Result<Value, VMError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => {
            if *b == 0 {
                return Err(VMError::InvalidOperation("Division by zero".to_string()));
            }
            Ok(Value::Float(*a as f64 / *b as f64))
        }
        (Value::Float(a), Value::Float(b)) => {
            if *b == 0.0 {
                return Err(VMError::InvalidOperation("Division by zero".to_string()));
            }
            Ok(Value::Float(a / b))
        }
        (Value::Integer(a), Value::Float(b)) => {
            if *b == 0.0 {
                return Err(VMError::InvalidOperation("Division by zero".to_string()));
            }
            Ok(Value::Float(*a as f64 / b))
        }
        (Value::Float(a), Value::Integer(b)) => {
            if *b == 0 {
                return Err(VMError::InvalidOperation("Division by zero".to_string()));
            }
            Ok(Value::Float(a / *b as f64))
        }
        _ => Err(VMError::TypeMismatch(format!(
            "Cannot divide {:?} by {:?}",
            a, b
        ))),
    }
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

pub fn equals(a: &Value, b: &Value) -> Result<Value, VMError> {
    if std::mem::discriminant(a) != std::mem::discriminant(b) {
        return Err(VMError::TypeMismatch(format!(
            "Cannot compare values of different types with ==: {:?} and {:?}",
            a, b
        )));
    }
    Ok(Value::Boolean(a == b))
}

pub fn not_equals(a: &Value, b: &Value) -> Result<Value, VMError> {
    if std::mem::discriminant(a) != std::mem::discriminant(b) {
        return Err(VMError::TypeMismatch(format!(
            "Cannot compare values of different types with !=: {:?} and {:?}",
            a, b
        )));
    }
    Ok(Value::Boolean(a != b))
}

pub fn greater_than(a: &Value, b: &Value) -> Result<Value, VMError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Boolean(a > b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Boolean(a > b)),
        (Value::Integer(a), Value::Float(b)) => Ok(Value::Boolean((*a as f64) > *b)),
        (Value::Float(a), Value::Integer(b)) => Ok(Value::Boolean(*a > (*b as f64))),
        _ => Err(VMError::TypeMismatch(format!(
            "Cannot compare {:?} > {:?}",
            a, b
        ))),
    }
}

pub fn greater_than_or_equal(a: &Value, b: &Value) -> Result<Value, VMError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Boolean(a >= b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Boolean(a >= b)),
        (Value::Integer(a), Value::Float(b)) => Ok(Value::Boolean((*a as f64) >= *b)),
        (Value::Float(a), Value::Integer(b)) => Ok(Value::Boolean(*a >= (*b as f64))),
        _ => Err(VMError::TypeMismatch(format!(
            "Cannot compare {:?} >= {:?}",
            a, b
        ))),
    }
}

pub fn less_than(a: &Value, b: &Value) -> Result<Value, VMError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Boolean(a < b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Boolean(a < b)),
        (Value::Integer(a), Value::Float(b)) => Ok(Value::Boolean((*a as f64) < *b)),
        (Value::Float(a), Value::Integer(b)) => Ok(Value::Boolean(*a < (*b as f64))),
        _ => Err(VMError::TypeMismatch(format!(
            "Cannot compare {:?} < {:?}",
            a, b
        ))),
    }
}

pub fn less_than_or_equal(a: &Value, b: &Value) -> Result<Value, VMError> {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Boolean(a <= b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Boolean(a <= b)),
        (Value::Integer(a), Value::Float(b)) => Ok(Value::Boolean((*a as f64) <= *b)),
        (Value::Float(a), Value::Integer(b)) => Ok(Value::Boolean(*a <= (*b as f64))),
        _ => Err(VMError::TypeMismatch(format!(
            "Cannot compare {:?} <= {:?}",
            a, b
        ))),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_integers() {
        let result = add(&Value::Integer(1), &Value::Integer(2)).unwrap();
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn add_floats() {
        let result = add(&Value::Float(1.5), &Value::Float(2.5)).unwrap();
        assert_eq!(result, Value::Float(4.0));
    }

    #[test]
    fn add_mixed_int_float() {
        let result = add(&Value::Integer(1), &Value::Float(2.5)).unwrap();
        assert_eq!(result, Value::Float(3.5));
    }

    #[test]
    fn add_strings() {
        let result = add(
            &Value::String("hello".to_string()),
            &Value::String(" world".to_string()),
        )
        .unwrap();
        assert_eq!(result, Value::String("hello world".to_string()));
    }

    #[test]
    fn add_string_and_integer() {
        let result = add(&Value::String("count: ".to_string()), &Value::Integer(42)).unwrap();
        assert_eq!(result, Value::String("count: 42".to_string()));
    }

    #[test]
    fn subtract_integers() {
        let result = subtract(&Value::Integer(5), &Value::Integer(3)).unwrap();
        assert_eq!(result, Value::Integer(2));
    }

    #[test]
    fn multiply_integers() {
        let result = multiply(&Value::Integer(3), &Value::Integer(4)).unwrap();
        assert_eq!(result, Value::Integer(12));
    }

    #[test]
    fn divide_integers_produces_float() {
        let result = divide(&Value::Integer(7), &Value::Integer(2)).unwrap();
        assert_eq!(result, Value::Float(3.5));
    }

    #[test]
    fn divide_by_zero_integer() {
        let result = divide(&Value::Integer(1), &Value::Integer(0));
        assert!(result.is_err());
    }

    #[test]
    fn divide_by_zero_float() {
        let result = divide(&Value::Float(1.0), &Value::Float(0.0));
        assert!(result.is_err());
    }

    #[test]
    fn type_mismatch_on_subtract_booleans() {
        let result = subtract(&Value::Boolean(true), &Value::Boolean(false));
        assert!(result.is_err());
    }

    #[test]
    fn equals_same_type() {
        let result = equals(&Value::Integer(1), &Value::Integer(1)).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn not_equals_same_type() {
        let result = not_equals(&Value::Integer(1), &Value::Integer(2)).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn equals_different_types_is_error() {
        let result = equals(&Value::Integer(1), &Value::Float(1.0));
        assert!(result.is_err());
    }

    #[test]
    fn comparison_operators() {
        assert_eq!(
            greater_than(&Value::Integer(2), &Value::Integer(1)).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            less_than(&Value::Integer(1), &Value::Integer(2)).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            greater_than_or_equal(&Value::Integer(2), &Value::Integer(2)).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            less_than_or_equal(&Value::Integer(2), &Value::Integer(2)).unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn comparison_mixed_int_float() {
        assert_eq!(
            greater_than(&Value::Integer(3), &Value::Float(2.5)).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            less_than(&Value::Float(1.5), &Value::Integer(2)).unwrap(),
            Value::Boolean(true)
        );
    }
}
