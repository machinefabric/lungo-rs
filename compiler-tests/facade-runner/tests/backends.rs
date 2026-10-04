//! Differential property tests of the generated facade: every exported function is called with
//! the same generated inputs on the lungo runtime (`pure`) and on Lean's own native backend
//! and runtime (`oracle`), and the results must agree exactly.
//!
//! The two facades declare their own (structurally identical) Rust types, so inputs are built
//! from neutral descriptions by per-module constructors, and results are compared through their
//! `Debug` rendering, which covers every field. Floats are rendered with `{:?}`, which is exact
//! and distinguishes `-0.0` and NaN.

use facade_runner::{oracle, pure};
use lungo::{ByteArray, FloatArray, Int, LeanClosure, List, Nat};
use num_bigint::{BigInt, BigUint, Sign};
use proptest::prelude::*;

fn same<T: std::fmt::Debug, U: std::fmt::Debug>(what: &str, p: T, o: U) -> Result<(), TestCaseError> {
    let (p, o) = (format!("{p:?}"), format!("{o:?}"));
    prop_assert_eq!(&p, &o, "{} differs between PureRust and the Lean oracle", what);
    Ok(())
}

/// Naturals across the scalar/big boundary: small, near `2^63`, and multi-limb.
fn nat() -> impl Strategy<Value = Nat> {
    prop_oneof![
        (0u64..1000).prop_map(|n| Nat::from(n as u128)),
        ((1u128 << 62)..(1u128 << 65)).prop_map(Nat::from),
        prop::collection::vec(any::<u32>(), 1..6).prop_map(|limbs| Nat::from(BigUint::new(limbs))),
    ]
}

fn int() -> impl Strategy<Value = Int> {
    (any::<bool>(), nat()).prop_map(|(neg, n)| {
        let sign = if neg { Sign::Minus } else { Sign::Plus };
        Int::from(BigInt::from_biguint(sign, n.to_biguint()))
    })
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Z0-9 ]{0,24}",
        any::<String>(),
        prop::collection::vec(prop_oneof![Just("é"), Just("ß"), Just("日本"), Just("🦀"), Just(" "), Just("a")], 0..10)
            .prop_map(|parts| parts.concat()),
    ]
}

fn float() -> impl Strategy<Value = f64> {
    prop_oneof![
        any::<f64>(),
        -1.0e6..1.0e6f64,
        Just(0.0),
        Just(-0.0),
        Just(f64::INFINITY),
        Just(f64::NEG_INFINITY),
        Just(f64::NAN),
    ]
}

#[derive(Clone, Debug)]
enum ShapeSpec {
    Circle(Nat),
    Rect(Nat, Nat),
    Tagged(String, Box<ShapeSpec>),
    None,
}

fn shape() -> impl Strategy<Value = ShapeSpec> {
    let leaf = prop_oneof![
        nat().prop_map(ShapeSpec::Circle),
        (nat(), nat()).prop_map(|(w, h)| ShapeSpec::Rect(w, h)),
        Just(ShapeSpec::None),
    ];
    leaf.prop_recursive(4, 8, 1, |inner| (text(), inner).prop_map(|(n, s)| ShapeSpec::Tagged(n, Box::new(s))))
}

#[derive(Clone, Debug)]
struct PixelSpec {
    x: u16,
    y: u16,
    color: u8,
    alpha: f64,
    label: String,
}

fn pixel() -> impl Strategy<Value = PixelSpec> {
    (any::<u16>(), any::<u16>(), 0u8..3, float(), text()).prop_map(|(x, y, color, alpha, label)| PixelSpec {
        x,
        y,
        color,
        alpha,
        label,
    })
}

