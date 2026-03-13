/// Construct a `Value::Map` from key-value pairs, calling `.into_ogham_value()`
/// on each value.
///
/// # Example
/// ```ignore
/// use ogham::{ogham_map, runtime::value::Value};
///
/// let val = ogham_map! {
///     "name" => "Alice",
///     "age" => 30,
///     "active" => true,
/// };
/// ```
#[macro_export]
macro_rules! ogham_map {
    ($($key:expr => $value:expr),* $(,)?) => {{
        use $crate::runtime::value::IntoOghamValue;
        let mut map = ::std::collections::HashMap::new();
        $(
            map.insert($key.into(), IntoOghamValue::into_ogham_value($value));
        )*
        $crate::runtime::value::Value::Map(map)
    }};
}
