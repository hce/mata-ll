//! Builtin Prelude registration: the hand-registered classes, class methods,
//! instances and primitive value schemes every module starts from. Moved out
//! of the monolithic typechecker mod.rs; `use super::*` keeps every name
//! resolution identical (continuation module, like solve.rs).
//!
//! The four-step ritual a builtin class used to require — build the method
//! types, insert the `ClassInfo`, insert an env `Scheme` per method, insert
//! `method_constraints` — is folded into [`Checker::register_builtin_class`];
//! per-type instances go through [`Checker::register_builtin_instance`]. Both
//! preserve the exact registrations the inline blocks used to make.

use super::*;

/// One method of a builtin class registration, consumed by
/// [`Checker::register_builtin_class`]:
/// `(method name, method type, env-scheme vars, per-method constraints)`.
///
/// - env-scheme vars: `Some(vars)` inserts the environment scheme
///   `forall vars. ty` under the method name. The vars are listed explicitly,
///   NOT derived from the type: their order fixes the fresh-variable
///   numbering at instantiation and therefore the variable names diagnostics
///   print. `None` skips the env insert — either another registration owns
///   the name (Monad's `return` keeps the Applicative `pure` scheme) or it
///   was registered elsewhere (the arithmetic operators).
/// - per-method constraints: the entry `method_constraints` gets for the
///   method name, as (class, constrained variable) pairs. Most methods
///   constrain the class variable alone; `traverse` also constrains its
///   applicative (`Applicative f`), which is why this is a list of pairs
///   rather than a single class name. An empty list registers nothing.
type BuiltinMethod = (
    &'static str,
    Ty,
    Option<Vec<TyVar>>,
    Vec<(&'static str, &'static str)>,
);

impl Checker {
    /// Insert one environment binding `name : forall vars. ty` (no quantified
    /// multiplicity variables — no builtin scheme has any).
    fn env_scheme(&mut self, name: &str, vars: Vec<TyVar>, ty: Ty) {
        self.env.insert(name.to_string(), Scheme { vars, mult_vars: vec![], ty });
    }

    /// Register one builtin class in a single call: the `ClassInfo` (methods
    /// in declaration order, no default methods), an env scheme per method
    /// that wants one, and the per-method wanted constraints. See
    /// [`BuiltinMethod`] for the shape of each entry.
    fn register_builtin_class(
        &mut self,
        name: &str,
        type_var: &str,
        superclasses: &[&str],
        methods: Vec<BuiltinMethod>,
    ) {
        let mut method_list = Vec::with_capacity(methods.len());
        for (mname, ty, env_vars, constraints) in methods {
            method_list.push((mname.to_string(), ty.clone()));
            if let Some(vars) = env_vars {
                self.env_scheme(mname, vars, ty);
            }
            if !constraints.is_empty() {
                self.method_constraints.insert(
                    mname.to_string(),
                    constraints
                        .iter()
                        .map(|(class, var)| TyConstraint {
                            class_name: class.to_string(),
                            type_var: var.to_string(),
                        })
                        .collect(),
                );
            }
        }
        self.classes.insert(name.to_string(), ClassInfo {
            name: name.to_string(),
            type_var: type_var.to_string(),
            superclasses: superclasses.iter().map(|s| s.to_string()).collect(),
            methods: method_list,
            default_methods: HashMap::new(),
        });
    }

    /// Register one builtin instance: `class` at `target`, each method mapped
    /// to its runtime implementation name. `context: None` — the structural
    /// fallback rule applies (every type argument needs the class itself),
    /// which is what the Show/Eq-style builtin instances want.
    fn register_builtin_instance<M: AsRef<str>, F: AsRef<str>>(
        &mut self,
        class: &str,
        target: Ty,
        methods: &[(M, F)],
    ) {
        self.builtin_instance(class, target, methods, None);
    }

    /// Like `register_builtin_instance`, but with the EMPTY declared context
    /// (`Some(vec![])`): the instance demands nothing of the constructor's
    /// own type arguments, so the structural fallback must not apply. Used by
    /// the higher-kinded Maybe/Either instances — see the comment at the
    /// Functor registrations below.
    fn register_builtin_instance_empty_ctx<M: AsRef<str>, F: AsRef<str>>(
        &mut self,
        class: &str,
        target: Ty,
        methods: &[(M, F)],
    ) {
        self.builtin_instance(class, target, methods, Some(vec![]));
    }

    fn builtin_instance<M: AsRef<str>, F: AsRef<str>>(
        &mut self,
        class: &str,
        target: Ty,
        methods: &[(M, F)],
        context: Option<Vec<TyConstraint>>,
    ) {
        self.register_instance(InstanceInfo {
            class_name: class.to_string(),
            target_type: target,
            method_fns: methods
                .iter()
                .map(|(m, f)| (m.as_ref().to_string(), f.as_ref().to_string()))
                .collect(),
            context,
        });
    }