/// Builds the same neutral descriptions in both facades' types.
macro_rules! builders {
    ($m:ident, $b:ident) => {
        mod $b {
            use super::{PixelSpec, ShapeSpec};
            use facade_runner::$m as f;

            pub fn color(c: u8) -> f::Color {
                match c {
                    0 => f::Color::Red,
                    1 => f::Color::Green,
                    _ => f::Color::Blue,
                }
            }

            pub fn pixel(p: &PixelSpec) -> f::Pixel {
                f::Pixel { x: p.x, y: p.y, color: color(p.color), alpha: p.alpha, label: p.label.clone() }
            }

            pub fn shape(s: &ShapeSpec) -> f::Shape {
                match s {
                    ShapeSpec::Circle(r) => f::Shape::Circle { radius: r.clone() },
                    ShapeSpec::Rect(w, h) => f::Shape::Rect { width: w.clone(), height: h.clone() },
                    ShapeSpec::Tagged(n, s) => f::Shape::Tagged { name: n.clone(), inner: Box::new(shape(s)) },
                    ShapeSpec::None => f::Shape::None,
                }
            }

            pub fn ordering(o: std::cmp::Ordering) -> f::_root_::Ordering {
                match o {
                    std::cmp::Ordering::Less => f::_root_::Ordering::Lt,
                    std::cmp::Ordering::Equal => f::_root_::Ordering::Eq,
                    std::cmp::Ordering::Greater => f::_root_::Ordering::Gt,
                }
            }
        }
    };
}

