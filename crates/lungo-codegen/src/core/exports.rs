//! The Lean definitions the runtime calls (`lean_io_error_to_string`, the `IO.Error`
//! constructors, …; see `lungo_runtime::exports`), checked against the C signature the runtime
//! calls them with.

use crate::CodegenError;
use lungo_bir::{Declaration, IrType, Param};
use lungo_protocol::Success;
use lungo_runtime::registry::Ty;

/// A Lean definition the runtime calls as `symbol`, and how each parameter of the runtime's call
/// maps to the definition's parameter.
pub struct RuntimeExport<'a> {
    pub symbol: &'a str,
    pub decl: &'a Declaration,
    /// The definition's non-`void` parameters with the representation the runtime passes for
    /// each: an owned parameter the runtime lends must be retained, a borrowed parameter the
    /// runtime passes owned must be released after the call.
    pub params: Vec<(&'a Param, Ty)>,
}

/// Every runtime export of the program, with its signature checked.
pub fn runtime_exports(success: &Success) -> Result<Vec<RuntimeExport<'_>>, CodegenError> {
    let mut out = Vec::new();
    for export in &success.runtime_exports {
        let spec = lungo_runtime::exports::REQUIRED
            .iter()
            .find(|r| r.symbol == export.symbol)
            .ok_or_else(|| CodegenError::internal(format!("the runtime does not call export {}", export.symbol)))?;
        let decl = success
            .bir
            .declaration(&export.declaration)
            .ok_or_else(|| CodegenError::internal(format!("{} is not part of the program", export.declaration)))?;
        let params: Vec<&Param> = decl.params.iter().filter(|p| p.ty != IrType::Void).collect();
        let matches = params.len() == spec.params.len()
            && params.iter().zip(spec.params).all(|(p, t)| match t {
                Ty::obj | Ty::b_obj => p.ty.is_object(),
                Ty::u8 => p.ty == IrType::Uint8,
                Ty::u16 => p.ty == IrType::Uint16,
                Ty::u32 => p.ty == IrType::Uint32,
                Ty::u64 => p.ty == IrType::Uint64,
                Ty::usize => p.ty == IrType::Usize,
                Ty::f64 => p.ty == IrType::Float,
                Ty::f32 => p.ty == IrType::Float32,
            })
            && decl.result.is_object()
            && matches!(spec.result, Ty::obj);
        if !matches {
            return Err(CodegenError::adapter(
                &decl.name,
                format!("the Lean definition exported as `{}` does not have the signature the runtime calls", spec.symbol),
            ));
        }
        out.push(RuntimeExport {
            symbol: spec.symbol,
            decl,
            params: params.into_iter().zip(spec.params.iter().copied()).collect(),
        });
    }
    Ok(out)
}
