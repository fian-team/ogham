//! High-level runtime API for integrating Ogham into Rust applications.
//!
//! This module provides a plug-and-play solution for executing Ogham source code
//! and converting it into executable UI components. It handles the full pipeline:
//! scanner -> parser -> VM -> UI bridge.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::parser::{Parser, SyntaxError};
use crate::scanner::Scanner;
use crate::tree::{ast_bridge, UI};
use crate::vm::{VM, VMError, Value};

/// Aggregated error type for all runtime execution stages.
#[derive(Debug)]
pub enum RuntimeError {
    /// File I/O error (file not found, permission denied, etc.)
    IoError(std::io::Error),
    /// Syntax error during parsing
    SyntaxError(SyntaxError),
    /// Runtime error during VM execution
    VmError(VMError),
    /// Error during AST to UI bridge conversion
    BridgeError(ast_bridge::BridgeError),
}

impl From<std::io::Error> for RuntimeError {
    fn from(err: std::io::Error) -> Self {
        RuntimeError::IoError(err)
    }
}

impl From<SyntaxError> for RuntimeError {
    fn from(err: SyntaxError) -> Self {
        RuntimeError::SyntaxError(err)
    }
}

impl From<VMError> for RuntimeError {
    fn from(err: VMError) -> Self {
        RuntimeError::VmError(err)
    }
}

