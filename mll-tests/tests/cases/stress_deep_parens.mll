-- Regression: deeply nested *parenthesised* application/constructor.
-- The parser detected a left operator section `(expr op)` by parsing the
-- parenthesised body speculatively and, on the (common) non-section path,
-- backtracking and parsing it AGAIN. Each nesting level parsed its body
-- twice, so a depth-n nest cost O(2^n) parse time — depth ~25 already took
-- seconds and 40 effectively hung the compiler. The fix continues infix
-- parsing from the already-parsed lhs (parse-once), making it O(n).
-- 40-deep here compiles instantly post-fix and stays within test budget.

inc :: Integer -> Integer
inc n = n + 1

data Nat = Z | S Nat
natDepth :: Nat -> Integer
natDepth Z = 0
natDepth (S n) = 1 + natDepth n

deepApp :: Integer
deepApp = inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc (inc 0)))))))))))))))))))))))))))))))))))))))

deepCon :: Nat
deepCon = S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S (S Z)))))))))))))))))))))))))))))))))))))))

main :: IO ()
main = do
    assert (deepApp == 40) "deeply nested parenthesised application"
    assert (natDepth deepCon == 40) "deeply nested parenthesised constructor"
    putStrLn "deep parens: OK"
