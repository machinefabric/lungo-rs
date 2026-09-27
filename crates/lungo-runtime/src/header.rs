//! `lungo.h`: the header of the runtime's C library (the [`capi`](crate) feature).
//!
//! Its hand-written part holds the inline fast paths over the object layout and the declarations
//! of the C ABI; its generated part declares one function per runtime primitive, from the
//! [`registry`](crate::registry), by [`primitive_declarations`]. `tests/header.rs` keeps the two
//! in sync.

use crate::registry::{self, Ty};

/// `lungo.h`, the header of the runtime's C library that generated C programs include.
pub const HEADER: &str = include_str!("../include/lungo.h");

/// The markers delimiting the generated section of [`HEADER`].
/// The runtime functions a WebAssembly module of a program exports besides the program's
/// boundary: the interface the TypeScript support library (`lungo-ts`) uses to pass bytes,
/// install itself as the host, and manage handles.
pub const WASM_EXPORTS: &[&str] = &[
    "lungo_wasm_alloc",
    "lungo_wasm_free",
    "lungo_wasm_buffer_new",
    "lungo_wasm_buffer_delete",
    "lungo_wasm_use_host",
    "lungo_buffer_alloc",
    "lungo_buffer_free",
    "lungo_handle_clone",
    "lungo_handle_release",
    "lungo_closure_call",
];

pub const BEGIN_PRIMITIVES: &str = "/* BEGIN GENERATED: runtime primitives (do not edit; see lungo-runtime/tests/header.rs) */";
pub const END_PRIMITIVES: &str = "/* END GENERATED: runtime primitives */";

/// The C type of a parameter or result representation.
pub fn c_type(ty: Ty) -> &'static str {
    match ty {
        Ty::obj | Ty::b_obj => "lungo_obj",
        Ty::u8 => "uint8_t",
        Ty::u16 => "uint16_t",
        Ty::u32 => "uint32_t",
        Ty::u64 => "uint64_t",
        Ty::usize => "size_t",
        Ty::f64 => "double",
        Ty::f32 => "float",
    }
}

/// The C symbol of the runtime primitive implementing the Lean extern `symbol`.
pub fn primitive_symbol(symbol: &str) -> String {
    format!("lungo_{symbol}")
}

/// One declaration per runtime primitive, sorted by symbol: the generated section of
/// `lungo.h`. Borrowed object parameters (`@&` in Lean) are marked.
pub fn primitive_declarations() -> String {
    let mut prims: Vec<_> = registry::intrinsics().collect();
    prims.sort_by_key(|p| p.symbol);
    let mut out = String::new();
    for p in prims {
        let params: Vec<String> = p
            .params
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let borrowed = if *t == Ty::b_obj { " /* borrowed */" } else { "" };
                format!("{} a{i}{borrowed}", c_type(*t))
            })
            .collect();
        let params = if params.is_empty() { "void".to_owned() } else { params.join(", ") };
        out.push_str(&format!("{} {}({params});\n", c_type(p.result), primitive_symbol(p.symbol)));
    }
    out
}

/// [`HEADER`] with its generated section replaced by `section`.
pub fn with_primitives(header: &str, section: &str) -> Result<String, String> {
    let begin = header.find(BEGIN_PRIMITIVES).ok_or("lungo.h has no BEGIN GENERATED marker")?;
    let end = header.find(END_PRIMITIVES).ok_or("lungo.h has no END GENERATED marker")?;
    if end < begin {
        return Err("lungo.h's generated-section markers are out of order".into());
    }
    let start = begin + BEGIN_PRIMITIVES.len();
    Ok(format!("{}\n{section}{}", &header[..start], &header[end..]))
}
