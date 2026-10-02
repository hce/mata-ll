import FixityTypeOps

data (:-:) a b = M a b

second :: Int :+: Bool :*: Int -> Int
second (L n) = n
second (R (P _ n)) = n

nested :: Int :-: Bool :-: Int -> Int
nested (M (M a _) c) = a + c

main :: IO ()
main = do
    assert (leftmost (R (L (P True 7))) == 7) "the defining module groups by its declarations"
    assert (second (R (P True 5)) == 5) "an imported type operator arrives with its fixity"
    assert (second (L 3) == 3) "the other alternative"
    assert (nested (M (M 1 True) 2) == 3) "an own type operator without a declaration is infixl 9"
    print (leftmost (R (L (P True 7))), second (R (P True 5)), second (L 3), nested (M (M 1 True) 2))
