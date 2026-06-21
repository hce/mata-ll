# Bugs found by short manual programs, not by large tests

An extensive test suite was generated with Claude Code — an Impulse Tracker
decoder, several cryptographic algorithms checked against NIST test vectors,
and classic data structures such as a red-black tree. Those caught many subtle
bugs, but a handful slipped through and were instead found by a few lines of
throwaway code written by hand. This file collects them.

The pattern is consistent. The large tests are elaborate but *correlated*: one
author, a few idioms repeated at scale, all sampling the same corner of the
language. A short program written from a different angle reaches somewhere the
suite never did. Complexity is not coverage.

## Recursive lazy value in a `where` / `let` binding

This program printed `(1, 1, 144, 233)` instead of the expected
`(144, 233, 144, 233)`:

```haskell
fib' :: [Integer]
fib' = [1, 1] ++ zipWith (+) fib' (drop 1 fib')

fibonacci' :: Integer -> Integer
fibonacci' = head . reverse . flip take fib'

fibonacci :: Integer -> Integer
fibonacci = head . reverse . flip take fib
  where
    fib = [1, 1] ++ zipWith (+) fib (drop 1 fib)

main :: IO ()
main = print ((fibonacci 12), (fibonacci 13), (fibonacci' 12), (fibonacci' 13))
```

The top-level `fib'` was correct; the identical list bound in a `where` clause
(`fib`) collapsed to `[1, 1]`. Cause: the binding compiled to
`local fib = __thunk(... __force(fib) ...)`, but a Lua local is not in scope
inside its own initializer, so the inner `fib` resolved to a nil global —
`zipWith` over nil yields the empty list. The fix forward-declares
`where` / `let` names before assigning them. Investigating it also revealed
that `let` was not recursive *at all* in the type checker; that was fixed too.

Why the suite missed it: not one of the ~270 tests bound a recursive *value*
(a CAF) in local scope. They always recursed through named top-level functions.

## Prime sieve over an infinite list

This one line did not terminate (stack overflow):

```haskell
let primes = 2:3:5:[x | x <- [6..], length (filter ((== 0) . (x `mod`)) $ takeWhile (< (x `div` 2)) primes) == 0]
```

The algorithm is correct — it does produce the primes. mata-ll's non-strict
evaluation was not. Tracing it down uncovered a chain of strictness shortcuts
that had leaked into lazy positions, each masking the next:

- a one-level function call passed as an argument was evaluated eagerly, so the
  recursive call inside `concatMap` (what a list comprehension desugars to)
  looped on the infinite generator;
- `x : rest` force-evaluated a variable tail, collapsing the list spine;
- lambda parameters were emitted unforced and broke when a higher-order call
  passed a thunk;
- the strictness analysis never scanned guard branches, so a recursive call
  appearing only in a guard mislabeled its parameter.

The repair had to stay surgical: a blunt "force every operand" version doubled
the tracker's decode time, so only saturated calls to inlinable helpers stay
eager.

Why the suite missed it: none of the generated tests exercised laziness over an
infinite structure. They all worked on finite, fully forced data.
