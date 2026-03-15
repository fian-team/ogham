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
use crate::widget::builder::WidgetRegistry;

pub mod compiler;
pub mod config;
pub mod environment;
pub mod error;
pub mod opcode;
pub mod ops;
pub mod value;
pub mod vm;
pub mod descriptor;

/// Built-in prelude source that defines helper functions available in all modules.
/// Compiled and executed once when the runtime is first created via
/// `Runtime::from_source`/`Runtime::from_file`.
const PRELUDE_SOURCE: &str = r#"
let rgb = fn (r, g, b) { { r: r, g: g, b: b, a: 255 } };
let rgba = fn (r, g, b, a) { { r: r, g: g, b: b, a: a } };
"#;

/// Manages component state and the call stack used for state key generation.
pub(crate) struct StateManager {
    pub(crate) component_state: HashMap<String, Value>,
    pub(crate) call_stack: Vec<String>,
    pub(crate) active_state_paths: HashSet<String>,
    pub(crate) has_branched: bool,
    pub(crate) call_counters: HashMap<String, usize>,
}

impl StateManager {
    fn new() -> Self {
        Self {
            component_state: HashMap::new(),
            call_stack: Vec::new(),
            active_state_paths: HashSet::new(),
            has_branched: false,
            call_counters: HashMap::new(),
        }
    }

    /// Get the current call stack path as a string.
    pub(crate) fn get_call_stack_path(&self) -> String {
        if self.call_stack.is_empty() {
            "".to_string()
        } else {
            self.call_stack.join("/")
        }
    }

    /// Generate a state key from the current call stack path and variable name.
    pub(crate) fn get_state_key(&self, variable_name: &str) -> String {
        let path = self.get_call_stack_path();
        if path.is_empty() {
            format!(":{}", variable_name)
        } else {
            format!("{}:{}", path, variable_name)
        }
    }

    /// Build a key from a given path slice and variable name.
    fn make_key(path: &[String], variable_name: &str) -> String {
        if path.is_empty() {
            format!(":{}", variable_name)
        } else {
            format!("{}:{}", path.join("/"), variable_name)
        }
    }

    /// Search up the call stack for an existing state key.
    fn find_existing_key(&self, variable_name: &str) -> Option<String> {
        let mut search_path = self.call_stack.clone();
        loop {
            let key = Self::make_key(&search_path, variable_name);
            if self.component_state.contains_key(&key) {
                return Some(key);
            }
            if search_path.is_empty() {
                break;
            }
            search_path.pop();
        }
        None
    }

    /// Get state value for a variable, searching up the call stack.
    pub(crate) fn get_state_value(&self, variable_name: &str) -> Option<Value> {
        self.find_existing_key(variable_name)
            .and_then(|key| self.component_state.get(&key).cloned())
    }

    /// Set state value for a variable at the most specific existing path,
    /// or create it at the current path if it doesn't exist.
    pub(crate) fn set_state_value(&mut self, variable_name: &str, value: Value) {
        if let Some(key) = self.find_existing_key(variable_name) {
            self.component_state.insert(key, value);
        } else {
            let key = self.get_state_key(variable_name);
            self.component_state.insert(key, value);
        }
    }

