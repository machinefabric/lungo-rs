use lungo::{Int, List, Nat};
use serde_json::json;
use shaping::drawing;
use shaping::geometry::{self, Point, Secret, Shape};

fn point(x: i64, y: i64) -> Point {
    Point { x: Int::from(x), y: Int::from(y) }
}

/// TEST0065: attributes shape the serialized form
#[test]
fn test0065_attributes_shape_the_serialized_form() {
    let p = geometry::translate(point(1, 2), Int::from(-3), Int::from(10));
    assert_eq!(serde_json::to_value(&p).unwrap(), json!({ "col": "-2", "y": "12" }));

    let shape = Shape::Segment { start: point(0, 0), stop: p.clone() };
    let value = serde_json::to_value(&shape).unwrap();
    assert_eq!(
        value,
        json!({ "kind": "segment", "from": { "col": "0", "y": "0" }, "stop": { "col": "-2", "y": "12" } })
    );
    assert_eq!(serde_json::from_value::<Shape>(value).unwrap(), shape);
}

/// TEST0066: struct attributes apply to their struct
#[test]
fn test0066_struct_attributes_apply_to_their_struct() {
    let big = "-170141183460469231731687303715884105729";
    let exact: Point = serde_json::from_value(json!({ "col": big, "y": -4 })).unwrap();
    assert_eq!(exact.x.to_string(), big);
    assert_eq!(exact.y, Int::from(-4));
    let extra = serde_json::from_value::<Point>(json!({ "col": 1, "y": 2, "z": 3 }));
    assert!(extra.unwrap_err().to_string().contains("unknown field `z`"));
}

/// TEST0067: skipped debug leaves the implementation to the application
#[test]
fn test0067_skipped_debug_leaves_the_implementation_to_the_application() {
    let secret = Secret { code: Nat::from(21u32) };
    assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
    assert_eq!(geometry::reveal(secret), Nat::from(42u32));
}

/// TEST0068: disabled comments are omitted only where selected
#[test]
fn test0068_disabled_comments_are_omitted_only_where_selected() {
    let generated = include_str!(concat!(env!("OUT_DIR"), "/lungo/geometry/geometry.rs"));
    assert!(generated.contains("/// Lean: `Geometry.translate"));
    assert!(!generated.contains("Lean: `Geometry.reveal"));
}

/// TEST0069: extern types share values between generated modules
#[test]
fn test0069_extern_types_share_values_between_generated_modules() {
    let mid: Point = drawing::midpoint(point(-4, 2), point(8, 6));
    assert_eq!(mid, point(2, 4));
    let outline: List<Point> = drawing::outline(Shape::Dot { at: mid.clone() });
    assert_eq!(outline.into_iter().collect::<Vec<_>>(), vec![mid, geometry::origin()]);
}
