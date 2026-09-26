fn main() -> lungo_build::Result<()> {
    let serde = "#[derive(serde::Serialize, serde::Deserialize)]";
    lungo_build::configure()
        .type_attribute("Geometry", serde)
        .struct_attribute("Geometry.Point", "#[serde(deny_unknown_fields)]")
        .enum_attribute("Geometry.Shape", r#"#[serde(tag = "kind", rename_all = "snake_case")]"#)
        .field_attribute("Geometry.Point.x", r#"#[serde(rename = "col")]"#)
        .field_attribute("Geometry.Shape.segment.start", r#"#[serde(rename = "from")]"#)
        .skip_debug(["Geometry.Secret"])
        .disable_comments(["Geometry.reveal"])
        .compile_lean("lean/geometry")?;
    lungo_build::configure()
        .extern_type("Geometry.Point", "crate::geometry::Point")
        .extern_type("Geometry.Shape", "crate::geometry::Shape")
        .compile_lean("lean/drawing")
}