    pub(super) fn init_prelude(&mut self) {
        let a = TyVar { name: "a".into(), id: u32::MAX };
        let b = TyVar { name: "b".into(), id: u32::MAX };
        let c = TyVar { name: "c".into(), id: u32::MAX };
        let f = TyVar { name: "f".into(), id: u32::MAX };
        let m = TyVar { name: "m".into(), id: u32::MAX };
        let ta = Ty::Var(a.clone());
        let tb = Ty::Var(b.clone());
        let tc = Ty::Var(c.clone());
        let tf = Ty::Var(f.clone());
        let tm = Ty::Var(m.clone());

        // Only register types for builtins that are NOT provided by Prelude.mll
        // Prelude.mll provides: putStrLn, sqrt, id, const, flip,
        //   head, tail, map, filter, take, zipWith, length, reverse
        // (foldr/foldl are Foldable class methods, registered below)
        // `print` is deliberately ABSENT: Prelude.mll defines the real
        // `print :: Show a => a -> IO ()` (via show), and its registration
        // must not compete with a stale monomorphic String form here — the
        // old builtin entry was only masked by registration order.
        let entries: Vec<(&str, Vec<TyVar>, Ty)> = vec![
            ("++", vec![a.clone()], Ty::fun(&[Ty::list(ta.clone()), Ty::list(ta.clone())], Ty::list(ta.clone()))),
            ("!!", vec![a.clone()], Ty::fun(&[Ty::list(ta.clone()), Ty::Con("Int".into())], ta.clone())),
            ("$", vec![a.clone(), b.clone()], Ty::fun(&[Ty::arrow(ta.clone(), tb.clone()), ta.clone()], tb.clone())),
            (".", vec![a.clone(), b.clone(), c.clone()], Ty::fun(&[Ty::arrow(tb.clone(), tc.clone()), Ty::arrow(ta.clone(), tb.clone()), ta.clone()], tc.clone())),
            ("not", vec![], Ty::arrow(Ty::Con("Bool".into()), Ty::Con("Bool".into()))),
            ("error", vec![a.clone()], Ty::arrow(Ty::Con("String".into()), ta.clone())),
            ("undefined", vec![a.clone()], ta.clone()),
            ("otherwise", vec![], Ty::Con("Bool".into())),
            ("seq", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), tb.clone()], tb.clone())),
            // pure/return, >>=, >> are now typeclass methods (Applicative/Monad)
            // but keep env entries so type inference sees them as polymorphic
            ("getArgs", vec![], Ty::io(Ty::list(Ty::Con("String".into())))),
            ("exit", vec![], Ty::arrow(Ty::Con("ExitValue".into()), Ty::io(Ty::Unit))),
            // Exception handling: catch Lua-level IO errors
            ("try", vec![a.clone()], Ty::arrow(
                Ty::io(ta.clone()),
                Ty::io(Ty::app(Ty::app(Ty::Con("Either".into()), Ty::Con("String".into())), ta.clone())),
            )),
            ("catch", vec![a.clone()], Ty::fun(&[
                Ty::io(ta.clone()),
                Ty::arrow(Ty::Con("String".into()), Ty::io(ta.clone())),
            ], Ty::io(ta.clone()))),
        ];
        for (name, vars, ty) in entries {
            self.env_scheme(name, vars, ty);
        }
        // HashMap operations (backed by Lua tables)
        let hm = |k: Ty, v: Ty| Ty::app(Ty::app(Ty::Con("HashMap".into()), k), v);
        let hm_kv = hm(ta.clone(), tb.clone());
        let hm_entries: Vec<(&str, Vec<TyVar>, Ty)> = vec![
            ("hmEmpty", vec![a.clone(), b.clone()], hm_kv.clone()),
            ("hmInsert", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), tb.clone(), hm_kv.clone()], hm_kv.clone())),
            ("hmLookup", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), hm_kv.clone()], Ty::app(Ty::Con("Maybe".into()), tb.clone()))),
            ("hmDelete", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), hm_kv.clone()], hm_kv.clone())),
            ("hmSize", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::Con("Int".into()))),
            ("hmKeys", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::list(ta.clone()))),
            ("hmValues", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::list(tb.clone()))),
            ("hmMember", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), hm_kv.clone()], Ty::Con("Bool".into()))),
            ("hmFromList", vec![a.clone(), b.clone()], Ty::arrow(Ty::list(Ty::Tuple(vec![ta.clone(), tb.clone()])), hm_kv.clone())),
            ("hmToList", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::list(Ty::Tuple(vec![ta.clone(), tb.clone()])))),
        ];
        for (name, vars, ty) in hm_entries {
            self.env_scheme(name, vars, ty);
        }

        // ByteString operations (backed by Lua strings as byte arrays)
        let bs = Ty::Con("ByteString".into());
        let int = Ty::Con("Int".into());
        let bool_ = Ty::Con("Bool".into());
        let bs_entries: Vec<(&str, Vec<TyVar>, Ty)> = vec![
            ("bsEmpty",     vec![], bs.clone()),
            ("bsLength",    vec![], Ty::arrow(bs.clone(), int.clone())),
            ("bsIndex",     vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsSub",       vec![], Ty::fun(&[bs.clone(), int.clone(), int.clone()], bs.clone())),
            ("bsSingleton", vec![], Ty::arrow(int.clone(), bs.clone())),
            ("bsConcat",    vec![], Ty::fun(&[bs.clone(), bs.clone()], bs.clone())),
            ("bsConcatList", vec![], Ty::arrow(Ty::list(bs.clone()), bs.clone())),
            ("bsNull",      vec![], Ty::arrow(bs.clone(), bool_.clone())),
            ("bsHead",      vec![], Ty::arrow(bs.clone(), int.clone())),
            ("bsTail",      vec![], Ty::arrow(bs.clone(), bs.clone())),
            ("bsCons",      vec![], Ty::fun(&[int.clone(), bs.clone()], bs.clone())),
            ("bsSnoc",      vec![], Ty::fun(&[bs.clone(), int.clone()], bs.clone())),
            ("bsReplicate", vec![], Ty::fun(&[int.clone(), int.clone()], bs.clone())),
            ("bsPack",      vec![], Ty::arrow(Ty::list(int.clone()), bs.clone())),
            ("bsUnpack",    vec![], Ty::arrow(bs.clone(), Ty::list(int.clone()))),
            ("bsMap",       vec![], Ty::fun(&[Ty::arrow(int.clone(), int.clone()), bs.clone()], bs.clone())),
            ("bsFoldl",     vec![a.clone()], Ty::fun(&[Ty::fun(&[ta.clone(), int.clone()], ta.clone()), ta.clone(), bs.clone()], ta.clone())),
            ("bsXor",       vec![], Ty::fun(&[bs.clone(), bs.clone()], bs.clone())),
            ("bsZipWith",   vec![], Ty::fun(&[Ty::fun(&[int.clone(), int.clone()], int.clone()), bs.clone(), bs.clone()], bs.clone())),
            ("bsToString",  vec![], Ty::arrow(bs.clone(), Ty::Con("String".into()))),
            ("bsFromString", vec![], Ty::arrow(Ty::Con("String".into()), bs.clone())),
            ("bsGetU16LE",  vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsGetU32LE",  vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsGetI8",     vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsGetI16LE",  vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsPutI16LE",  vec![], Ty::arrow(int.clone(), bs.clone())),
        ];
        for (name, vars, ty) in bs_entries {
            self.env_scheme(name, vars, ty);
        }

        // `max`/`min` are Ord class methods (registered with the Ord class
        // below), like `compare` — NOT unconstrained builtins: an
        // unconstrained `forall a. a -> a -> a` lowered to math.max/min
        // typechecks at every type and then crashes at runtime on anything
        // that is not a Lua number (String, Bool, boxed Integer).
        // Arithmetic operators are the methods of the numeric classes
        // registered further below (Num for + - *, Fractional for /). Their
        // env schemes are `forall a. a -> a -> a`; the Num/Fractional class
        // constraint on `a` is attached via `method_constraints` in the
        // numeric-class registration block, exactly like `==`/`show`.
        for op in &["+", "-", "*", "/"] {
            self.env_scheme(op, vec![a.clone()], Ty::fun(&[ta.clone(), ta.clone()], ta.clone()));
        }
        // Comparison operators will be registered as Ord methods below
        for op in &["&&", "||"] {
            self.env_scheme(op, vec![], Ty::fun(&[Ty::Con("Bool".into()), Ty::Con("Bool".into())], Ty::Con("Bool".into())));
        }
        // div/mod/quot/rem are the Integral class methods (`forall a. a->a->a`,
        // constrained to Integral below). Previously monomorphic Int->Int.
        for name in &["mod", "div", "quot", "rem"] {
            self.env_scheme(name, vec![a.clone()], Ty::fun(&[ta.clone(), ta.clone()], ta.clone()));
        }
        // List functions that need lazy cons (implemented in Lua runtime)
        self.env_scheme("head", vec![a.clone()], Ty::arrow(Ty::list(ta.clone()), ta.clone()));
        self.env_scheme("tail", vec![a.clone()], Ty::arrow(Ty::list(ta.clone()), Ty::list(ta.clone())));
        self.env_scheme("map", vec![a.clone(), b.clone()], Ty::fun(&[Ty::arrow(ta.clone(), tb.clone()), Ty::list(ta.clone())], Ty::list(tb.clone())));
        self.env_scheme("filter", vec![a.clone(), b.clone()], Ty::fun(&[Ty::arrow(ta.clone(), Ty::Con("Bool".into())), Ty::list(ta.clone())], Ty::list(ta.clone())));
        self.env_scheme("take", vec![a.clone()], Ty::fun(&[Ty::Con("Int".into()), Ty::list(ta.clone())], Ty::list(ta.clone())));
        self.env_scheme("drop", vec![a.clone()], Ty::fun(&[Ty::Con("Int".into()), Ty::list(ta.clone())], Ty::list(ta.clone())));
        self.env_scheme("zipWith", vec![a.clone(), b.clone(), c.clone()], Ty::fun(&[Ty::fun(&[ta.clone(), tb.clone()], tc.clone()), Ty::list(ta.clone()), Ty::list(tb.clone())], Ty::list(tc.clone())));

        // Maybe
        self.constructors.insert("Just".into(), ConInfo { type_name: "Maybe".into(), variant_index: 1, total_variants: 2, field_types: vec![ta.clone()], type_vars: vec![a.clone()], result_type: Ty::app(Ty::Con("Maybe".into()), ta.clone()), existential_vars: vec![], existential_constraints: vec![] });
        self.constructors.insert("Nothing".into(), ConInfo { type_name: "Maybe".into(), variant_index: 2, total_variants: 2, field_types: vec![], type_vars: vec![a.clone()], result_type: Ty::app(Ty::Con("Maybe".into()), ta.clone()), existential_vars: vec![], existential_constraints: vec![] });
        self.env_scheme("Just", vec![a.clone()], Ty::arrow(ta.clone(), Ty::app(Ty::Con("Maybe".into()), ta.clone())));
        self.env_scheme("Nothing", vec![a.clone()], Ty::app(Ty::Con("Maybe".into()), ta.clone()));
        self.env.insert("True".into(), Scheme::mono(Ty::Con("Bool".into())));
        self.env.insert("False".into(), Scheme::mono(Ty::Con("Bool".into())));

        // List constructors
        self.constructors.insert(":".into(), ConInfo {
            type_name: "[]".into(), variant_index: 1, total_variants: 2,
            field_types: vec![ta.clone(), Ty::list(ta.clone())],
            type_vars: vec![a.clone()],
            result_type: Ty::list(ta.clone()),
            existential_vars: vec![],
            existential_constraints: vec![],
        });
        self.constructors.insert("[]".into(), ConInfo {
            type_name: "[]".into(), variant_index: 2, total_variants: 2,
            field_types: vec![],
            type_vars: vec![a.clone()],
            result_type: Ty::list(ta.clone()),
            existential_vars: vec![],
            existential_constraints: vec![],
        });
        // (:) :: a -> [a] -> [a]
        self.env_scheme(":", vec![a.clone()], Ty::fun(&[ta.clone(), Ty::list(ta.clone())], Ty::list(ta.clone())));
        // [] :: [a]
        self.env_scheme("[]", vec![a.clone()], Ty::list(ta.clone()));

        // LuaFunction and engage
        let s = TyVar { name: "s".into(), id: u32::MAX };
        let ts = Ty::Var(s.clone());

        // LuaFunction is just an opaque Con type — the scope var is
        // attached when it appears in a type signature as LuaFunction s
        // (handled by ast_type_to_ty via type application)

        // liftIO :: IO a -> LuaIO s a
        self.env_scheme("liftIO", vec![a.clone(), s.clone()],
            Ty::arrow(Ty::io(ta.clone()), Ty::lua_io(s.clone(), ta.clone())));

        // engage :: LuaFunction s -> a
        // (the type annotation at the call site determines a)
        // At runtime, engage is the identity — the LuaFunction is
        // already a Lua function, engage just satisfies the type system.
        self.env_scheme("engage", vec![a.clone(), s.clone()],
            Ty::arrow(Ty::app(Ty::Con("LuaFunction".into()), Ty::Var(s.clone())), ta.clone()));

        // ST s a — pure mutable state monad (same runtime as IO, type-level distinction only)
        // STArray s — mutable integer array, scoped to ST s
        let st_s = |inner: Ty| Ty::app(Ty::app(Ty::Con("ST".into()), ts.clone()), inner);
        let sta_s = Ty::app(Ty::Con("STArray".into()), ts.clone());

        // runST :: (forall s. ST s a) -> a
        // Rank-2: the s is universally quantified in the argument
        self.env_scheme("runST", vec![a.clone()],
            Ty::arrow(Ty::Forall(s.clone(), Box::new(st_s(ta.clone()))), ta.clone()));
        // newSTArray :: Int -> Int -> ST s (STArray s)
        self.env_scheme("newSTArray", vec![s.clone()],
            Ty::fun(&[int.clone(), int.clone()], st_s(sta_s.clone())));
        // readSTArray :: STArray s -> Int -> ST s Int
        self.env_scheme("readSTArray", vec![s.clone()],
            Ty::fun(&[sta_s.clone(), int.clone()], st_s(int.clone())));
        // writeSTArray :: STArray s -> Int -> Int -> ST s ()
        self.env_scheme("writeSTArray", vec![s.clone()],
            Ty::fun(&[sta_s.clone(), int.clone(), int.clone()], st_s(Ty::Unit)));
        // modifySTArray :: STArray s -> Int -> (Int -> Int) -> ST s ()
        self.env_scheme("modifySTArray", vec![s.clone()],
            Ty::fun(&[sta_s.clone(), int.clone(), Ty::arrow(int.clone(), int.clone())], st_s(Ty::Unit)));
        // stArrayLength :: STArray s -> ST s Int
        self.env_scheme("stArrayLength", vec![s.clone()],
            Ty::arrow(sta_s.clone(), st_s(int.clone())));
        // newSTArrayFromList :: [Int] -> ST s (STArray s)
        self.env_scheme("newSTArrayFromList", vec![s.clone()],
            Ty::arrow(Ty::list(int.clone()), st_s(sta_s.clone())));
        // stArrayToList :: STArray s -> ST s [Int]
        self.env_scheme("stArrayToList", vec![s.clone()],
            Ty::arrow(sta_s.clone(), st_s(Ty::list(int.clone()))));

        // IORef a — plain mutable state in IO (Data.IORef). Unlike STArray
        // it is polymorphic in the element and NOT region-scoped: the ref is
        // an ordinary first-class value whose ops live in IO. Laziness is
        // GHC's: writeIORef doesn't force the value, modifyIORef stores the
        // unevaluated `f old`, modifyIORef' forces the new value to WHNF.
        let ioref = |inner: Ty| Ty::app(Ty::Con("IORef".into()), inner);
        // newIORef :: a -> IO (IORef a)
        self.env_scheme("newIORef", vec![a.clone()],
            Ty::arrow(ta.clone(), Ty::io(ioref(ta.clone()))));
        // readIORef :: IORef a -> IO a
        self.env_scheme("readIORef", vec![a.clone()],
            Ty::arrow(ioref(ta.clone()), Ty::io(ta.clone())));
        // writeIORef :: IORef a -> a -> IO ()
        self.env_scheme("writeIORef", vec![a.clone()],
            Ty::fun(&[ioref(ta.clone()), ta.clone()], Ty::io(Ty::Unit)));
        // modifyIORef :: IORef a -> (a -> a) -> IO ()
        self.env_scheme("modifyIORef", vec![a.clone()],
            Ty::fun(&[ioref(ta.clone()), Ty::arrow(ta.clone(), ta.clone())], Ty::io(Ty::Unit)));
        // modifyIORef' :: IORef a -> (a -> a) -> IO ()
        self.env_scheme("modifyIORef'", vec![a.clone()],
            Ty::fun(&[ioref(ta.clone()), Ty::arrow(ta.clone(), ta.clone())], Ty::io(Ty::Unit)));

        // -- Functor → Applicative → Monad hierarchy --

        // Type abbreviations for higher-kinded method types
        let fa = Ty::App(Box::new(tf.clone()), Box::new(ta.clone()));
        let fb = Ty::App(Box::new(tf.clone()), Box::new(tb.clone()));
        let ma = Ty::App(Box::new(tm.clone()), Box::new(ta.clone()));
        let mb = Ty::App(Box::new(tm.clone()), Box::new(tb.clone()));

        // Built-in Functor typeclass
        // fmap :: (a -> b) -> f a -> f b
        let fmap_ty = Ty::fun(&[Ty::arrow(ta.clone(), tb.clone()), fa.clone()], fb.clone());
        self.register_builtin_class("Functor", "f", &[], vec![
            ("fmap", fmap_ty.clone(), Some(vec![a.clone(), b.clone(), f.clone()]), vec![]),
            ("<$>", fmap_ty, Some(vec![a.clone(), b.clone(), f.clone()]), vec![]),
        ]);

        // Functor instances (fmap and <$> map to same implementations)
        for tc_name in &["IO", "LuaIO", "ST"] {
            self.register_builtin_instance("Functor", Ty::Con(tc_name.to_string()),
                &[("fmap", "fmap_IO"), ("<$>", "fmap_IO")]);
        }
        self.register_builtin_instance("Functor", Ty::Con("[]".to_string()),
            &[("fmap", "map"), ("<$>", "map")]);
        for tc_name in &["Maybe", "Either"] {
            let impl_fn = format!("fmap_{}", tc_name);
            // Empty context, NOT None: a higher-kinded instance demands
            // nothing of the constructor's own type arguments, so the
            // structural fallback rule in `has_instance` (meant for
            // Show/Eq-style element checking) must not apply. Without
            // this, a wanted like `Functor (Either String)` — where the
            // class variable binds to a partially-applied constructor —
            // would wrongly require `Functor String`.
            self.register_builtin_instance_empty_ctx("Functor", Ty::Con(tc_name.to_string()),
                &[("fmap", impl_fn.clone()), ("<$>", impl_fn)]);
        }

        // Built-in Applicative typeclass (superclass: Functor)
        // pure   :: a -> f a
        // (<*>)  :: f (a -> b) -> f a -> f b
        // liftA2 :: (a -> b -> c) -> f a -> f b -> f c
        // liftA2 is a real method (as in GHC), not sugar for <$>/<*>: the
        // <$>/<*> chain routes a FUNCTION through the applicative (an
        // `f (b -> c)` intermediate), and the type-erased IO runtime cannot
        // represent an action whose result is itself a Lua function
        // (__mll_run could not tell it from an unrun action). liftA2 keeps
        // only fully-applied values in the container, so generic Applicative
        // code (traverse) works at IO too.
        let pure_ty = Ty::arrow(ta.clone(), fa.clone());
        let fab = Ty::App(Box::new(tf.clone()), Box::new(Ty::arrow(ta.clone(), tb.clone())));
        let ap_ty = Ty::fun(&[fab, fa.clone()], fb.clone());
        let fc = Ty::App(Box::new(tf.clone()), Box::new(tc.clone()));
        let lifta2_ty = Ty::fun(
            &[Ty::fun(&[ta.clone(), tb.clone()], tc.clone()), fa.clone(), fb.clone()],
            fc,
        );
        self.register_builtin_class("Applicative", "f", &["Functor"], vec![
            ("pure", pure_ty.clone(), Some(vec![a.clone(), f.clone()]), vec![]),
            ("<*>", ap_ty, Some(vec![a.clone(), b.clone(), f.clone()]), vec![]),
            ("liftA2", lifta2_ty, Some(vec![a.clone(), b.clone(), c.clone(), f.clone()]), vec![]),
        ]);
        // `return`'s env entry is the Applicative `pure` scheme; the Monad
        // method of the same name (below) deliberately adds no second one.
        self.env_scheme("return", vec![a.clone(), f.clone()], pure_ty);

        // Applicative instances
        for tc_name in &["IO", "LuaIO", "ST"] {
            self.register_builtin_instance("Applicative", Ty::Con(tc_name.to_string()),
                &[("pure", "pure"), ("<*>", "ap_IO"), ("liftA2", "liftA2_IO")]);
        }
        self.register_builtin_instance("Applicative", Ty::Con("[]".to_string()),
            &[("pure", "pure_List"), ("<*>", "ap_List"), ("liftA2", "liftA2_List")]);
        self.register_builtin_instance("Applicative", Ty::Con("Maybe".to_string()),
            &[("pure", "pure_Maybe"), ("<*>", "ap_Maybe"), ("liftA2", "liftA2_Maybe")]);
        // Empty context, not None — see the Functor Either instance.
        self.register_builtin_instance_empty_ctx("Applicative", Ty::Con("Either".to_string()),
            &[("pure", "pure_Either"), ("<*>", "ap_Either"), ("liftA2", "liftA2_Either")]);

        // Built-in Monad typeclass (superclass: Applicative)
        // >>=    :: m a -> (a -> m b) -> m b
        // >>     :: m a -> m b -> m b
        // return :: a -> m a
        self.register_builtin_class("Monad", "m", &["Applicative"], vec![
            (">>=", Ty::fun(&[ma.clone(), Ty::arrow(ta.clone(), mb.clone())], mb.clone()),
                Some(vec![a.clone(), b.clone(), m.clone()]), vec![]),
            (">>", Ty::fun(&[ma.clone(), mb.clone()], mb.clone()),
                Some(vec![a.clone(), b.clone(), m.clone()]), vec![]),
            ("return", Ty::arrow(ta.clone(), ma.clone()), None, vec![]),
        ]);

        // Monad instances for IO, LuaIO, ST
        for monad_name in &["IO", "LuaIO", "ST"] {
            self.register_builtin_instance("Monad", Ty::Con(monad_name.to_string()),
                &[(">>=", ">>="), (">>", ">>"), ("return", "pure")]);
        }

        // Monad instance for [] (lists)
        self.register_builtin_instance("Monad", Ty::Con("[]".to_string()),
            &[(">>=", "bind_List"), (">>", "then_List"), ("return", "pure_List")]);

        // Monad instance for Maybe
        self.register_builtin_instance("Monad", Ty::Con("Maybe".to_string()),
            &[(">>=", "bind_Maybe"), (">>", "then_Maybe"), ("return", "pure_Maybe")]);

        // Built-in Foldable typeclass
        // foldr :: (a -> b -> b) -> b -> t a -> b
        // foldl :: (b -> a -> b) -> b -> t a -> b
        // The remaining GHC Foldable vocabulary (length, null, elem, sum,
        // product, maximum, minimum, foldMap, toList) is defined generically
        // over these two methods in the Prelude / Data.Foldable.
        let t = TyVar { name: "t".into(), id: u32::MAX };
        let tt = Ty::Var(t.clone());
        let ta_in_t = Ty::App(Box::new(tt.clone()), Box::new(ta.clone()));
        let foldr_ty = Ty::fun(
            &[Ty::fun(&[ta.clone(), tb.clone()], tb.clone()), tb.clone(), ta_in_t.clone()],
            tb.clone(),
        );
        let foldl_ty = Ty::fun(
            &[Ty::fun(&[tb.clone(), ta.clone()], tb.clone()), tb.clone(), ta_in_t.clone()],
            tb.clone(),
        );
        // Emit wanted constraints at use sites so a fold over a type without
        // a Foldable instance — or an ambiguous one like `Right 5` with an
        // undetermined Left type — is a compile error with the annotation
        // hint, not a deferred dispatch that fails at runtime.
        self.register_builtin_class("Foldable", "t", &[], vec![
            ("foldr", foldr_ty, Some(vec![a.clone(), b.clone(), t.clone()]), vec![("Foldable", "t")]),
            ("foldl", foldl_ty, Some(vec![a.clone(), b.clone(), t.clone()]), vec![("Foldable", "t")]),
        ]);

        // The Foldable instances for [], Maybe and Either (folds over Right,
        // like GHC) are ordinary `instance Foldable …` declarations in
        // Prelude.mll — the kind system checks their heads against the class
        // variable's Type -> Type kind like any user instance. Tuples
        // deliberately have no instance: the class variable has kind
        // Type -> Type and mata-ll has no partially-applied tuple constructor
        // (consistent with tuples having no Ord instance either).

        // Built-in Traversable typeclass (superclasses: Functor, Foldable)
        // traverse :: Applicative f => (a -> f b) -> t a -> f (t b)
        // sequenceA is defined in the Prelude as `traverse (\x -> x)`.
        let tb_in_t = Ty::App(Box::new(tt.clone()), Box::new(tb.clone()));
        let traverse_ty = Ty::fun(
            &[Ty::arrow(ta.clone(), fb.clone()), ta_in_t.clone()],
            Ty::App(Box::new(tf.clone()), Box::new(tb_in_t)),
        );
        // `traverse` carries TWO constraints — the class's own (`Traversable
        // t`) and one on its applicative (`Applicative f`) — which is why the
        // constraint field is a list of (class, variable) pairs.
        self.register_builtin_class("Traversable", "t", &["Functor", "Foldable"], vec![
            ("traverse", traverse_ty, Some(vec![a.clone(), b.clone(), f.clone(), t.clone()]),
                vec![("Traversable", "t"), ("Applicative", "f")]),
        ]);
        // Like Foldable, the Traversable instances for [], Maybe and Either
        // live in Prelude.mll as ordinary `instance Traversable …`
        // declarations.

        // Built-in Enum typeclass
        // succ :: a -> a
        // pred :: a -> a
        // toEnum :: Int -> a
        // fromEnum :: a -> Int
        // enumFrom :: a -> [a]
        // enumFromThen :: a -> a -> [a]
        // enumFromTo :: a -> a -> [a]
        // enumFromThenTo :: a -> a -> a -> [a]
        let succ_ty = Ty::arrow(ta.clone(), ta.clone());
        let to_enum_ty = Ty::arrow(Ty::Con("Int".into()), ta.clone());
        let from_enum_ty = Ty::arrow(ta.clone(), Ty::Con("Int".into()));
        let enum_from_ty = Ty::arrow(ta.clone(), Ty::List(Box::new(ta.clone())));
        let enum_from_then_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::List(Box::new(ta.clone())));
        let enum_from_to_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::List(Box::new(ta.clone())));
        let enum_from_then_to_ty = Ty::fun(&[ta.clone(), ta.clone(), ta.clone()], Ty::List(Box::new(ta.clone())));
        self.register_builtin_class("Enum", "a", &[], vec![
            ("succ", succ_ty.clone(), Some(vec![a.clone()]), vec![]),
            ("pred", succ_ty, Some(vec![a.clone()]), vec![]),
            ("toEnum", to_enum_ty, Some(vec![a.clone()]), vec![]),
            ("fromEnum", from_enum_ty, Some(vec![a.clone()]), vec![]),
            ("enumFrom", enum_from_ty, Some(vec![a.clone()]), vec![]),
            ("enumFromThen", enum_from_then_ty, Some(vec![a.clone()]), vec![]),
            ("enumFromTo", enum_from_to_ty, Some(vec![a.clone()]), vec![]),
            ("enumFromThenTo", enum_from_then_to_ty, Some(vec![a.clone()]), vec![]),
        ]);

        // Enum instance for Int
        self.register_builtin_instance("Enum", Ty::Con("Int".to_string()), &[
            ("succ", "succ_Int"), ("pred", "pred_Int"),
            ("toEnum", "toEnum_Int"), ("fromEnum", "fromEnum_Int"),
            ("enumFrom", "enumFrom_Int"), ("enumFromThen", "enumFromThen_Int"),
            ("enumFromTo", "enumFromTo_Int"), ("enumFromThenTo", "enumFromThenTo_Int"),
        ]);

        // HashMap KEY constraint (round-3 Q54): the runtime map is a plain
        // Lua table keyed by the forced key, so only types whose Lua
        // representation has VALUE semantics can be keys. A boxed Integer
        // key is a Lua table — identity semantics: lookups missed, size
        // grew per insert of "the same" key, and hashmap_keys' table.sort
        // crashed. Hashable is a method-less marker class over the scalar
        // key types; the key-taking hm* functions carry it, so an Integer
        // key is a compile-time "No instance for 'Hashable Integer'".
        self.register_builtin_class("Hashable", "a", &[], vec![]);
        for t in &["Int", "Number", "String", "Bool", "ByteString"] {
            self.register_builtin_instance::<&str, &str>("Hashable", Ty::Con(t.to_string()), &[]);
        }
        for name in &["hmInsert", "hmLookup", "hmDelete", "hmMember", "hmFromList"] {
            self.fn_contexts.insert(name.to_string(), FnContext {
                declared: vec![("Hashable".to_string(), ta.clone())],
                at_use: vec![("Hashable".to_string(), ta.clone())],
                ..FnContext::default()
            });
        }

        // Built-in Bounded typeclass
        let min_bound_ty = ta.clone();
        let max_bound_ty = ta.clone();
        self.register_builtin_class("Bounded", "a", &[], vec![
            ("minBound", min_bound_ty, Some(vec![a.clone()]), vec![]),
            ("maxBound", max_bound_ty, Some(vec![a.clone()]), vec![]),
        ]);
        // Bounded Int / Bounded Bool (GHC parity); runtime constants in
        // runtime.lua (minBound_Int is math.mininteger on 5.3+, the
        // closest double on LuaJIT).
        self.register_builtin_instance("Bounded", Ty::Con("Int".to_string()), &[
            ("minBound", "minBound_Int"), ("maxBound", "maxBound_Int"),
        ]);
        self.register_builtin_instance("Bounded", Ty::Con("Bool".to_string()), &[
            ("minBound", "minBound_Bool"), ("maxBound", "maxBound_Bool"),
        ]);

        // Class constraints carried by the built-in class methods (the
        // constraint field of each registration below, and of the numeric
        // classes further down). Each constrains the class variable "a"; a
        // use whose "a" resolves to a concrete type with no instance (a
        // function, an IO action, a type without the relevant deriving) is
        // rejected at the function boundary.

        // Built-in Show typeclass
        let show_ty = Ty::arrow(ta.clone(), Ty::Con("String".into()));
        self.register_builtin_class("Show", "a", &[], vec![
            ("show", show_ty, Some(vec![a.clone()]), vec![("Show", "a")]),
        ]);

        // Built-in Read typeclass
        let read_ty = Ty::arrow(Ty::Con("String".into()), ta.clone());
        self.register_builtin_class("Read", "a", &[], vec![
            ("read", read_ty, Some(vec![a.clone()]), vec![("Read", "a")]),
        ]);
        // Read instances for base types
        for type_name in &["Int", "Integer", "Number", "Bool", "String"] {
            self.register_builtin_instance("Read", Ty::Con(type_name.to_string()),
                &[("read", format!("read_{}", type_name))]);
        }

        // Built-in Eq typeclass
        let eq_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::Con("Bool".into()));
        self.register_builtin_class("Eq", "a", &[], vec![
            ("==", eq_ty, Some(vec![a.clone()]), vec![("Eq", "a")]),
        ]);
        // /= is derived from ==
        self.env_scheme("/=", vec![a.clone()], Ty::fun(&[ta.clone(), ta.clone()], Ty::Con("Bool".into())));
        self.method_constraints.insert("/=".to_string(), vec![TyConstraint {
            class_name: "Eq".to_string(),
            type_var: "a".to_string(),
        }]);

        // Eq instances for base types
        for type_name in &["Int", "Integer", "Number", "String", "Bool", "ByteString"] {
            self.register_builtin_instance("Eq", Ty::Con(type_name.to_string()),
                &[("==", format!("eq_{}", type_name))]);
        }
        // Eq (IORef a) — pointer identity, context-free like GHC's instance
        // (no `Eq a` demanded of the element; two refs are == iff they are
        // the same cell, whatever they hold).
        self.register_builtin_instance_empty_ctx("Eq", Ty::Con("IORef".to_string()),
            &[("==", "eq_IORef")]);

        // Built-in Ord typeclass (superclass: Eq)
        let cmp_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::Con("Bool".into()));
        // `compare` is an Ord method returning Ordering (defined in the prelude).
        let compare_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::Con("Ordering".into()));
        // `max`/`min` return the class type itself, not Bool.
        let sel_ty = Ty::fun(&[ta.clone(), ta.clone()], ta.clone());
        self.register_builtin_class("Ord", "a", &["Eq"], vec![
            ("<", cmp_ty.clone(), Some(vec![a.clone()]), vec![("Ord", "a")]),
            (">", cmp_ty.clone(), Some(vec![a.clone()]), vec![("Ord", "a")]),
            ("<=", cmp_ty.clone(), Some(vec![a.clone()]), vec![("Ord", "a")]),
            (">=", cmp_ty, Some(vec![a.clone()]), vec![("Ord", "a")]),
            ("compare", compare_ty, Some(vec![a.clone()]), vec![("Ord", "a")]),
            ("max", sel_ty.clone(), Some(vec![a.clone()]), vec![("Ord", "a")]),
            ("min", sel_ty, Some(vec![a.clone()]), vec![("Ord", "a")]),
        ]);

        // Ord instances for base types
        // Bool included: GHC has Ord Bool (False < True); it was missing.
        for type_name in &["Int", "Integer", "Number", "String", "ByteString", "Bool"] {
            let mut fns: Vec<(String, String)> = ["<", ">", "<=", ">="].iter()
                .map(|op| (op.to_string(), format!("ord_{}__{}", op_to_name(op), type_name)))
                .collect();
            // Every base Ord type has `compare`/`max`/`min` runtime helpers.
            for m in &["compare", "max", "min"] {
                fns.push((m.to_string(), format!("ord_{}__{}", m, type_name)));
            }
            self.register_builtin_instance("Ord", Ty::Con(type_name.to_string()), &fns);
        }

        // ===================================================================
        // Numeric class hierarchy: Num, Fractional, Real, Integral.
        // Registered built-in (like Eq/Ord) rather than as source classes,
        // because the operator methods (+ - * / div mod quot rem) must map to
        // THEMSELVES in the Int/Number instances so the monomorphizer keeps
        // them as inline InfixApp (byte-identical concrete arithmetic) — a
        // self-reference no source `instance` body can express. The named
        // methods (negate/abs/signum/fromInteger/recip/fromRational/toInteger/
        // quotRem/divMod) dispatch to small runtime helpers emitted on demand.
        // ===================================================================
        {
            let bin = Ty::fun(&[ta.clone(), ta.clone()], ta.clone());
            let un = Ty::arrow(ta.clone(), ta.clone());
            // GHC-faithful: `fromInteger :: Integer -> a` — a numeric literal is
            // an `Integer` that `fromInteger` lowers to the target Num type.
            let from_integer_ty = Ty::arrow(Ty::Con("Integer".into()), ta.clone());
            // `toInteger :: a -> Integer` (restored now that Integer exists).
            let to_integer_ty = Ty::arrow(ta.clone(), Ty::Con("Integer".into()));
            // No Rational type in mata-ll: a fractional literal is a Number
            // (f64) at the source level, so `fromRational` takes that same
            // representation. Documented as the single numeric-tower deviation.
            let from_rational_ty = Ty::arrow(Ty::Con("Number".into()), ta.clone());
            let pair = Ty::Tuple(vec![ta.clone(), ta.clone()]);
            let to_pair = Ty::fun(&[ta.clone(), ta.clone()], pair);

            // The operator methods' env schemes were registered in the
            // arithmetic block above, so they carry `None`; the named methods
            // get their `forall a` env schemes here.

            // ---- class Num a ----
            self.register_builtin_class("Num", "a", &[], vec![
                ("+", bin.clone(), None, vec![("Num", "a")]),
                ("-", bin.clone(), None, vec![("Num", "a")]),
                ("*", bin.clone(), None, vec![("Num", "a")]),
                ("negate", un.clone(), Some(vec![a.clone()]), vec![("Num", "a")]),
                ("abs", un.clone(), Some(vec![a.clone()]), vec![("Num", "a")]),
                ("signum", un.clone(), Some(vec![a.clone()]), vec![("Num", "a")]),
                ("fromInteger", from_integer_ty, Some(vec![a.clone()]), vec![("Num", "a")]),
            ]);
            // ---- class Num a => Fractional a ----
            self.register_builtin_class("Fractional", "a", &["Num"], vec![
                ("/", bin.clone(), None, vec![("Fractional", "a")]),
                ("recip", un.clone(), Some(vec![a.clone()]), vec![("Fractional", "a")]),
                ("fromRational", from_rational_ty, Some(vec![a.clone()]), vec![("Fractional", "a")]),
            ]);
            // ---- class (Num a, Ord a) => Real a ----
            // GHC's `Real` has `toRational :: a -> Rational`; mata-ll has no
            // Rational, so the class is a superclass marker with no methods.
            self.register_builtin_class("Real", "a", &["Num", "Ord"], vec![]);
            // ---- class (Real a, Enum a) => Integral a ----
            self.register_builtin_class("Integral", "a", &["Real", "Enum"], vec![
                ("quot", bin.clone(), None, vec![("Integral", "a")]),
                ("rem", bin.clone(), None, vec![("Integral", "a")]),
                ("div", bin.clone(), None, vec![("Integral", "a")]),
                ("mod", bin, None, vec![("Integral", "a")]),
                ("quotRem", to_pair.clone(), Some(vec![a.clone()]), vec![("Integral", "a")]),
                ("divMod", to_pair, Some(vec![a.clone()]), vec![("Integral", "a")]),
                ("toInteger", to_integer_ty, Some(vec![a.clone()]), vec![("Integral", "a")]),
            ]);

            // ---- instances ----
            // Operators self-map (mono keeps them inline); named methods point
            // at runtime helpers. See codegen PRELUDE for the helper bodies.
            // Num Int
            self.register_builtin_instance("Num", Ty::Con("Int".to_string()),
                &[("+", "+"), ("-", "-"), ("*", "*"),
                    ("negate", "negate_Int"), ("abs", "abs_Int"),
                    ("signum", "signum_Int"), ("fromInteger", "fromInteger_Int")]);
            // Num Number
            self.register_builtin_instance("Num", Ty::Con("Number".to_string()),
                &[("+", "+"), ("-", "-"), ("*", "*"),
                    ("negate", "negate_Number"), ("abs", "abs_Number"),
                    ("signum", "signum_Number"), ("fromInteger", "fromInteger_Number")]);
            // Fractional Number (Integer is deliberately NOT Fractional, as GHC)
            self.register_builtin_instance("Fractional", Ty::Con("Number".to_string()),
                &[("/", "/"), ("recip", "recip_Number"),
                    ("fromRational", "fromRational_Number")]);
            // Real Int / Real Number (no methods; just evidence Ord+Num).
            self.register_builtin_instance::<&str, &str>("Real", Ty::Con("Int".to_string()), &[]);
            self.register_builtin_instance::<&str, &str>("Real", Ty::Con("Number".to_string()), &[]);
            // Integral Int (Number is NOT Integral, as GHC). Operators self-map
            // (mono keeps them inline); toInteger lifts to a bignum.
            self.register_builtin_instance("Integral", Ty::Con("Int".to_string()),
                &[("div", "div"), ("mod", "mod"), ("quot", "quot"), ("rem", "rem"),
                    ("quotRem", "quotRem_Int"), ("divMod", "divMod_Int"),
                    ("toInteger", "toInteger_Int")]);

            // ---- Arbitrary-precision Integer instances ----
            // Unlike Int/Number, the operators do NOT self-map: every method
            // routes to a bignum runtime helper (see codegen runtime.lua), so
            // the monomorphizer materialises them as calls rather than inline
            // Lua arithmetic.
            self.register_builtin_instance("Num", Ty::Con("Integer".to_string()),
                &[("+", "add_Integer"), ("-", "sub_Integer"), ("*", "mul_Integer"),
                    ("negate", "negate_Integer"), ("abs", "abs_Integer"),
                    ("signum", "signum_Integer"), ("fromInteger", "fromInteger_Integer")]);
            self.register_builtin_instance::<&str, &str>("Real", Ty::Con("Integer".to_string()), &[]);
            self.register_builtin_instance("Integral", Ty::Con("Integer".to_string()),
                &[("div", "div_Integer"), ("mod", "mod_Integer"),
                    ("quot", "quot_Integer"), ("rem", "rem_Integer"),
                    ("quotRem", "quotRem_Integer"), ("divMod", "divMod_Integer"),
                    ("toInteger", "toInteger_Integer")]);
            // Enum Integer: the helpers are mata-ll source in lib/Prelude.mll
            // (like Enum Int), so they dispatch through the Integer instances.
            {
                let fns: Vec<(String, String)> = ["succ", "pred", "toEnum", "fromEnum",
                        "enumFrom", "enumFromThen", "enumFromTo", "enumFromThenTo"]
                    .iter().map(|m| (m.to_string(), format!("{m}_Integer"))).collect();
                self.register_builtin_instance("Enum", Ty::Con("Integer".to_string()), &fns);
            }
        }

        // The Semigroup and Monoid CLASS declarations are now ordinary source
        // classes in lib/Prelude.mll (`class Semigroup a where (<>) :: …` and
        // `class Semigroup a => Monoid a where { mempty; mappend }`). Their
        // method env entries and their per-method class constraints —
        // including the `mempty` ambiguity check — are synthesized by
        // `register_class` exactly as for any user class, so nothing about
        // them needs to be hard-registered here anymore. Only the runtime
        // string-concatenation primitive their String instances call stays a
        // builtin (below), because Lua `..` has no source-level spelling.

        // `semigroup_String` is the runtime string-concatenation primitive
        // (Lua `..`, defined in codegen's preamble and inlined at call sites).
        // mata-ll String is opaque — unlike GHC's `[Char]` it has no `++` — so
        // this is the ONLY way to concatenate two Strings, and the Prelude's
        // `instance Semigroup String` / `instance Monoid String` bodies call
        // it by name. Registering it in the environment makes those source
        // instance bodies type-check; codegen already knows the name. (The
        // list instances use the ordinary `++` operator instead, so no such
        // primitive is exposed for lists.)
        self.env_scheme("semigroup_String", vec![],
            Ty::fun(&[Ty::Con("String".into()), Ty::Con("String".into())], Ty::Con("String".into())));

        // The Semigroup/Monoid classes and their String/[a] instances all
        // live in lib/Prelude.mll now. The deliberate mata-ll divergence —
        // `<>` on a concrete list type is rejected in favour of `++` — lives
        // in the monomorphizer's dispatch (`resolve_at_type`), keyed on the
        // class name from the (now source) class registration, so it is
        // unaffected by the move. `mappend` still dispatches on lists (its
        // instance body is `xs ++ ys`), and an undetermined `mempty` is an
        // ambiguity error via the constraint `register_class` synthesizes for
        // it, exactly as for the builtin `mempty` before.

        // Show instances for base types and parameterized types
        for type_name in &["Int", "Integer", "Number", "String", "Bool", "[]", "Maybe", "ByteString"] {
            self.register_builtin_instance("Show", Ty::Con(type_name.to_string()),
                &[("show", format!("show_{}", type_name))]);
        }

        // `()` is a base type like any other and carries the GHC base
        // instances Show/Eq/Ord. It is registered separately from the loops
        // above because its instance key is the type string "()" (matching
        // `format!("{}", Ty::Unit)`) while its mangled runtime names must be
        // identifier-safe (`show_Unit`, not `show_()`). Runtime rep is nil,
        // so eq/ord are trivial (nil == nil; compare is always EQ).
        self.register_builtin_instance("Show", Ty::Unit, &[("show", "show_Unit")]);
        self.register_builtin_instance("Eq", Ty::Unit, &[("==", "eq_Unit")]);
        self.register_builtin_instance("Ord", Ty::Unit, &[
            ("<", "ord_lt__Unit"), (">", "ord_gt__Unit"),
            ("<=", "ord_le__Unit"), (">=", "ord_ge__Unit"),
            ("compare", "ord_compare__Unit"),
            ("max", "ord_max__Unit"), ("min", "ord_min__Unit")]);
    }
}
