//! The BIR verifier accepts well-formed uses of every instruction and rejects each violation
//! of the invariants the Rust backend relies on.

use patina_bir::*;

fn ctor(name: &str, tag: u32, size: u32, usize: u32, ssize: u32) -> CtorInfo {
    CtorInfo { name: name.into(), tag, size, usize, ssize }
}

fn param(var: VarId, ty: IrType, borrow: bool) -> Param {
    Param { var, ty, borrow }
}

fn let_(var: VarId, ty: IrType, expr: Expr) -> Stmt {
    Stmt::Let { var, ty, expr }
}

fn ret(v: VarId) -> Terminator {
    Terminator::Ret { arg: Arg::Var(v) }
}

fn block(stmts: Vec<Stmt>, terminator: Terminator) -> Block {
    Block { stmts, terminator }
}

fn function(name: &str, params: Vec<Param>, result: IrType, body: Block) -> Declaration {
    Declaration {
        name: name.into(),
        module: "M".into(),
        origin: None,
        params,
        result,
        body: Body::Function { block: body },
    }
}

fn program(mut declarations: Vec<Declaration>) -> Program {
    declarations.sort_by(|a, b| a.name.cmp(&b.name));
    Program {
        bir_version: BIR_VERSION,
        modules: vec![Module { name: "M".into(), imports: vec![], initializers: vec![] }],
        declarations,
    }
}

/// `add : obj → obj → obj`, a callee for calls and closures.
fn add() -> Declaration {
    function(
        "add",
        vec![param(1, IrType::Object, false), param(2, IrType::Object, false)],
        IrType::Object,
        block(vec![], ret(1)),
    )
}

/// A declaration using every statement, expression and terminator form, well-formed.
fn everything() -> Declaration {
    use IrType::*;
    let point = ctor("Point", 0, 1, 1, 9);
    let body = block(
        vec![
            let_(10, Uint64, Expr::Lit(Literal::Num("18446744073709551615".into()))),
            let_(11, Object, Expr::Lit(Literal::Num("340282366920938463463374607431768211456".into()))),
            let_(12, Object, Expr::Lit(Literal::Str("hi".into()))),
            let_(13, Usize, Expr::Lit(Literal::Num("7".into()))),
            let_(14, Float, Expr::Unbox { var: 1 }),
            let_(15, Object, Expr::Ctor { info: point.clone(), args: vec![Arg::Var(12)] }),
            Stmt::Uset { var: 15, index: 1, value: 13 },
            Stmt::Sset { var: 15, index: 2, offset: 0, value: 14, ty: Float },
            Stmt::Sset { var: 15, index: 2, offset: 8, value: 3, ty: Uint8 },
            let_(16, Usize, Expr::Uproj { index: 1, var: 15 }),
            let_(17, Float, Expr::Sproj { fields: 2, offset: 0, var: 15 }),
            let_(18, Tobject, Expr::Proj { index: 0, var: 15 }),
            Stmt::Inc { var: 18, count: 2, checked: true, persistent: false },
            let_(19, Uint8, Expr::IsShared { var: 15 }),
            let_(20, Object, Expr::Reset { fields: 1, var: 15 }),
            let_(
                21,
                Object,
                Expr::Reuse { var: 20, info: ctor("Point", 0, 1, 1, 9), update_header: false, args: vec![Arg::Erased] },
            ),
            Stmt::Set { var: 21, index: 0, arg: Arg::Var(11) },
            Stmt::SetTag { var: 21, tag: 1 },
            let_(22, Object, Expr::Pap { function: "add".into(), args: vec![Arg::Var(11)] }),
            let_(23, Object, Expr::Ap { var: 22, args: vec![Arg::Var(12)] }),
            let_(24, Object, Expr::Fap { function: "add".into(), args: vec![Arg::Var(23), Arg::Erased] }),
            let_(25, Object, Expr::Box { ty: Usize, var: 16 }),
            Stmt::Dec { var: 25, count: 1, checked: false, persistent: false },
            Stmt::Del { var: 21 },
            Stmt::Join { id: 30, params: vec![param(31, Object, false)], body: block(vec![], ret(31)) },
        ],
        Terminator::Case {
            type_name: "Bool".into(),
            var: 3,
            var_ty: Uint8,
            alts: vec![
                Alt::Ctor {
                    info: ctor("Bool.false", 0, 0, 0, 0),
                    body: block(vec![], Terminator::Jmp { id: 30, args: vec![Arg::Var(24)] }),
                },
                Alt::Ctor { info: ctor("Bool.true", 1, 0, 0, 0), body: block(vec![], Terminator::Unreachable) },
                Alt::Default { body: block(vec![], ret(24)) },
            ],
        },
    );
    function("everything", vec![param(1, Object, true), param(3, Uint8, false)], Object, body)
}