    /// Cleanup state for components that are no longer mounted.
    fn cleanup_unmounted_state(&mut self) {
        let keys_to_remove: Vec<String> = self
            .component_state
            .keys()
            .filter(|key| {
                if let Some(colon_pos) = key.find(':') {
                    let path = &key[..colon_pos];
                    !self.active_state_paths.contains(path)
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        for key in keys_to_remove {
            self.component_state.remove(&key);
        }
    }
}

/// Manages import resolution, caching, and cycle detection.
pub(crate) struct ImportResolver {
    pub(crate) project_root: Option<PathBuf>,
    pub(crate) import_paths: HashMap<String, PathBuf>,
    loading_stack: Vec<PathBuf>,
    loaded: HashSet<PathBuf>,
    cache: HashMap<PathBuf, Environment>,
}

impl ImportResolver {
    fn new() -> Self {
        Self {
            project_root: None,
            import_paths: HashMap::new(),
            loading_stack: Vec::new(),
            loaded: HashSet::new(),
            cache: HashMap::new(),
        }
    }

    /// Returns the canonical paths of all modules that were imported.
    pub(crate) fn get_imported_paths(&self) -> Vec<PathBuf> {
        self.loaded.iter().cloned().collect()
    }
}

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
    pub(crate) state: StateManager,
    pub(crate) imports: ImportResolver,
    /// Screen dimensions set from the most recent `layout()` call.
    /// Exposed as built-in variables `screen_width` and `screen_height` in the VM.
    pub(crate) screen_width: f32,
    pub(crate) screen_height: f32,
    /// Registry of widget type names to factory functions. Populated with
    /// built-in types by default; host applications can add custom widgets
    /// via [`RuntimeConfig::with_widget`].
    pub widget_registry: WidgetRegistry,
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
            state: StateManager::new(),
            imports: ImportResolver::new(),
            screen_width: 0.0,
            widget_registry: WidgetRegistry::with_defaults(),
            screen_height: 0.0,
        }
    }

    pub fn set_project_root(&mut self, path: PathBuf) {
        self.imports.project_root = Some(path);
    }

    pub fn project_root(&self) -> Option<&PathBuf> {
        self.imports.project_root.as_ref()
    }

    pub fn set_import_paths(&mut self, paths: HashMap<String, PathBuf>) {
        self.imports.import_paths = paths;
    }

