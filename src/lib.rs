pub mod parser;
pub mod runtime;
pub mod scanner;
pub mod skia;
pub mod tree;
pub mod vm;

/// Convenience function to compile an Ogham source file into a UI.
///
/// This is the simplest way to use Ogham as a library. It handles the complete
/// compilation pipeline: reading the file, scanning, parsing, executing, and
/// converting to a UI widget tree.
///
/// # Arguments
///
/// * `path` - Path to the Ogham source file (`.ogh` extension)
///
/// # Returns
///
/// Returns a `UI` instance on success, or a `runtime::RuntimeError` if any stage fails.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
///
/// let ui = ogham::compile_file("ui.ogh")?;
/// # Ok(())
/// # }
/// ```
pub fn compile_file<P: AsRef<std::path::Path>>(
    path: P,
) -> Result<tree::UI, runtime::RuntimeError> {
    runtime::from_file(path, None)
}

/// Convenience function to compile Ogham source code from a string into a UI.
///
/// This function handles the complete compilation pipeline: scanning, parsing,
/// executing, and converting to a UI widget tree.
///
/// # Arguments
///
/// * `source` - The Ogham source code as a string
///
/// # Returns
///
/// Returns a `UI` instance on success, or a `runtime::RuntimeError` if any stage fails.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
///
/// let source = r#"
///     fn main() {
///         return flex {
///             children: [text { text: "Hello, World!" }]
///         }
///     }
/// "#;
///
/// let ui = ogham::compile_source(source)?;
/// # Ok(())
/// # }
/// ```
pub fn compile_source(source: &str) -> Result<tree::UI, runtime::RuntimeError> {
    runtime::from_source(source, None)
}

/// Create a new UI from a widget reference.
///
/// This is a convenience function that allows `ogham::new(widget)` syntax.
/// It's equivalent to `tree::UI::new(widget)`.
pub fn new(widget: tree::WidgetRef) -> tree::UI {
    tree::UI::new(widget)
}

/// Watch a file for changes and compile it.
///
/// This is a convenience function that creates a file watcher and compiles
/// the file in one step. It returns both the UI and the watcher so you can
/// monitor for changes and recompile as needed.
///
/// # Arguments
///
/// * `path` - Path to the Ogham source file to watch and compile
///
/// # Returns
///
/// Returns a tuple of `(UI, FileWatcher)` on success, or a `runtime::RuntimeError`
/// if compilation or watcher creation fails.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
///
/// let (mut ui, mut watcher) = ogham::watch_file("ui.ogh")?;
///
/// // In your event loop:
/// if watcher.check_for_changes() {
///     ui = watcher.recompile(None)?;
/// }
/// # Ok(())
/// # }
/// ```
pub fn watch_file<P: AsRef<std::path::Path>>(
    path: P,
) -> Result<(tree::UI, runtime::FileWatcher), runtime::RuntimeError> {
    runtime::watch_and_compile(path, None)
}
