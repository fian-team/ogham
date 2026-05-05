//! Phase 2 M3 — Portal widget integration tests.
//!
//! Tests focus on the widget-level surface (PortalWidget
//! construction via builder, as_portal() detection, owned_path
//! plumbing, type-check rejections). Two-pass paint and
//! hit-test ordering at the renderer level need a Skia surface
//! to fully validate; they're exercised at M5 via the UL
//! escape-menu integration.

use std::sync::{Arc, Mutex};

use ogham::runtime::value::Value;
use ogham::runtime::Runtime;
use ogham::widget::{builder, portal_widget::PortalWidget, PortalInfo, Widget};

// BridgeError doesn't impl Debug for its WidgetRef-bearing
// VMError variant — wrap with a helper that flattens the
// useful kinds (InvalidPropertyType is the only one tests
// reach).
struct DebugBridgeError<'a>(&'a builder::BridgeError);
impl<'a> std::fmt::Debug for DebugBridgeError<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            builder::BridgeError::InvalidWidgetType(s) => write!(f, "InvalidWidgetType({})", s),
            builder::BridgeError::MissingProperty(s) => write!(f, "MissingProperty({})", s),
            builder::BridgeError::InvalidPropertyType(n, m) => {
                write!(f, "InvalidPropertyType({}, {})", n, m)
            }
            builder::BridgeError::VMError(_) => write!(f, "VMError(...)"),
        }
    }
}

fn build_root(src: &str) -> ogham::widget::WidgetRef {
    let runtime = Arc::new(Mutex::new(
        Runtime::from_source(src, None).expect("parse"),
    ));
    let module = {
        let rt = runtime.lock().unwrap();
        rt.get_module().expect("module").clone()
    };
    let widget_value = {
        let mut rt = runtime.lock().unwrap();
        rt.execute_module(&module).expect("execute")
    };
    let registry = ogham::widget::builder::WidgetRegistry::with_defaults();
    builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value)
        .expect("build widget tree")
}

#[test]
fn portal_widget_reports_as_portal_when_open() {
    let mut p = PortalWidget::new();
    p.open = true;
    p.focus_trap = true;
    let info = p.as_portal().expect("portal should report as_portal");
    assert!(info.open);
    assert!(info.focus_trap);
}

#[test]
fn portal_widget_zero_dimensions_in_parent_flow() {
    // Portal contributes 0×0 to the parent's flex layout;
    // children paint into the viewport in Pass B regardless.
    let p = PortalWidget::new();
    assert_eq!(p.get_fixed_width(), Some(0.0));
    assert_eq!(p.get_fixed_height(), Some(0.0));
}

#[test]
fn closed_portal_with_ghosts_still_exposes_children() {
    // M3 audit fix: when a portal closes but its inner
    // children are still ghosting through their exit
    // animation, get_children() must continue to expose them
    // so the renderer paints them in Pass B until drain. The
    // renderer detects close-with-ghosts via the is_exiting
    // check and pushes to portal_layer with focus_trap=false
    // (only fully-open portals trap focus).
    let mut p = PortalWidget::new();
    p.open = false;
    p.inner.children.push(Arc::new(Mutex::new(
        ogham::widget::flex_widget::FlexWidget::new(),
    )));
    assert_eq!(
        p.get_children().len(),
        1,
        "closed portal still exposes inner children for ghost-paint"
    );
}

#[test]
fn portal_built_from_ogh_with_open_true() {
    let src = r#"
let main = fn () {
  Portal {
    open: true,
    focus_trap: false,
    children: [Flex { children: [] }],
  }
};
"#;
    let root = build_root(src);
    let widget = root.lock().unwrap();
    let info = widget.as_portal();
    assert!(info.is_some(), "root should be a portal");
    let info = info.unwrap();
    assert!(info.open, "open should be true from descriptor");
    assert!(!info.focus_trap, "focus_trap should be false");
    assert!(!widget.get_children().is_empty(), "open portal exposes children");
}

#[test]
fn portal_built_from_ogh_with_focus_trap_true() {
    let src = r#"
let main = fn () {
  Portal {
    open: true,
    focus_trap: true,
    children: [Flex { children: [] }],
  }
};
"#;
    let root = build_root(src);
    let widget = root.lock().unwrap();
    let info = widget.as_portal().expect("is portal");
    assert!(info.focus_trap, "focus_trap from descriptor");
}

