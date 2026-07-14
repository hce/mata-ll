-- Foldable: foldr/foldl are class methods (instances: [], Maybe, Either);
-- length/null/elem/sum/product/maximum/minimum/foldMap are generic over
-- them, and foldMap uses the Monoid class (String and list instances).

main :: IO ()
main = do
    -- class methods at each instance
    assert (foldr (\x acc -> x + acc) 0 [1, 2, 3] == 6) "foldr list"
    assert (foldl (\acc x -> acc - x) 10 [1, 2, 3] == 4) "foldl list"
    assert (foldr (\x acc -> x + acc) 1 (Just 5) == 6) "foldr Just"
    assert (foldr (\x acc -> x + acc) 1 (Nothing :: Maybe Integer) == 1) "foldr Nothing"
    assert (foldl (\acc x -> acc + x) 1 (Just 5) == 6) "foldl Just"
    assert (foldr (\x acc -> x + acc) 1 (Right 5 :: Either String Integer) == 6) "foldr Right"
    assert (foldr (\x acc -> x + acc) 1 (Left "e" :: Either String Integer) == 1) "foldr Left"
    assert (foldl (\acc x -> acc + x) 1 (Right 5 :: Either String Integer) == 6) "foldl Right"
    -- generic functions over lists
    assert (length [1, 2, 3] == 3) "length list"
    assert (null ([] :: [Integer])) "null empty list"
    assert (not (null [1])) "null nonempty"
    assert (elem 3 [1, 2, 3]) "elem hit"
    assert (not (elem 4 [1, 2, 3])) "elem miss"
    assert (sum [1, 2, 3, 4] == 10) "sum list"
    assert (sum ([] :: [Integer]) == 0) "sum empty"
    assert (product [1, 2, 3, 4] == 24) "product list"
    assert (maximum [3, 1, 4, 1, 5] == 5) "maximum list"
    assert (minimum [3, 1, 4, 1, 5] == 1) "minimum list"
    assert (maximum ["a", "c", "b"] == "c") "maximum strings"
    -- generic functions over Maybe / Either
    assert (length (Just 9) == 1) "length Just"
    assert (length (Nothing :: Maybe Integer) == 0) "length Nothing"
    assert (null (Nothing :: Maybe Integer)) "null Nothing"
    assert (not (null (Just 1))) "null Just"
    assert (elem 5 (Just 5)) "elem Just"
    assert (not (elem 5 (Nothing :: Maybe Integer))) "elem Nothing"
    assert (sum (Just 6) == 6) "sum Just"
    assert (product (Nothing :: Maybe Integer) == 1) "product Nothing"
    assert (maximum (Just 7) == 7) "maximum Just"
    assert (length (Right 1 :: Either String Integer) == 1) "length Right"
    assert (sum (Left "e" :: Either String Integer) == 0) "sum Left"
    -- foldMap with the String and list monoids; mempty/mappend directly
    assert (foldMap show [1, 2, 3] == "123") "foldMap show"
    assert (foldMap show (Just 42) == "42") "foldMap Maybe"
    assert (foldMap show (Nothing :: Maybe Integer) == "") "foldMap Nothing is mempty"
    assert (foldMap (\x -> [x, x * 10]) [1, 2] == [1, 10, 2, 20]) "foldMap list monoid"
    assert (mappend "foo" "bar" == "foobar") "mappend String"
    assert (mappend [1, 2] [3] == [1, 2, 3]) "mappend list"
    assert (mappend mempty [7] == [7]) "mempty list"
    assert ((mempty :: String) == "") "mempty String"
    -- laziness parity with the old list-only definitions:
    -- null/elem short-circuit on infinite lists
    assert (not (null (iterate (\x -> x + 1) 1))) "null infinite"
    assert (elem 5 (iterate (\x -> x + 1) 1)) "elem infinite"