builders!(pure, p);
builders!(oracle, o);

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// TEST0007: nat arithmetic
    #[test]
    fn test0007_nat_arithmetic(a in nat(), b in nat()) {
        same("natOps", pure::nat_ops(a.clone(), b.clone()), oracle::nat_ops(a, b))?;
    }

    /// TEST0008: nat powers
    #[test]
    fn test0008_nat_powers(a in nat(), e in 0u8..40) {
        same("natPow", pure::nat_pow(a.clone(), e), oracle::nat_pow(a, e))?;
    }

    /// TEST0009: int arithmetic
    #[test]
    fn test0009_int_arithmetic(a in int(), b in int()) {
        same("intOps", pure::int_ops(a.clone(), b.clone()), oracle::int_ops(a, b))?;
    }

    /// TEST0010: fixed width
    #[test]
    fn test0010_fixed_width(a: u64, b: u64, c: i32, d: i32, e: u8, f: u16, g: usize) {
        same("fixedOps", pure::fixed_ops(a, b, c, d, e, f, g), oracle::fixed_ops(a, b, c, d, e, f, g))?;
    }

    /// TEST0011: signed widths
    #[test]
    fn test0011_signed_widths(a: i8, b: i16, c: i64, d: isize) {
        same("signedMix", pure::signed_mix(a, b, c, d), oracle::signed_mix(a, b, c, d))?;
    }

    /// TEST0012: floating point
    #[test]
    fn test0012_floating_point(x in float(), y in float(), z: f32) {
        same("floatOps", pure::float_ops(x, y), oracle::float_ops(x, y))?;
        same("float32Ops", pure::float32_ops(z), oracle::float32_ops(z))?;
    }

    /// TEST0013: float arrays
    #[test]
    fn test0013_float_arrays(xs in prop::collection::vec(float(), 0..16), k in float()) {
        let arr = FloatArray::from(xs);
        same("floats", pure::floats(arr.clone()), oracle::floats(arr.clone()))?;
        same("scaled", pure::scaled(arr.clone(), k), oracle::scaled(arr, k))?;
    }

    /// TEST0014: strings
    #[test]
    fn test0014_strings(s in text(), sep in text(), parts in prop::collection::vec(text(), 0..6)) {
        same("textStats", pure::text_stats(s.clone()), oracle::text_stats(s.clone()))?;
        same("joinWith", pure::join_with(sep.clone(), parts.clone()), oracle::join_with(sep, parts))?;
        same("bytes", pure::bytes(s.clone()), oracle::bytes(s))?;
    }

    /// TEST0015: characters
    #[test]
    fn test0015_characters(c: char) {
        same("charInfo", pure::char_info(c), oracle::char_info(c))?;
    }

    /// TEST0016: utf8 decoding
    #[test]
    fn test0016_utf8_decoding(bytes in prop::collection::vec(any::<u8>(), 0..24)) {
        let b = ByteArray::from(bytes);
        same("decode", pure::decode(b.clone()), oracle::decode(b))?;
    }

    /// TEST0017: records
    #[test]
    fn test0017_records(p in pixel(), c in 0u8..3, ps in prop::collection::vec(pixel(), 0..5)) {
        same("recolor", pure::recolor(p::pixel(&p), p::color(c)), oracle::recolor(o::pixel(&p), o::color(c)))?;
        same(
            "brighten",
            pure::brighten(ps.iter().map(p::pixel).collect()),
            oracle::brighten(ps.iter().map(o::pixel).collect()),
        )?;
    }

    /// TEST0018: structures
    #[test]
    fn test0018_structures(a in nat(), b in nat(), n in nat(), s in text()) {
        same(
            "addMeters",
            pure::add_meters(pure::Meters { value: a.clone() }, pure::Meters { value: b.clone() }),
            oracle::add_meters(oracle::Meters { value: a }, oracle::Meters { value: b }),
        )?;
        same("swapPair", pure::swap_pair((n.clone(), s.clone())), oracle::swap_pair((n, s)))?;
    }

    /// TEST0019: inductives
    #[test]
    fn test0019_inductives(s in shape()) {
        same("describeShape", pure::describe_shape(p::shape(&s)), oracle::describe_shape(o::shape(&s)))?;
        same("Shape.area", pure::shape::area(p::shape(&s)), oracle::shape::area(o::shape(&s)))?;
    }

    /// TEST0020: except and option
    #[test]
    fn test0020_except_and_option(x in int(), xs in prop::collection::vec(prop::option::of(nat()), 0..8)) {
        same("classify", pure::classify(x.clone()), oracle::classify(x))?;
        same("firstSome", pure::first_some(List::from(xs.clone())), oracle::first_some(List::from(xs)))?;
    }

    /// TEST0021: trees
    #[test]
    fn test0021_trees(xs in prop::collection::vec(int(), 0..24)) {
        let pt = pure::build_tree(List::from(xs.clone()));
        let ot = oracle::build_tree(List::from(xs.clone()));
        same("buildTree", &pt, &ot)?;
        same("treeToList", pure::tree_to_list(pt.clone()), oracle::tree_to_list(ot.clone()))?;
        same("mirror", pure::mirror(pt.clone()), oracle::mirror(ot.clone()))?;
        // `Tree.toList` of the tree built by insertion is the sorted, deduplicated input.
        let mut sorted = xs.clone();
        sorted.sort();
        sorted.dedup();
        same("treeToList (sorted)", pure::tree_to_list(pt), List::from(sorted))?;
    }

    /// TEST0022: instances from rust
    #[test]
    fn test0022_instances_from_rust(xs in prop::collection::vec(any::<i32>(), 0..16)) {
        // The `Ord` dictionary is a Rust closure, called back from Lean in both backends.
        let pord = pure::_root_::Ord { compare: LeanClosure::from_fn(|a: Nat, b: Nat| p::ordering(a.cmp(&b))) };
        let oord = oracle::_root_::Ord { compare: LeanClosure::from_fn(|a: Nat, b: Nat| o::ordering(a.cmp(&b))) };
        let (mut pt, mut ot) = (pure::Tree::Leaf, oracle::Tree::Leaf);
        for x in &xs {
            let v = Nat::from(x.unsigned_abs() as u128);
            pt = pure::tree::insert(pord.clone(), pt, v.clone());
            ot = oracle::tree::insert(oord.clone(), ot, v);
        }
        same("Tree.insert", &pt, &ot)?;
        same("Tree.toList", pure::tree::to_list(pt), oracle::tree::to_list(ot))?;
    }

    /// TEST0023: higher order
    #[test]
    fn test0023_higher_order(x in nat(), xs in prop::collection::vec(int(), 0..10), ns in prop::collection::vec(nat(), 0..10), k in nat()) {
        let kp = k.clone();
        let pf = LeanClosure::from_fn(move |n: Nat| &n * &kp + Nat::from(1u128));
        let ko = k.clone();
        let of = LeanClosure::from_fn(move |n: Nat| &n * &ko + Nat::from(1u128));
        same("applyTwice", pure::apply_twice(pf, x.clone()), oracle::apply_twice(of, x.clone()))?;

        let pneg = LeanClosure::from_fn(|i: Int| -&i);
        let oneg = LeanClosure::from_fn(|i: Int| -&i);
        same("mapAll", pure::map_all(pneg, List::from(xs.clone())), oracle::map_all(oneg, List::from(xs)))?;

        let padd = LeanClosure::from_fn(|a: Nat, b: Nat| &a + &b);
        let oadd = LeanClosure::from_fn(|a: Nat, b: Nat| &a + &b);
        same("foldWith", pure::fold_with(padd, k.clone(), ns.clone()), oracle::fold_with(oadd, k.clone(), ns))?;
        same("makeAdder", pure::make_adder(k.clone(), x.clone()), oracle::make_adder(k, x))?;
    }

    /// TEST0024: polymorphism
    #[test]
    fn test0024_polymorphism(s in text(), n in nat(), xss in prop::collection::vec(prop::collection::vec(text(), 0..4), 0..5)) {
        same("identity", pure::identity(s.clone()), oracle::identity(s.clone()))?;
        same("pairUp", pure::pair_up(n.clone(), s.clone()), oracle::pair_up(n, s))?;
        let lists: List<List<String>> = xss.into_iter().map(List::from).collect();
        same("lengths", pure::lengths(lists.clone()), oracle::lengths(lists))?;
    }

    /// TEST0025: effects
    #[test]
    fn test0025_effects(n in 0u64..200, s in text()) {
        let n = Nat::from(n as u128);
        let pr = pure::count_to(n.clone()).map_err(|e| e.message().to_owned());
        let or = oracle::count_to(n.clone()).map_err(|e| e.message().to_owned());
        same("countTo", pr, or)?;
        let pr = pure::fail_if_odd(n.clone()).map_err(|e| e.message().to_owned());
        let or = oracle::fail_if_odd(n).map_err(|e| e.message().to_owned());
        same("failIfOdd", pr, or)?;
        same("pureEffect", pure::pure_effect(s.clone()), oracle::pure_effect(s))?;
    }
}

/// TEST0026: Lean closures returned to Rust can be called from Rust in both backends.
#[test]
fn test0026_closures_flow_both_ways() {
    let p = pure::apply_twice(LeanClosure::from_fn(|n: Nat| &n + &n), Nat::from(5u128));
    let o = oracle::apply_twice(LeanClosure::from_fn(|n: Nat| &n + &n), Nat::from(5u128));
    assert_eq!(p, Nat::from(20u128));
    assert_eq!(o, Nat::from(20u128));
}

/// TEST0027: Values shared between threads are safe on both runtimes (Lean marks them multi-threaded).
#[test]
fn test0027_concurrent_calls() {
    let tree: Vec<Int> = (0..200).map(|i| Int::from(((i * 7919) % 257) as i128 - 128)).collect();
    std::thread::scope(|s| {
        for _ in 0..8 {
            let tree = tree.clone();
            s.spawn(move || {
                let p = pure::tree_to_list(pure::build_tree(List::from(tree.clone())));
                let o = oracle::tree_to_list(oracle::build_tree(List::from(tree)));
                assert_eq!(format!("{p:?}"), format!("{o:?}"));
            });
        }
    });
}