impl From<ast_bridge::BridgeError> for RuntimeError {
    fn from(err: ast_bridge::BridgeError) -> Self {
        RuntimeError::BridgeError(err)
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::IoError(e) => write!(f, "I/O error: {}", e),
            RuntimeError::SyntaxError(e) => {
                write!(f, "Syntax error at {}:{}: {}", e.line, e.column, e.message)
            }
            RuntimeError::VmError(e) => {
                write!(f, "Runtime error: {:?}", e)
            }
            RuntimeError::BridgeError(e) => {
                write!(f, "Bridge error: {:?}", e)
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Configuration for runtime execution, allowing customization of the execution process.
///
/// This struct provides hooks for host application integration including
/// host state injection and event handling.
#[derive(Default)]
pub struct RuntimeConfig {
    /// Optional host state that can be accessed via a special keyword (e.g., `global` or `host`).
    /// This is separate from environment-scoped variables and `state` keyword values.
    ///
    /// Host state is global and persists across function calls, allowing the host application
    /// to provide data that the Ogham script can access but not modify.
    pub host_state: Option<std::collections::HashMap<String, Value>>,
    
    /// Optional callback for handling events emitted by the UI.
    /// This allows the host application to receive and process events
    /// from the Ogham UI.
    ///
    /// The callback receives the event name and any associated data.
    /// Returning `true` indicates the event was handled.
    pub event_handler: Option<Box<dyn Fn(&str, Option<&Value>) -> bool>>,
}

impl RuntimeConfig {
    /// Create a new default runtime configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set host state that can be accessed via a special keyword.
    /// This state is separate from environment variables and `state` keyword values.
    pub fn with_host_state(mut self, state: std::collections::HashMap<String, Value>) -> Self {
        self.host_state = Some(state);
        self
    }

    /// Set an event handler callback.
    pub fn with_event_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(&str, Option<&Value>) -> bool + 'static,
    {
        self.event_handler = Some(Box::new(handler));
        self
    }
}

/// Compile an Ogham source file into a UI.
///
/// This function handles the complete compilation pipeline:
/// 1. Read the source file
/// 2. Scan the source into tokens
/// 3. Parse tokens into an AST
/// 4. Execute the AST in the VM
/// 5. Convert the result to a UI widget tree
///
/// # Arguments
///
/// * `path` - Path to the Ogham source file (`.ogh` extension)
/// * `config` - Optional runtime configuration
///
/// # Returns
///
/// Returns a `UI` instance on success, or a `RuntimeError` if any stage fails.
///
/// # Example
///
/// ```no_run
/// use ogham::runtime;
///
/// let ui = runtime::from_file("ui.ogh", None)?;
/// ```
pub fn from_file<P: AsRef<Path>>(
    path: P,
    config: Option<RuntimeConfig>,
) -> Result<UI, RuntimeError> {
    let source = fs::read_to_string(path)?;
    from_source(&source, config)
}

/// Compile Ogham source code from a string into a UI.
///
/// This function handles the complete compilation pipeline:
/// 1. Scan the source into tokens
/// 2. Parse tokens into an AST
/// 3. Execute the AST in the VM
/// 4. Convert the result to a UI widget tree
///
/// # Arguments
///
/// * `source` - The Ogham source code as a string
/// * `config` - Optional runtime configuration
///
/// # Returns
///
/// Returns a `UI` instance on success, or a `RuntimeError` if any stage fails.
///
/// # Example
///
/// ```
/// use ogham::runtime;
///
/// let source = r#"
///     fn main() {
///         return flex {
///             children: [text { text: "Hello, World!" }]
///         }
///     }
/// "#;
///
/// let ui = runtime::from_source(source, None)?;
/// ```
pub fn from_source(source: &str, config: Option<RuntimeConfig>) -> Result<UI, RuntimeError> {
    // Step 1: Scan source into tokens
    let mut scanner = Scanner::new(source.to_string());
    let tokens = scanner.scan();

    // Step 2: Parse tokens into AST
    let mut parser = Parser::new(tokens);
    let module = parser.parse()?;

    // Step 3: Execute in VM
    let mut vm = VM::new();
    
    // Inject host state if provided
    if let Some(config) = config.as_ref() {
        if let Some(ref state) = config.host_state.as_ref() {
            for (name, value) in state.iter() {
                vm.inject_host_state(name.clone(), value.clone());
            }
        }
    }

    let value = vm.execute_module(&module)?;

    // Step 4: Convert VM value to UI widget
    let widget = ast_bridge::widget_value_to_widget_ref(&mut vm, &value)?;

    // Step 5: Create UI
    Ok(UI::new(widget))
}

/// File watcher for monitoring Ogham source files for changes.
///
/// This struct wraps the underlying file system watcher and provides
/// a simple API for watching a file and receiving change notifications.
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<Result<NotifyEvent, notify::Error>>,
    watched_path: PathBuf,
}

impl FileWatcher {
    /// Create a new file watcher for the specified file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to watch
    ///
    /// # Returns
    ///
    /// Returns a `FileWatcher` on success, or a `RuntimeError` if the watcher
    /// could not be created or the file path is invalid.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ogham::runtime;
    ///
    /// let watcher = runtime::FileWatcher::new("ui.ogh")?;
    /// ```
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, RuntimeError> {
        let path_buf = PathBuf::from(path.as_ref());
        
        // Verify the file exists
        if !path_buf.exists() {
            return Err(RuntimeError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", path_buf.display()),
            )));
        }

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx)
            .map_err(|e| RuntimeError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create file watcher: {}", e),
            )))?;

        // Watch the parent directory (non-recursive) to detect changes to the file
        if let Some(parent) = path_buf.parent() {
            watcher.watch(parent, RecursiveMode::NonRecursive)
                .map_err(|e| RuntimeError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to watch directory: {}", e),
                )))?;
        }

        Ok(Self {
            watcher,
            receiver: rx,
            watched_path: path_buf,
        })
    }

    /// Check if the watched file has changed.
    ///
    /// This method should be called periodically (e.g., in an event loop)
    /// to check for file changes. It returns `true` if the watched file
    /// was modified or created.
    ///
    /// # Returns
    ///
    /// Returns `true` if the watched file changed, `false` otherwise.
    /// Errors from the underlying watcher are logged but don't cause this
    /// method to return an error.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ogham::runtime;
    ///
    /// let mut watcher = runtime::FileWatcher::new("ui.ogh")?;
    /// let mut ui = runtime::from_file("ui.ogh", None)?;
    ///
    /// // In your event loop:
    /// if watcher.check_for_changes() {
    ///     // File changed, recompile
    ///     ui = runtime::from_file("ui.ogh", None)?;
    /// }
    /// ```
    pub fn check_for_changes(&self) -> bool {
        // Try to receive all pending events
        let mut file_changed = false;
        
        while let Ok(Ok(event)) = self.receiver.try_recv() {
            match event.kind {
                EventKind::Modify(_) | EventKind::Create(_) => {
                    // Check if the changed file matches our watched file
                    if event.paths.iter().any(|p| p == &self.watched_path) {
                        file_changed = true;
                    }
                }
                _ => {}
            }
        }
        
        file_changed
    }

    /// Get a reference to the watched file path.
    pub fn path(&self) -> &Path {
        &self.watched_path
    }

    /// Recompile the watched file with the given configuration.
    ///
    /// This is a convenience method that reads the file and compiles it.
    ///
    /// # Arguments
    ///
    /// * `config` - Optional runtime configuration
    ///
    /// # Returns
    ///
    /// Returns a new `UI` instance on success, or a `RuntimeError` if compilation fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ogham::runtime;
    ///
    /// let mut watcher = runtime::FileWatcher::new("ui.ogh")?;
    /// let mut ui = runtime::from_file("ui.ogh", None)?;
    ///
    /// // In your event loop:
    /// if watcher.check_for_changes() {
    ///     ui = watcher.recompile(None)?;
    /// }
    /// ```
    pub fn recompile(&self, config: Option<RuntimeConfig>) -> Result<UI, RuntimeError> {
        from_file(&self.watched_path, config)
    }
}

/// Create a file watcher and compile the file in one step.
///
/// This is a convenience function that creates a watcher and compiles
/// the file, returning both the UI and the watcher.
///
/// # Arguments
///
/// * `path` - Path to the Ogham source file to watch and compile
/// * `config` - Optional runtime configuration
///
/// # Returns
///
/// Returns a tuple of `(UI, FileWatcher)` on success, or a `RuntimeError` if
/// compilation or watcher creation fails.
///
/// # Example
///
/// ```no_run
/// use ogham::runtime;
///
/// let (mut ui, mut watcher) = runtime::watch_and_compile("ui.ogh", None)?;
///
/// // In your event loop:
/// if watcher.check_for_changes() {
///     ui = watcher.recompile(None)?;
/// }
/// ```
pub fn watch_and_compile<P: AsRef<Path>>(
    path: P,
    config: Option<RuntimeConfig>,
) -> Result<(UI, FileWatcher), RuntimeError> {
    let watcher = FileWatcher::new(&path)?;
    let ui = from_file(&path, config)?;
    Ok((ui, watcher))
}