fn errors_of(p: &Program) -> Vec<String> {
    match validate(p) {
        Ok(()) => vec![],
        Err(es) => es.iter().map(|e| e.to_string()).collect(),
    }
}

#[track_caller]
fn assert_rejects(p: &Program, needle: &str) {
    let errors = errors_of(p);
    assert!(errors.iter().any(|e| e.contains(needle)), "expected an error containing {needle:?}, got {errors:#?}");
}

fn simple(stmts: Vec<Stmt>, terminator: Terminator) -> Program {
    program(vec![
        add(),
        function(
            "f",
            vec![param(1, IrType::Object, false), param(2, IrType::Uint8, false)],
            IrType::Object,
            block(stmts, terminator),
        ),
    ])
}

#[test]
fn every_instruction_in_well_formed_use_is_accepted() {
    let p = program(vec![add(), everything()]);
    assert_eq!(errors_of(&p), Vec::<String>::new());
}

#[test]
fn scoping() {
    assert_rejects(&simple(vec![], ret(9)), "x_9 is used outside its scope");
    assert_rejects(
        &simple(vec![let_(1, IrType::Object, Expr::Lit(Literal::Str("s".into())))], ret(1)),
        "x_1 is bound more than once",
    );
    // A variable bound in one case alternative is not visible after the case, nor in another.
    let alt_bound = Terminator::Case {
        type_name: "Bool".into(),
        var: 2,
        var_ty: IrType::Uint8,
        alts: vec![
            Alt::Ctor {
                info: ctor("Bool.false", 0, 0, 0, 0),
                body: block(vec![let_(5, IrType::Object, Expr::Lit(Literal::Str("a".into())))], ret(5)),
            },
            Alt::Default { body: block(vec![], ret(5)) },
        ],
    };
    assert_rejects(&simple(vec![], alt_bound), "x_5 is used outside its scope");
    // Join point parameters are local to the join point.
    let join = Stmt::Join { id: 7, params: vec![param(8, IrType::Object, false)], body: block(vec![], ret(8)) };
    assert_rejects(&simple(vec![join], ret(8)), "x_8 is used outside its scope");
}

#[test]
fn join_points() {
    let join = || Stmt::Join { id: 7, params: vec![param(8, IrType::Object, false)], body: block(vec![], ret(8)) };
    assert_rejects(
        &simple(vec![], Terminator::Jmp { id: 7, args: vec![Arg::Var(1)] }),
        "jump to block_7, which is not in scope",
    );
    assert_rejects(
        &simple(vec![join()], Terminator::Jmp { id: 7, args: vec![] }),
        "passes 0 arguments for 1 parameters",
    );
    assert_rejects(
        &simple(vec![join()], Terminator::Jmp { id: 7, args: vec![Arg::Var(2)] }),
        "jump argument: x_2 of type u8 is passed as obj",
    );
    // Join points and variables share one identifier namespace.
    let clash = Stmt::Join { id: 1, params: vec![], body: block(vec![], ret(1)) };
    assert_rejects(&simple(vec![clash], ret(1)), "block_1 is bound more than once");
    // A join point is not in scope inside its own body.
    let recursive = Stmt::Join { id: 7, params: vec![], body: block(vec![], Terminator::Jmp { id: 7, args: vec![] }) };
    assert_rejects(&simple(vec![recursive], ret(1)), "jump to block_7, which is not in scope");
}

#[test]
fn calls_and_closures() {
    let call =
        |args: Vec<Arg>| simple(vec![let_(3, IrType::Object, Expr::Fap { function: "add".into(), args })], ret(3));
    assert_rejects(&call(vec![Arg::Var(1)]), "add takes 2 arguments but is applied to 1");
    assert_rejects(&call(vec![Arg::Var(1), Arg::Var(2)]), "call argument: x_2 of type u8 is passed as obj");
    assert_rejects(
        &simple(vec![let_(3, IrType::Object, Expr::Fap { function: "missing".into(), args: vec![] })], ret(3)),
        "call to unknown declaration \"missing\"",
    );
    assert_rejects(
        &simple(
            vec![let_(3, IrType::Uint8, Expr::Fap { function: "add".into(), args: vec![Arg::Var(1), Arg::Var(1)] })],
            ret(1),
        ),
        "add returns obj but its result is bound as u8",
    );
    assert_rejects(
        &simple(
            vec![let_(3, IrType::Object, Expr::Pap { function: "add".into(), args: vec![Arg::Var(1), Arg::Var(1)] })],
            ret(3),
        ),
        "closure over add fixes 2 of 2 arguments",
    );
    let scalar_fn = function(
        "sc",
        vec![param(1, IrType::Uint8, false), param(2, IrType::Object, false)],
        IrType::Object,
        block(vec![], ret(2)),
    );
    let p = program(vec![
        scalar_fn,
        function(
            "f",
            vec![param(1, IrType::Object, false)],
            IrType::Object,
            block(vec![let_(3, IrType::Object, Expr::Pap { function: "sc".into(), args: vec![] })], ret(3)),
        ),
    ]);
    assert_rejects(&p, "closure over sc, which takes unboxed parameters");
    assert_rejects(
        &simple(vec![let_(3, IrType::Object, Expr::Ap { var: 1, args: vec![] })], ret(3)),
        "closure application without arguments",
    );
    assert_rejects(
        &simple(vec![let_(3, IrType::Object, Expr::Ap { var: 2, args: vec![Arg::Var(1)] })], ret(3)),
        "application requires an object, but x_2 has type u8",
    );
}

