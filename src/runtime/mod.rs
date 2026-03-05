//! Ogham runtime: bytecode compiler, VM, and supporting infrastructure.
//!
//! The primary execution path compiles Ogham source to bytecode
//! ([`compiler`]) and runs it in a stack-based VM ([`vm`]).
//! Shared arithmetic and comparison operations live in [`ops`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::parser::{Function, ImportStatement, Parser};
use crate::runtime::compiler::Compiler;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::environment::Environment;
use crate::runtime::error::{RuntimeError, VMError};
use crate::runtime::opcode::FunctionProto;
use crate::runtime::value::Value;
use crate::runtime::vm::VM;
use crate::scanner::Scanner;

pub mod compiler;
pub mod config;
pub mod environment;
pub mod error;
pub mod opcode;
pub mod ops;
pub mod value;
pub mod vm;
pub mod widget;

/// The Ogham runtime: holds module state, component state, import caches,
/// and the environment used during execution.
pub struct Runtime {
    pub(crate) environment: Environment,
    host_state: HashMap<String, Value>,
    event_handlers: HashMap<String, Arc<dyn Fn(&[Value]) -> bool + Send + Sync>>,
    needs_rerender: bool,
    module: Option<Function>,
    /// Compiled bytecode for the module (cached so rerenders skip compilation).
    compiled_module: Option<FunctionProto>,
    // State management
    pub(crate) component_state: HashMap<String, Value>, // Key format: "{call_stack_path}:{variable_name}"
    pub(crate) call_stack: Vec<String>,                 // Current execution path
    pub(crate) active_state_paths: HashSet<String>, // Paths that declared state in current render
    pub(crate) has_branched: bool, // Track if branching has occurred in current function
    pub(crate) call_counters: HashMap<String, usize>, // Track call counts per function to generate unique paths
    // Import resolution
    project_root: Option<PathBuf>,
    import_paths: HashMap<String, PathBuf>,
    import_loading_stack: Vec<PathBuf>,
    import_loaded: HashSet<PathBuf>,
    /// Cached environment per resolved path so re-imports (e.g. view_two importing button)
    /// can merge that module's exports into the current scope without re-executing.
    import_cache: HashMap<PathBuf, Environment>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            host_state: HashMap::new(),
            event_handlers: HashMap::new(),
            needs_rerender: false,
            module: None,
            compiled_module: None,
            component_state: HashMap::new(),
            call_stack: Vec::new(),
            active_state_paths: HashSet::new(),
            has_branched: false,
            call_counters: HashMap::new(),
            project_root: None,
            import_paths: HashMap::new(),
            import_loading_stack: Vec::new(),
            import_loaded: HashSet::new(),
            import_cache: HashMap::new(),
        }
    }

    pub fn set_project_root(&mut self, path: PathBuf) {
        self.project_root = Some(path);
    }

    pub fn project_root(&self) -> Option<&PathBuf> {
        self.project_root.as_ref()
    }

    pub fn set_import_paths(&mut self, paths: HashMap<String, PathBuf>) {
        self.import_paths = paths;
    }

    pub fn inject_host_state(&mut self, name: String, value: Value) {
        self.host_state.insert(name, value);
    }

    /// Like `inject_host_state`, but only inserts the value when it differs
    /// from the currently stored value (avoiding unnecessary HashMap churn
    /// on every frame when state hasn't changed).
    pub fn inject_host_state_if_changed(&mut self, name: String, value: Value) {
        if self.host_state.get(&name) != Some(&value) {
            self.host_state.insert(name, value);
        }
    }

    pub fn get_host_state(&self, name: &str) -> Option<Value> {
        self.host_state.get(name).cloned()
    }

    /// Inject multiple host state values at once. Only values that differ
    /// from the currently stored value are inserted, and `request_rerender`
    /// is called automatically if anything changed.
    pub fn inject_host_state_batch(
        &mut self,
        values: impl IntoIterator<Item = (String, Value)>,
    ) {
        let mut changed = false;
        for (key, value) in values {
            if self.host_state.get(&key) != Some(&value) {
                self.host_state.insert(key, value);
                changed = true;
            }
        }
        if changed {
            self.request_rerender();
        }
    }

    pub fn register_event_handler<S, F>(&mut self, name: S, handler: F)
    where
        S: Into<String>,
        F: Fn(&[Value]) -> bool + Send + Sync + 'static,
    {
        self.event_handlers.insert(name.into(), Arc::new(handler));
    }

    pub fn register_event_handler_arc(
        &mut self,
        name: String,
        handler: Arc<dyn Fn(&[Value]) -> bool + Send + Sync>,
    ) {
        self.event_handlers.insert(name, handler);
    }

    pub fn emit_event(&self, name: &str, args: &[Value]) -> bool {
        self.event_handlers
            .get(name)
            .map(|handler| handler(args))
            .unwrap_or(false)
    }

    pub fn needs_rerender(&self) -> bool {
        self.needs_rerender
    }

    pub fn request_rerender(&mut self) {
        self.needs_rerender = true;
    }

    pub fn clear_rerender_flag(&mut self) {
        self.needs_rerender = false;
    }

    pub fn set_module(&mut self, module: Function) {
        self.module = Some(module);
        // Invalidate cached bytecode so the next execution recompiles.
        self.compiled_module = None;
    }

    pub fn get_module(&self) -> Option<&Function> {
        self.module.as_ref()
    }

    /// Returns the canonical paths of all modules that were imported during the last
    /// execute_module/rerender. Used by the file watcher to watch every file that
    /// affects the current UI.
    pub fn get_imported_paths(&self) -> Vec<PathBuf> {
        self.import_loaded.iter().cloned().collect()
    }

    pub fn rerender(&mut self) -> Result<Value, VMError> {
        if self.module.is_none() {
            return Err(VMError::InvalidOperation(
                "No module stored in runtime. Cannot rerender.".to_string(),
            ));
        }
        // Clear the rerender flag before re-executing
        self.needs_rerender = false;
        // Reset environment to module level
        self.environment = Environment::new();
        // Clear active state paths for new render
        self.active_state_paths.clear();
        // Clear call stack
        self.call_stack.clear();
        // Reset call counters for new render
        self.call_counters.clear();
        // Reuse compiled module (already cached from first execute_module).
        // Preserve import caches so that imported files are not re-read,
        // re-parsed, and re-executed from disk on every rerender. The file
        // watcher handles invalidation when files change on disk.
        let result = self.execute_module_cached();
        // Cleanup state for unmounted components
        self.cleanup_unmounted_state();
        result
    }

    /// Invalidate the cached bytecode and import caches so the next
    /// execute_module recompiles and re-imports everything from disk.
    pub fn invalidate_compiled_module(&mut self) {
        self.compiled_module = None;
        self.import_loaded.clear();
        self.import_cache.clear();
    }

    pub fn execute_module(&mut self, module: &Function) -> Result<Value, VMError> {
        // Try bytecode path first: compile the module and run it in the VM.
        self.active_state_paths.clear();
        self.call_stack.clear();
        self.call_counters.clear();
        self.import_loading_stack.clear();
        self.import_loaded.clear();
        self.import_cache.clear();

        // Compile (or use cached compilation).
        let proto = if let Some(ref cached) = self.compiled_module {
            cached.clone()
        } else {
            let proto = Compiler::compile_module(module)?;
            self.compiled_module = Some(proto.clone());
            proto
        };

        let mut vm = VM::new();
        let result = vm.run(&proto, self);
        self.cleanup_unmounted_state();
        result
    }

    /// Re-execute a module using cached bytecode and cached imports.
    ///
    /// Unlike `execute_module`, this method:
    /// - Preserves `import_loaded` and `import_cache` so that imported `.ogh`
    ///   files are resolved from memory instead of being re-read, re-parsed,
    ///   and re-executed from disk.
    /// - Requires that compiled bytecode already exists (will error otherwise).
    /// - Does not need the module AST, avoiding an expensive deep clone.
    ///
    /// Used by `rerender()` for efficient re-evaluation. The file watcher
    /// handles cache invalidation when source files change on disk.
    fn execute_module_cached(&mut self) -> Result<Value, VMError> {
        self.active_state_paths.clear();
        self.call_stack.clear();
        self.call_counters.clear();
        self.import_loading_stack.clear();
        // NOTE: import_loaded and import_cache are intentionally preserved
        // so that imports are resolved from memory rather than disk.

        let proto = self.compiled_module.clone().ok_or_else(|| {
            VMError::InvalidOperation(
                "No compiled module cached. Cannot execute_module_cached.".to_string(),
            )
        })?;

        let mut vm = VM::new();
        let result = vm.run(&proto, self);
        self.cleanup_unmounted_state();
        result
    }

    /// Get the current call stack path as a string
    pub(crate) fn get_call_stack_path(&self) -> String {
        if self.call_stack.is_empty() {
            "".to_string()
        } else {
            self.call_stack.join("/")
        }
    }

    /// Generate a state key from the current call stack path and variable name
    pub(crate) fn get_state_key(&self, variable_name: &str) -> String {
        let path = self.get_call_stack_path();
        if path.is_empty() {
            format!(":{}", variable_name)
        } else {
            format!("{}:{}", path, variable_name)
        }
    }

    /// Get state value for a variable, searching up the call stack
    pub(crate) fn get_state_value(&self, variable_name: &str) -> Option<Value> {
        // Search up the call stack, from most specific to least specific
        let mut search_path = self.call_stack.clone();

        // First try with full path
        loop {
            let path_str = if search_path.is_empty() {
                "".to_string()
            } else {
                search_path.join("/")
            };
            let key = if path_str.is_empty() {
                format!(":{}", variable_name)
            } else {
                format!("{}:{}", path_str, variable_name)
            };

            if let Some(value) = self.component_state.get(&key) {
                return Some(value.clone());
            }

            // If we've exhausted the path, break
            if search_path.is_empty() {
                break;
            }

            // Try with shorter path (pop one level)
            search_path.pop();
        }

        None
    }

    /// Set state value for a variable at the current call stack path
    /// This will update the state at the most specific path where it exists,
    /// or create it at the current path if it doesn't exist
    pub(crate) fn set_state_value(&mut self, variable_name: &str, value: Value) {
        // First, try to find existing state up the call stack
        let mut search_path = self.call_stack.clone();
        let mut found = false;

        loop {
            let path_str = if search_path.is_empty() {
                "".to_string()
            } else {
                search_path.join("/")
            };
            let key = if path_str.is_empty() {
                format!(":{}", variable_name)
            } else {
                format!("{}:{}", path_str, variable_name)
            };

            if self.component_state.contains_key(&key) {
                // Found existing state, update it
                self.component_state.insert(key, value.clone());
                found = true;
                break;
            }

            // If we've exhausted the path, break
            if search_path.is_empty() {
                break;
            }

            // Try with shorter path (pop one level)
            search_path.pop();
        }

        // If not found, create at current path
        if !found {
            let key = self.get_state_key(variable_name);
            self.component_state.insert(key, value);
        }
    }

    /// Cleanup state for components that are no longer mounted
    fn cleanup_unmounted_state(&mut self) {
        // Collect keys to remove
        let keys_to_remove: Vec<String> = self
            .component_state
            .keys()
            .filter(|key| {
                // Extract path from key (format: "{path}:{variable_name}")
                if let Some(colon_pos) = key.find(':') {
                    let path = &key[..colon_pos];
                    // Remove if path is not in active_state_paths
                    !self.active_state_paths.contains(path)
                } else {
                    // Malformed key, remove it
                    true
                }
            })
            .cloned()
            .collect();

        // Remove the keys
        for key in keys_to_remove {
            self.component_state.remove(&key);
        }
    }

    pub(crate) fn execute_import(
        &mut self,
        import_stmt: &ImportStatement,
    ) -> Result<Value, VMError> {
        let project_root = self.project_root.as_ref().ok_or_else(|| {
            VMError::ImportError("project root not set; cannot resolve import path".to_string())
        })?;

        let path_str = import_stmt.get_path();

        let mut resolved = None;
        for (prefix, base) in &self.import_paths {
            if let Some(rest) = path_str.strip_prefix(prefix.as_str()) {
                let rest = rest.strip_prefix('/').unwrap_or(rest);
                resolved = Some(base.join(rest));
                break;
            }
        }
        let mut resolved = resolved.unwrap_or_else(|| project_root.join(path_str));

        if resolved.extension().is_none() {
            resolved.set_extension("ogh");
        }

        let key = resolved.canonicalize().unwrap_or(resolved.clone());

        if self.import_loading_stack.contains(&key) {
            let mut cycle = self.import_loading_stack.clone();
            cycle.push(key.clone());
            return Err(VMError::ImportCycle(cycle));
        }
        if self.import_loaded.contains(&key) {
            // Module already loaded (e.g. button.ogh by view_one). Merge its exports
            // into the current scope so this module can use them.
            if let Some(cached_env) = self.import_cache.get(&key) {
                let names_to_copy_opt: Option<Vec<String>> = import_stmt.get_names().clone();
                if let Some(ref names) = names_to_copy_opt {
                    for name in names {
                        if cached_env.get(name).is_none() {
                            return Err(VMError::ImportError(format!(
                                "export '{}' not found in {}",
                                name,
                                resolved.display()
                            )));
                        }
                    }
                }
                if let Err(conflict_name) =
                    self.environment
                        .copy_from(cached_env, names_to_copy_opt.as_deref(), false)
                {
                    return Err(VMError::ImportConflict(conflict_name));
                }
            }
            return Ok(Value::Void);
        }

        // Read file from disk only when not cached.
        let source = fs::read_to_string(&resolved).map_err(|e| {
            VMError::ImportError(format!("failed to read {}: {}", resolved.display(), e))
        })?;

        self.import_loading_stack.push(key.clone());

        let mut scanner = Scanner::new(source);
        let tokens = scanner.scan();
        let mut parser = Parser::new(tokens);
        let imported_module = parser.parse().map_err(|e| {
            VMError::ImportError(format!(
                "parse error in {} at {}:{}: {}",
                resolved.display(),
                e.line,
                e.column,
                e.message
            ))
        })?;

        let (proto, local_names) = Compiler::compile_import(&imported_module).map_err(|e| {
            self.import_loading_stack.pop();
            e
        })?;
        let mut vm = VM::new();
        let _result = vm.run(&proto, self).map_err(|e| {
            self.import_loading_stack.pop();
            e
        })?;
        let exports_map = vm.read_stack_locals(&local_names);

        let mut temp_env = Environment::new();
        for (name, value) in exports_map {
            temp_env.define(name, value);
        }

        let names_to_copy: Option<Vec<String>> = import_stmt.get_names().clone();
        if let Some(ref names) = names_to_copy {
            for name in names {
                if temp_env.get(name).is_none() {
                    self.import_loading_stack.pop();
                    return Err(VMError::ImportError(format!(
                        "export '{}' not found in {}",
                        name,
                        resolved.display()
                    )));
                }
            }
        }

        // Cache this module's env so later re-imports (e.g. view_two importing button)
        // can merge its exports into their scope without re-executing.
        self.import_cache.insert(key.clone(), temp_env.clone());

        // Use check_conflict = false so re-exported names (e.g. button from multiple
        // view files that each import button.ogh) overwrite instead of conflicting.
        if let Err(conflict_name) =
            self.environment
                .copy_from(&temp_env, names_to_copy.as_deref(), false)
        {
            self.import_loading_stack.pop();
            return Err(VMError::ImportConflict(conflict_name));
        }

        self.import_loading_stack.pop();
        self.import_loaded.insert(key);

        Ok(Value::Void)
    }

    /// Call a bytecode closure (produced by the bytecode compiler).
    /// Used by the widget tree's event handlers.
    pub fn call_bytecode_closure(
        &mut self,
        closure: &Rc<opcode::VMClosure>,
        args: &[Value],
    ) -> Result<Value, VMError> {
        let mut vm = VM::new();
        vm.call_closure(closure, args, self)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

pub fn from_file<P: AsRef<Path>>(
    path: P,
    config: Option<RuntimeConfig>,
) -> Result<Runtime, RuntimeError> {
    let path_buf = path.as_ref().to_path_buf();
    let source = fs::read_to_string(&path_buf)?;
    let mut runtime = from_source(&source, config)?;
    if runtime.project_root().is_none() {
        runtime.set_project_root(
            path_buf
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".")),
        );
    }
    Ok(runtime)
}

