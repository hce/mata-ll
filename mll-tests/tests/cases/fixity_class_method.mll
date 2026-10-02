import FixityClass

data Wrap = Wrap Int deriving (Show, Eq)

instance Mix Wrap where
    (Wrap a) <+> (Wrap b) = Wrap (a - b)
    blend (Wrap a) (Wrap b) = Wrap (a - b)

early :: Int
early = 10 |.| 3 |.| 2

infixr 5 |.|
(|.|) :: Int -> Int -> Int
a |.| b = a - b

late :: Int
late = 10 |.| 3 |.| 2

main :: IO ()
main = do
    assert (mixed == 9) "the class's module groups its method by the declaration"
    assert (blended == 9) "and its backtick method"
    assert ((10 :: Int) <+> 3 <+> 2 == 9) "an imported class method arrives with its fixity"
    assert ((10 :: Int) `blend` 3 `blend` 2 == 9) "an imported backtick method too"
    assert (Wrap 10 <+> Wrap 3 <+> Wrap 2 == Wrap 9) "at an instance of the importer"
    assert (early == 9) "a fixity declaration governs uses above it"
    assert (late == 9) "and below it"
    print (mixed, blended, (10 :: Int) <+> 3 <+> 2, (10 :: Int) `blend` 3 `blend` 2)
    print (Wrap 10 <+> Wrap 3 <+> Wrap 2, early, late)
