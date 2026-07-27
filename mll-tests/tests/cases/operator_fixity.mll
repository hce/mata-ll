-- Operator fixity declaration tests

-- ============================================================
-- Custom operators with fixity
-- ============================================================

-- Define a custom right-associative cons-like operator
infixr 5 <|

(<|) :: a -> [a] -> [a]
(<|) x xs = x : xs

-- Define a custom left-associative operator
infixl 6 <+>

(<+>) :: Int -> Int -> Int
(<+>) a b = a + b + 1

-- Define a non-associative comparison-like operator
infix 4 ===

(===) :: Int -> Int -> Bool
(===) a b = a == b

-- ============================================================
-- Override default precedence
-- ============================================================

-- Custom operator with lower precedence than + (prec 6)
infixl 4 |+|

(|+|) :: Int -> Int -> Int
(|+|) a b = a + b

-- Custom operator with higher precedence than + (prec 6)
infixl 8 |*|

(|*|) :: Int -> Int -> Int
(|*|) a b = a * b

main :: IO ()
main = do
    -- Right-associative: 1 <| 2 <| 3 <| [] == 1 : (2 : (3 : []))
    assert (1 <| 2 <| 3 <| [] == [1, 2, 3]) "infixr cons-like"

    -- Left-associative custom op
    assert (1 <+> 2 == 4) "custom left-assoc basic"
    -- 1 <+> 2 <+> 3 = ((1 <+> 2) <+> 3) = (4 <+> 3) = 8
    assert (1 <+> 2 <+> 3 == 8) "custom left-assoc chain"

    -- Non-associative
    assert (42 === 42) "custom infix eq true"
    assert (not (42 === 43)) "custom infix eq false"

    -- Precedence: |+| has prec 4, * has prec 7
    -- 1 |+| 2 * 3 should be 1 |+| (2 * 3) = 1 + 6 = 7
    -- (parenthesized against ==, which shares precedence 4 with |+| —
    -- an unparenthesized mix of infixl 4 and infix 4 is rejected, as in GHC)
    assert ((1 |+| 2 * 3) == 7) "low prec: |+| after *"

    -- |*| has prec 8 (higher than +, prec 6)
    -- 1 + 2 |*| 3 should be 1 + (2 |*| 3) = 1 + 6 = 7
    assert (1 + 2 |*| 3 == 7) "high prec: |*| before +"

    -- Mix of custom and built-in operators
    -- <+> has prec 6 (same as +), left-assoc
    -- 2 * 3 <+> 4 = (2 * 3) <+> 4 = 6 <+> 4 = 11
    assert (2 * 3 <+> 4 == 11) "custom mixed with builtin"

    -- Operator sections with custom operators
    let f = (<+> 10)
    assert (f 5 == 16) "custom op section right"

    let g = (100 |+|)
    assert (g 42 == 142) "custom op section left"

    -- Declared fixities drive the section-operand precedence rule
    -- (Haskell 2010 §3.5) exactly like the builtin ones: the operand must
    -- bind tighter than the section operator, or chain with it at equal
    -- precedence in the section's own direction.
    assert ((<| 2 <| []) 1 == [1, 2]) "right section, declared infixr chain"
    assert ((<+> 2 |*| 3) 1 == 8) "right section, tighter declared operand"
    assert ((1 <+> 2 <+>) 3 == 8) "left section, declared infixl chain"
    assert ((2 |*| 3 |+|) 1 == 7) "left section, tighter declared operand"