#[test]
fn constructors_and_fields() {
    let bad_arity = simple(
        vec![let_(3, IrType::Object, Expr::Ctor { info: ctor("P", 0, 2, 0, 0), args: vec![Arg::Var(1)] })],
        ret(3),
    );
    assert_rejects(&bad_arity, "ctor P takes 2 object fields but is given 1");
    let bad_tag =
        simple(vec![let_(3, IrType::Object, Expr::Ctor { info: ctor("P", 244, 0, 0, 0), args: vec![] })], ret(3));
    assert_rejects(&bad_tag, "constructor tag 244 exceeds the maximum 243");
    let scalar_field = simple(
        vec![let_(3, IrType::Object, Expr::Ctor { info: ctor("P", 0, 1, 0, 0), args: vec![Arg::Var(2)] })],
        ret(3),
    );
    assert_rejects(&scalar_field, "ctor field: x_2 of type u8 is passed as obj");
    assert_rejects(
        &simple(vec![let_(3, IrType::Uint8, Expr::Proj { index: 0, var: 1 })], ret(1)),
        "proj produces an object but is bound as u8",
    );
    assert_rejects(
        &simple(vec![let_(3, IrType::Uint64, Expr::Uproj { index: 0, var: 1 })], ret(1)),
        "uproj bound as u64",
    );
    assert_rejects(
        &simple(vec![let_(3, IrType::Usize, Expr::Sproj { fields: 0, offset: 0, var: 1 })], ret(1)),
        "sproj bound as usize",
    );
    assert_rejects(
        &simple(vec![let_(3, IrType::Object, Expr::Proj { index: 0, var: 2 })], ret(3)),
        "proj requires an object, but x_2 has type u8",
    );
    assert_rejects(
        &simple(vec![Stmt::Sset { var: 1, index: 0, offset: 0, value: 2, ty: IrType::Usize }], ret(1)),
        "sset stores a non-scalar type usize",
    );
    assert_rejects(
        &simple(vec![Stmt::Sset { var: 1, index: 0, offset: 0, value: 2, ty: IrType::Uint16 }], ret(1)),
        "sset: x_2 of type u8 is passed as u16",
    );
    assert_rejects(
        &simple(vec![Stmt::Uset { var: 1, index: 0, value: 2 }], ret(1)),
        "uset: x_2 of type u8 is passed as usize",
    );
    assert_rejects(
        &simple(vec![Stmt::SetTag { var: 1, tag: 300 }], ret(1)),
        "constructor tag 300 exceeds the maximum 243",
    );
    assert_rejects(
        &simple(vec![Stmt::Set { var: 2, index: 0, arg: Arg::Var(1) }], ret(1)),
        "set requires an object, but x_2 has type u8",
    );
    let reuse = Expr::Reuse { var: 1, info: ctor("P", 0, 1, 0, 0), update_header: true, args: vec![] };
    assert_rejects(&simple(vec![let_(3, IrType::Object, reuse)], ret(3)), "reuse for P gives 0 fields for 1");
}

#[test]
fn reference_counting() {
    assert_rejects(
        &simple(vec![Stmt::Inc { var: 1, count: 0, checked: true, persistent: false }], ret(1)),
        "inc by zero",
    );
    assert_rejects(&simple(vec![Stmt::Dec { var: 1, count: 2, checked: true, persistent: false }], ret(1)), "dec by 2");
    assert_rejects(
        &simple(vec![Stmt::Inc { var: 2, count: 1, checked: true, persistent: false }], ret(1)),
        "inc requires an object, but x_2 has type u8",
    );
    assert_rejects(&simple(vec![Stmt::Del { var: 2 }], ret(1)), "del requires an object");
    assert_rejects(&simple(vec![let_(3, IrType::Object, Expr::IsShared { var: 1 })], ret(1)), "isShared bound as obj");
}

