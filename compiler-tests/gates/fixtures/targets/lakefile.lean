import Lake
open Lake DSL

package targets

@[default_target]
lean_lib Shapes where
  roots := #[`Shapes.Circle, `Shapes.Square]

@[default_target]
lean_exe tool where
  root := `Tool