#[test]
fn portal_open_defaults_to_false_when_omitted() {
    let src = r#"
let main = fn () {
  Portal {
    children: [],
  }
};
"#;
    let root = build_root(src);
    let widget = root.lock().unwrap();
    let info = widget.as_portal().expect("is portal");
    assert!(!info.open, "missing 'open' defaults to false");
    assert!(!info.focus_trap);
}

#[test]
fn portal_rejects_non_array_children() {
    let src = r#"
let main = fn () {
  Portal {
    open: true,
    children: 42,
  }
};
"#;
    // The builder runs at execute_module's wrap-up — but
    // execute_module returns the widget VALUE, then the host
    // (lib::Ogham::new) calls widget_value_to_widget_ref,
    // which is where the type-check fires. Replicate that here.
    let runtime = Arc::new(Mutex::new(
        Runtime::from_source(src, None).expect("parse"),
    ));
    let module = {
        let rt = runtime.lock().unwrap();
        rt.get_module().expect("module").clone()
    };
    let widget_value = {
        let mut rt = runtime.lock().unwrap();
        rt.execute_module(&module).expect("execute")
    };
    let registry = ogham::widget::builder::WidgetRegistry::with_defaults();
    let result = builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value);
    let err = match result {
        Ok(_) => panic!("non-array children should be rejected"),
        Err(e) => format!("{:?}", DebugBridgeError(&e)),
    };
    assert!(
        err.contains("children") && err.contains("array"),
        "diagnostic should mention children/array; got {}",
        err
    );
}

#[test]
fn portal_rejects_non_widget_array_element() {
    let src = r#"
let main = fn () {
  Portal {
    open: true,
    children: [42],
  }
};
"#;
    let runtime = Arc::new(Mutex::new(
        Runtime::from_source(src, None).expect("parse"),
    ));
    let module = {
        let rt = runtime.lock().unwrap();
        rt.get_module().expect("module").clone()
    };
    let widget_value = {
        let mut rt = runtime.lock().unwrap();
        rt.execute_module(&module).expect("execute")
    };
    let registry = ogham::widget::builder::WidgetRegistry::with_defaults();
    let result = builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value);
    if result.is_ok() {
        panic!("non-widget array element should be rejected");
    }
}

#[test]
fn portal_owns_path_prefix_for_state_cleanup() {
    let src = r#"
let panel = fn () {
  Portal {
    open: true,
    children: [Flex { children: [] }],
  }
};
let main = fn () { panel() };
"#;
    let root = build_root(src);
    let widget = root.lock().unwrap();
    // The Portal owns the call-stack path of the panel
    // function that produced its descriptor. The exact path
    // identifier is implementation detail; the contract is
    // non-empty.
    let prefix = widget.owned_path_prefix();
    assert!(
        !prefix.is_empty(),
        "portal produced inside panel() should have a path prefix; got {:?}",
        prefix
    );
}

#[test]
fn portal_open_toggle_reconciles_children() {
    // Verify that flipping `open` from true to false reconciles
    // the children out (no panic, no visible change to the
    // inner Flex's children Vec — exit-cycle handled by
    // begin_exit/drain elsewhere).
    let src = r#"
let main = fn () {
  Portal {
    open: gate,
    children: [Flex { children: [] }],
  }
};
"#;
    let runtime = Arc::new(Mutex::new(
        Runtime::from_source(src, None).expect("parse"),
    ));
    {
        let mut rt = runtime.lock().unwrap();
        rt.inject_host_state("gate".to_string(), Value::Boolean(true));
    }
    let module = {
        let rt = runtime.lock().unwrap();
        rt.get_module().expect("module").clone()
    };
    let widget_value = {
        let mut rt = runtime.lock().unwrap();
        rt.execute_module(&module).expect("execute")
    };
    let registry = ogham::widget::builder::WidgetRegistry::with_defaults();
    let root = builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value)
        .expect("build");

    {
        let widget = root.lock().unwrap();
        let info = widget.as_portal().expect("is portal");
        assert!(info.open);
    }

    // Flip the gate, re-execute, reconcile.
    {
        let mut rt = runtime.lock().unwrap();
        rt.inject_host_state("gate".to_string(), Value::Boolean(false));
    }
    let widget_value2 = {
        let mut rt = runtime.lock().unwrap();
        rt.rerender().expect("rerender")
    };
    let new_root =
        builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value2)
            .expect("rebuild");
    {
        let mut root_guard = root.lock().unwrap();
        root_guard.update(new_root);
        let info = root_guard.as_portal().expect("still portal");
        assert!(!info.open, "open should now be false");
    }
}