#[test]
fn boxing_and_literals() {
    assert_rejects(
        &simple(vec![let_(3, IrType::Object, Expr::Box { ty: IrType::Object, var: 1 })], ret(3)),
        "box of non-scalar type obj",
    );
    assert_rejects(
        &simple(vec![let_(3, IrType::Object, Expr::Box { ty: IrType::Uint32, var: 2 })], ret(3)),
        "box: x_2 of type u8 is passed as u32",
    );
    assert_rejects(&simple(vec![let_(3, IrType::Object, Expr::Unbox { var: 1 })], ret(3)), "unbox bound as obj");
    let lit = |ty, n: &str| simple(vec![let_(3, ty, Expr::Lit(Literal::Num(n.into())))], ret(1));
    assert_rejects(&lit(IrType::Uint8, "256"), "literal 256 does not fit u8");
    assert_rejects(&lit(IrType::Uint64, "18446744073709551616"), "does not fit u64");
    assert_rejects(&lit(IrType::Object, "007"), "not a canonical decimal");
    assert_rejects(&lit(IrType::Object, "-1"), "not a canonical decimal");
    assert_rejects(&lit(IrType::Float, "1"), "numeric literal bound as float");
    assert_rejects(
        &simple(vec![let_(3, IrType::Uint8, Expr::Lit(Literal::Str("s".into())))], ret(1)),
        "string literal produces an object",
    );
}

#[test]
fn cases_and_returns() {
    let case = |var, var_ty, alts| simple(vec![], Terminator::Case { type_name: "T".into(), var, var_ty, alts });
    let leaf = |tag| Alt::Ctor { info: ctor("T.c", tag, 0, 0, 0), body: block(vec![], ret(1)) };
    let default = || Alt::Default { body: block(vec![], ret(1)) };
    assert_rejects(&case(1, IrType::Object, vec![]), "case without alternatives");
    assert_rejects(&case(1, IrType::Object, vec![leaf(0), leaf(0)]), "two alternatives for tag 0");
    assert_rejects(
        &case(1, IrType::Object, vec![default(), leaf(0)]),
        "default alternative is not the last alternative",
    );
    assert_rejects(&case(2, IrType::Uint16, vec![leaf(0)]), "case on x_2 of type u8 declared as u16");
    let float_case = program(vec![function(
        "g",
        vec![param(1, IrType::Float, false), param(2, IrType::Object, false)],
        IrType::Object,
        block(
            vec![],
            Terminator::Case {
                type_name: "T".into(),
                var: 1,
                var_ty: IrType::Float,
                alts: vec![Alt::Default { body: block(vec![], ret(2)) }],
            },
        ),
    )]);
    assert_rejects(&float_case, "case on a value of type float");
    assert_rejects(&simple(vec![], ret(2)), "return: x_2 of type u8 is passed as obj");
    // An erased value is a valid object result; it cannot stand for a scalar.
    assert_eq!(errors_of(&simple(vec![], Terminator::Ret { arg: Arg::Erased })), Vec::<String>::new());
    let scalar =
        program(vec![function("h", vec![], IrType::Uint8, block(vec![], Terminator::Ret { arg: Arg::Erased }))]);
    assert_rejects(&scalar, "an erased argument cannot be passed as u8");
}

#[test]
fn program_structure() {
    let mut p = program(vec![add(), everything()]);
    p.bir_version = BIR_VERSION + 1;
    assert_rejects(&p, "is incompatible with");

    let mut p = program(vec![add(), everything()]);
    p.declarations.swap(0, 1);
    assert_rejects(&p, "declarations are not strictly sorted");

    let mut p = program(vec![add()]);
    p.declarations[0].module = "Elsewhere".into();
    assert_rejects(&p, "owning module \"Elsewhere\" is not part of the program");

    let mut p = program(vec![add()]);
    p.modules.push(p.modules[0].clone());
    assert_rejects(&p, "module name \"M\" is empty or duplicated");

    let mut p = program(vec![add()]);
    p.modules[0].initializers.push(Initializer::Value { decl: "add".into(), init_fn: "initAdd".into() });
    assert_rejects(&p, "initializer references missing declaration \"initAdd\"");
}

#[test]
fn externs() {
    let entry = ExternEntry::Standard { backend: "c".into(), symbol: "lean_add".into() };
    let ext = |selected: ExternEntry, exported_by: Option<&str>| Declaration {
        name: "ext".into(),
        module: "M".into(),
        origin: None,
        params: vec![param(1, IrType::Object, true)],
        result: IrType::Object,
        body: Body::Extern { entries: vec![entry.clone()], selected, exported_by: exported_by.map(String::from) },
    };
    assert_eq!(errors_of(&program(vec![add(), ext(entry.clone(), Some("add"))])), Vec::<String>::new());
    assert_rejects(
        &program(vec![ext(ExternEntry::Opaque, None)]),
        "selected extern entry is not one of the declaration's entries",
    );
    assert_rejects(
        &program(vec![ext(entry.clone(), Some("impl"))]),
        "the @[export] implementation \"impl\" is not a compiled function",
    );
}