    /// Update the screen dimensions exposed as built-in variables.
    /// Called from `UI::layout()` each frame.
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Read an Ogham state variable by name. Returns `None` if the variable
    /// doesn't exist in the component state.
    pub fn get_state(&self, name: &str) -> Option<Value> {
        self.state.get_state_value(name)
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

    pub(crate) fn register_event_handler_arc(
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
        self.imports.get_imported_paths()
    }

    pub fn rerender(&mut self) -> Result<Value, VMError> {
        if self.module.is_none() {
            return Err(VMError::InvalidOperation(
                "No module stored in runtime. Cannot rerender.".to_string(),
            ));
        }
        self.needs_rerender = false;
        self.environment = Environment::new();
        self.state.active_state_paths.clear();
        self.state.call_stack.clear();
        self.state.call_counters.clear();
        let result = self.execute_module_cached();
        self.state.cleanup_unmounted_state();
        result
    }

    pub fn execute_module(&mut self, module: &Function) -> Result<Value, VMError> {
        self.state.active_state_paths.clear();
        self.state.call_stack.clear();
        self.state.call_counters.clear();
        self.imports.loading_stack.clear();
        self.imports.loaded.clear();
        self.imports.cache.clear();

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
        self.state.cleanup_unmounted_state();
        result
    }

    /// Re-execute a module using cached bytecode and cached imports.
    fn execute_module_cached(&mut self) -> Result<Value, VMError> {
        self.state.active_state_paths.clear();
        self.state.call_stack.clear();
        self.state.call_counters.clear();
        self.imports.loading_stack.clear();
        // NOTE: import_loaded and import_cache are intentionally preserved
        // so that imports are resolved from memory rather than disk.

        let proto = self.compiled_module.clone().ok_or_else(|| {
            VMError::InvalidOperation(
                "No compiled module cached. Cannot execute_module_cached.".to_string(),
            )
        })?;

        let mut vm = VM::new();
        let result = vm.run(&proto, self);
        self.state.cleanup_unmounted_state();
        result
    }

    pub(crate) fn execute_import(
        &mut self,
        import_stmt: &ImportStatement,
    ) -> Result<Value, VMError> {
        let project_root = self.imports.project_root.as_ref().ok_or_else(|| {
            VMError::ImportError("project root not set; cannot resolve import path".to_string())
        })?;

        let path_str = import_stmt.get_path();

        let mut resolved = None;
        for (prefix, base) in &self.imports.import_paths {
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

        if self.imports.loading_stack.contains(&key) {
            let mut cycle = self.imports.loading_stack.clone();
            cycle.push(key.clone());
            return Err(VMError::ImportCycle(cycle));
        }
        if self.imports.loaded.contains(&key) {
            if let Some(cached_env) = self.imports.cache.get(&key) {
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

        let source = fs::read_to_string(&resolved).map_err(|e| {
            VMError::ImportError(format!("failed to read {}: {}", resolved.display(), e))
        })?;

        self.imports.loading_stack.push(key.clone());

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
            self.imports.loading_stack.pop();
            e
        })?;
        let mut vm = VM::new();
        let _result = vm.run(&proto, self).map_err(|e| {
            self.imports.loading_stack.pop();
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
                    self.imports.loading_stack.pop();
                    return Err(VMError::ImportError(format!(
                        "export '{}' not found in {}",
                        name,
                        resolved.display()
                    )));
                }
            }
        }

        self.imports.cache.insert(key.clone(), temp_env.clone());

        if let Err(conflict_name) =
            self.environment
                .copy_from(&temp_env, names_to_copy.as_deref(), false)
        {
            self.imports.loading_stack.pop();
            return Err(VMError::ImportConflict(conflict_name));
        }

        self.imports.loading_stack.pop();
        self.imports.loaded.insert(key);

        Ok(Value::Void)
    }

    /// Execute the built-in prelude, injecting `rgb` and `rgba` helpers into host state.
    fn execute_prelude(&mut self) -> Result<(), VMError> {
        let mut scanner = Scanner::new(PRELUDE_SOURCE.to_string());
        let tokens = scanner.scan();
        let mut parser = Parser::new(tokens);
        let prelude_module = parser.parse().map_err(|e| {
            VMError::InvalidOperation(format!("prelude parse error: {}", e.message))
        })?;
        let proto = Compiler::compile_module(&prelude_module)?;
        let mut vm = VM::new();
        vm.run(&proto, self)?;
        // Move any prelude bindings from environment into host_state
        let env_vars = self.environment.top_level_variables().clone();
        for (name, value) in env_vars {
            self.inject_host_state(name, value);
        }
        self.environment = Environment::new();
        Ok(())
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

    pub fn from_file<P: AsRef<Path>>(
        path: P,
        config: Option<RuntimeConfig>,
    ) -> Result<Runtime, RuntimeError> {
        let path_buf = path.as_ref().to_path_buf();
        let source = fs::read_to_string(&path_buf)?;
        let mut runtime = Self::from_source(&source, config)?;
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

    pub fn from_source(
        source: &str,
        config: Option<RuntimeConfig>,
    ) -> Result<Runtime, RuntimeError> {
        let mut scanner = Scanner::new(source.to_string());
        let tokens = scanner.scan();

        let mut parser = Parser::new(tokens);
        let module = parser.parse()?;

        let mut runtime = Runtime::new();

        if let Err(e) = runtime.execute_prelude() {
            eprintln!("[ogham] prelude error: {:?}", e);
        }

        if let Some(config) = config.as_ref() {
            if let Some(ref state) = config.host_state.as_ref() {
                for (name, value) in state.iter() {
                    runtime.inject_host_state(name.clone(), value.clone());
                }
            }

            for (name, handler) in config.event_handlers.iter() {
                runtime.register_event_handler_arc(name.clone(), handler.clone());
            }

            if let Some(ref project_root) = config.project_root {
                runtime.set_project_root(project_root.clone());
            }

            if !config.import_paths.is_empty() {
                runtime.set_import_paths(config.import_paths.clone());
            }

            for (name, factory) in &config.custom_widgets {
                runtime
                    .widget_registry
                    .factories
                    .insert(name.clone(), factory.clone());
            }
        }

        runtime.set_module(module.clone());

        Ok(runtime)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> Value {
        let mut runtime =
            Runtime::from_source(source, None).expect("parse and create runtime");
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
        let mut runtime =
            Runtime::from_source(source, None).expect("parse and create runtime");
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
        let mut runtime =
            Runtime::from_source(source, None).expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        let result = runtime.execute_module(&module).expect("execute");
        assert_eq!(result, Value::Integer(5));
    }
}
