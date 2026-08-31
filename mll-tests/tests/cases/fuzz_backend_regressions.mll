-- Regressions found by backend_fuzz (the type-correct backend fuzzer),
-- one per section, each with the batch index that surfaced it. All three
-- were found within the first 66 generated programs of the first batch.
--
-- (1) index 34: an IIFE argument moved into a single-name local RHS kept
--     its call-position paren (needed there for multi-return truncation,
--     redundant in a local RHS); pass 1 had already run, so the leftover
--     survived to the emitted tree and the idempotence refutation flagged
--     it. A lambda applied to a constructor application in a lazy list
--     element is the shape.
--
-- (2) index 61: `const 5 True` was the only VISIBLE call site of const,
--     judging its first parameter always-cheap (no entry force) — while
--     the first-class `const` handed to flip escaped the site scan and
--     flip's flat call passed a raw thunk, which const's bare return
--     forwarded: a thunk body returning a raw thunk (caught by the WHNF
--     refutation's checked __force). An escaping reference now poisons
--     the always-cheap judgment.
--
-- (3) index 66: `[]`'s element type stays a dead polymorphic variable, so
--     `flip` kept the shared generic copy (whose body calls `f(a, b)`
--     flat) while the first-class `const` was rightly widened to three
--     parameters — f consumed its eta slot as nil and flip's real result
--     was applied to a boolean ("attempt to call a boolean value").
--     Arity-widening specialization now fires on the canonicalized type
--     even when dead variables remain.

main :: IO ()
main = do
    -- (1) paren idempotence: lambda over a constructor arg, lazy element
    assert (length [(\v -> 3) (Just False)] == 1) "paren idempotence"
    print ((length [(\v -> 3) (Just False)]) :: Int)
    -- (2) escaping reference vs always-cheap params
    print ((const 5 True) :: Int)
    print (((flip const (Just True) (null [])), 1) :: (Bool, Int))
    -- (3) widened builtin at a type with a dead variable
    print (((flip const True (\v -> False)) []) :: Bool)