pub fn from_source(source: &str, config: Option<RuntimeConfig>) -> Result<Runtime, RuntimeError> {
    // Step 1: Scan source into tokens
    let mut scanner = Scanner::new(source.to_string());
    let tokens = scanner.scan();

    // Step 2: Parse tokens into AST
    let mut parser = Parser::new(tokens);
    let module = parser.parse()?;

    // Step 3: Execute in Runtime (kept alive for UI event handlers)
    let mut runtime = Runtime::new();

    // Inject host state and config if provided
    if let Some(config) = config.as_ref() {
        if let Some(ref state) = config.host_state.as_ref() {
            for (name, value) in state.iter() {
                runtime.inject_host_state(name.clone(), value.clone());
            }
        }

        // Register per-event handlers (for `event("name", ...)`).
        for (name, handler) in config.event_handlers.iter() {
            runtime.register_event_handler_arc(name.clone(), handler.clone());
        }

        if let Some(ref project_root) = config.project_root {
            runtime.set_project_root(project_root.clone());
        }

        if !config.import_paths.is_empty() {
            runtime.set_import_paths(config.import_paths.clone());
        }
    }

    // Store the module in the runtime for potential rerendering
    runtime.set_module(module.clone());

    Ok(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> Value {
        let mut runtime = from_source(source, None).expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        runtime.execute_module(&module).expect("execute")
    }

    #[test]
    fn arithmetic_operations() {
        assert_eq!(
            run("let main = fn () { return 2 + 3; };"),
            Value::Integer(5)
        );
        assert_eq!(
            run("let main = fn () { return 10 - 4; };"),
            Value::Integer(6)
        );
        assert_eq!(
            run("let main = fn () { return 3 * 7; };"),
            Value::Integer(21)
        );
    }

    #[test]
    fn comparison_operations() {
        assert_eq!(
            run("let main = fn () { return 5 > 3; };"),
            Value::Boolean(true)
        );
        assert_eq!(
            run("let main = fn () { return 2 < 1; };"),
            Value::Boolean(false)
        );
        assert_eq!(
            run("let main = fn () { return 3 == 3; };"),
            Value::Boolean(true)
        );
    }

    #[test]
    fn string_concatenation() {
        assert_eq!(
            run(r#"let main = fn () { return "hello" + " " + "world"; };"#),
            Value::String("hello world".to_string())
        );
    }

    #[test]
    fn closures_capture_variables() {
        let source = r#"
let make_adder = fn (x: int) {
    return fn (y: int) { return x + y; };
};
let main = fn () {
    let add5 = make_adder(5);
    return add5(3);
};
"#;
        assert_eq!(run(source), Value::Integer(8));
    }

    #[test]
    fn conditional_if_else() {
        let source = r#"
let main = fn () {
    let x = 10;
    if x > 5 {
        return true;
    } else {
        return false;
    }
};
"#;
        assert_eq!(run(source), Value::Boolean(true));
    }

    #[test]
    fn for_loop_expression_produces_array() {
        let source = r#"
let main = fn () {
    let arr = [1, 2, 3];
    return arr.length();
};
"#;
        assert_eq!(run(source), Value::Integer(3));
    }

    #[test]
    fn match_expression() {
        let source = r#"
let main = fn () {
    let x = 2;
    return match x {
        1 => "one",
        2 => "two",
        _ => "other",
    };
};
"#;
        assert_eq!(run(source), Value::String("two".to_string()));
    }

    #[test]
    fn state_persists_across_rerenders() {
        let source = r#"
let main = fn () {
    state count = 0;
    count = count + 1;
    return count;
};
"#;
        let mut runtime = from_source(source, None).expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        let first = runtime.execute_module(&module).expect("first execute");
        assert_eq!(first, Value::Integer(1));
        let second = runtime.rerender().expect("rerender");
        assert_eq!(second, Value::Integer(2));
    }

    #[test]
    fn map_literals() {
        let source = r#"
let main = fn () {
    let m = { x: 10, y: 20 };
    return m.x + m.y;
};
"#;
        assert_eq!(run(source), Value::Integer(30));
    }

    #[test]
    fn nested_function_calls() {
        let source = r#"
let double = fn (n: int) { return n * 2; };
let main = fn () {
    return double(double(3));
};
"#;
        assert_eq!(run(source), Value::Integer(12));
    }

    #[test]
    fn array_access_length_and_index() {
        let source = r#"
let main = fn () {
  let array = [1, 2, 3, 4, 5];
  let last_index = array.length() - 1;
  let value = array[last_index];
  value
};
"#;
        let mut runtime = from_source(source, None).expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        let result = runtime.execute_module(&module).expect("execute");
        assert_eq!(result, Value::Integer(5));
    }
}
