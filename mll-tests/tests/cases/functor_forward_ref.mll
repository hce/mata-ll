-- A derived Functor whose field is a LATER-declared (or mutually
-- recursive) container must call that container's own fmap.
-- Regression: resolving against the not-yet-populated instance registry
-- fell back to the DERIVING type's fmap, so the inner value was
-- destructured with the outer type's constructor patterns (runtime
-- crash). The prescan now predicts `fmap_T` for every type that will
-- have derived (or bare-headed declared) Functor by module end.

data Outer a = MkOuter (Inner a) | ONone
    deriving (Functor, Show)

data Inner a = MkInner a a
    deriving (Functor, Show)

-- mutual recursion: each fmap references the other
data A a = MkA (B a) | ALeaf a
    deriving (Functor, Show)

data B a = MkB (A a) | BNil
    deriving (Functor, Show)

main :: IO ()
main = do
    print (fmap (\n -> n + 1) (MkOuter (MkInner 1 2)))
    print (fmap (\n -> n + 1) (ONone :: Outer Int))
    print (fmap (\n -> n * 2) (MkA (MkB (MkA (MkB (ALeaf 10))))))
    print (fmap (\n -> n * 2) (MkB (ALeaf 3)))
