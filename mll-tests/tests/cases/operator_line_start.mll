-- Test: a continuation line that BEGINS with a dash-headed operator
-- (`-->`, `--.`) is code, not a comment. The lexer's line-start comment
-- scan lacked the operator exemption its mid-line scan had, so
--   x = a
--     --> b
-- silently compiled as `x = a`. A run of dashes alone (`---`) is still a
-- comment, as is `-- text`.

infixr 1 -->
(-->) :: Bool -> Bool -> Bool
(-->) a b = not a || b

infixl 6 --.
(--.) :: Int -> Int -> Int
(--.) a b = a - b - 1

implication :: Bool
implication = True
    --> False

chained :: Bool
chained = False
    --> True
    --> False

minusOne :: Int
minusOne = 10
    --. 3
    --. 2      --- a real comment: dashes only, then text
    -- and an ordinary comment line between continuations
    --. 1

main :: IO ()
main = do
    assert (implication == False) "True --> False on a continuation line is False"
    assert (chained == True) "False --> (True --> False) is True"
    assert (minusOne == 1) "((10 --. 3) --. 2) --. 1 is 1"
